use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use spin::Mutex;

const SLAB_SIZES:       [usize; 6] = [8, 16, 32, 64, 128, 256];
const OBJECTS_PER_CLASS: usize     = 512;

// Total bytes reserved for slab pools: 512 * (8+16+32+64+128+256) = 258 048 B ≈ 252 KB
const SLAB_REGION_BYTES: usize = OBJECTS_PER_CLASS * (8 + 16 + 32 + 64 + 128 + 256);

// ── Slab class ───────────────────────────────────────────────────────────────

struct SlabClass {
    obj_size:     usize,
    region_start: usize, // virtual base of this class's pool
    free_head:    usize, // 0 = empty; otherwise points to next free block
    free_count:   usize,
    total:        usize,
}

impl SlabClass {
    const fn new(obj_size: usize) -> Self {
        Self { obj_size, region_start: 0, free_head: 0, free_count: 0, total: 0 }
    }

    fn init(&mut self, region_start: usize, count: usize) {
        self.region_start = region_start;
        self.total        = count;
        self.free_count   = count;
        // Thread each block's first word into a singly-linked freelist.
        for i in 0..count {
            let block = region_start + i * self.obj_size;
            let next  = if i + 1 < count { region_start + (i + 1) * self.obj_size } else { 0 };
            unsafe { *(block as *mut usize) = next; }
        }
        self.free_head = region_start;
    }

    fn alloc(&mut self) -> *mut u8 {
        if self.free_head == 0 { return ptr::null_mut(); }
        let block      = self.free_head;
        self.free_head = unsafe { *(block as *const usize) };
        self.free_count -= 1;
        block as *mut u8
    }

    fn dealloc(&mut self, ptr: *mut u8) {
        unsafe { *(ptr as *mut usize) = self.free_head; }
        self.free_head = ptr as usize;
        self.free_count += 1;
    }

    fn owns(&self, ptr: *mut u8) -> bool {
        let addr = ptr as usize;
        addr >= self.region_start && addr < self.region_start + self.total * self.obj_size
    }
}

// ── Bump allocator (fallback for allocations > 256 B) ───────────────────────

struct Bump {
    current: usize,
    end:     usize,
}

impl Bump {
    const fn new() -> Self { Self { current: 0, end: 0 } }

    fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let start = align_up(self.current, layout.align());
        let end   = start.saturating_add(layout.size());
        if end > self.end { return ptr::null_mut(); }
        self.current = end;
        start as *mut u8
    }

    fn used(&self)     -> usize { self.current.saturating_sub(self.start()) }
    fn capacity(&self) -> usize { self.end.saturating_sub(self.start()) }
    fn start(&self)    -> usize { self.end.saturating_sub(crate::memory::heap_size() - SLAB_REGION_BYTES) }
}

// ── Combined allocator ───────────────────────────────────────────────────────

struct SlabAllocator {
    initialized: bool,
    heap_start:  usize,
    slabs:       [SlabClass; 6],
    bump:        Bump,
}

impl SlabAllocator {
    const fn new() -> Self {
        Self {
            initialized: false,
            heap_start:  0,
            slabs: [
                SlabClass::new(8),
                SlabClass::new(16),
                SlabClass::new(32),
                SlabClass::new(64),
                SlabClass::new(128),
                SlabClass::new(256),
            ],
            bump: Bump::new(),
        }
    }

    fn init(&mut self, heap_virt: usize, heap_size: usize) {
        self.heap_start = heap_virt;
        let mut offset  = 0;
        for i in 0..6 {
            self.slabs[i].init(heap_virt + offset, OBJECTS_PER_CLASS);
            offset += SLAB_SIZES[i] * OBJECTS_PER_CLASS;
        }
        // Bump region fills the rest.
        self.bump.current = heap_virt + offset;
        self.bump.end     = heap_virt + heap_size;
        self.initialized  = true;
    }

    fn alloc(&mut self, layout: Layout) -> *mut u8 {
        if !self.initialized { return ptr::null_mut(); }
        // Try each slab class that fits, smallest first; fall through if exhausted.
        for slab in &mut self.slabs {
            if layout.size() <= slab.obj_size && layout.align() <= slab.obj_size {
                let p = slab.alloc();
                if !p.is_null() { return p; }
            }
        }
        self.bump.alloc(layout)
    }

    fn dealloc(&mut self, ptr: *mut u8, _layout: Layout) {
        for slab in &mut self.slabs {
            if slab.owns(ptr) { slab.dealloc(ptr); return; }
        }
        // Bump allocations are not individually freed.
    }
}

// ── Heap stats (for the `heap` shell command in #20) ────────────────────────

pub struct HeapStats {
    pub heap_start:    usize,
    pub heap_size:     usize,
    pub bump_used:     usize,
    pub bump_capacity: usize,
    pub slab_sizes:    [usize; 6],
    pub slab_free:     [usize; 6],
    pub slab_total:    [usize; 6],
}

pub fn stats() -> HeapStats {
    let a = ALLOCATOR.0.lock();
    let mut slab_free  = [0usize; 6];
    let mut slab_total = [0usize; 6];
    for (i, s) in a.slabs.iter().enumerate() {
        slab_free[i]  = s.free_count;
        slab_total[i] = s.total;
    }
    HeapStats {
        heap_start:    a.heap_start,
        heap_size:     crate::memory::heap_size(),
        bump_used:     a.bump.used(),
        bump_capacity: a.bump.capacity(),
        slab_sizes:    SLAB_SIZES,
        slab_free,
        slab_total,
    }
}

// ── GlobalAlloc wiring ───────────────────────────────────────────────────────

fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

struct LockedAllocator(Mutex<SlabAllocator>);

unsafe impl GlobalAlloc for LockedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.0.lock().alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.0.lock().dealloc(ptr, layout)
    }
}

#[global_allocator]
static ALLOCATOR: LockedAllocator = LockedAllocator(Mutex::new(SlabAllocator::new()));

pub fn init(heap_virt: usize, heap_size: usize) {
    ALLOCATOR.0.lock().init(heap_virt, heap_size);
    serial_println!(
        "[ALLOC] heap {:#012x}, {} MB | slabs: {:?} x{} ({} KB) | bump: {} KB",
        heap_virt,
        heap_size / (1024 * 1024),
        SLAB_SIZES,
        OBJECTS_PER_CLASS,
        SLAB_REGION_BYTES / 1024,
        (heap_size - SLAB_REGION_BYTES) / 1024,
    );
}
