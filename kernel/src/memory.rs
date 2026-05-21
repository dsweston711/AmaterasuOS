use bootloader_api::info::{MemoryRegionKind, MemoryRegions};
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

pub const HEAP_SIZE: usize = 16 * 1024 * 1024; // 16 MB

static HEAP_PHYS:   AtomicU64   = AtomicU64::new(0);
static PHYS_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Parse the bootloader memory map, log all usable regions to serial,
/// and anchor the heap at the first usable region large enough to hold HEAP_SIZE.
pub fn init(regions: &MemoryRegions, phys_offset: usize) {
    PHYS_OFFSET.store(phys_offset, Ordering::Relaxed);
    crate::serial_println!("[MEM] Usable memory regions:");

    let mut heap_found = false;

    for region in regions.iter() {
        if region.kind != MemoryRegionKind::Usable {
            continue;
        }
        let size = region.end - region.start;
        crate::serial_println!(
            "[MEM]   {:#012x}..{:#012x}  ({} KB)",
            region.start, region.end, size / 1024
        );

        if !heap_found && size >= HEAP_SIZE as u64 {
            HEAP_PHYS.store(region.start, Ordering::Relaxed);
            heap_found = true;
            crate::serial_println!(
                "[MEM] Heap reserved: {:#012x} + {} MB (virt {:#012x})",
                region.start,
                HEAP_SIZE / (1024 * 1024),
                region.start as usize + phys_offset,
            );
        }
    }

    if !heap_found {
        panic!("no usable memory region large enough for the heap ({} MB required)", HEAP_SIZE / (1024 * 1024));
    }
}

pub fn heap_start_virt() -> usize {
    HEAP_PHYS.load(Ordering::Relaxed) as usize + PHYS_OFFSET.load(Ordering::Relaxed)
}

pub fn heap_size() -> usize {
    HEAP_SIZE
}
