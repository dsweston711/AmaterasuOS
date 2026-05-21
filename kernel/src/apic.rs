use core::sync::atomic::{AtomicUsize, Ordering};

// ── LAPIC register offsets ───────────────────────────────────────────────────

const LAPIC_ID:  u32 = 0x020;
const LAPIC_TPR: u32 = 0x080; // Task Priority Register
const LAPIC_EOI: u32 = 0x0B0; // End-of-Interrupt
const LAPIC_SVR: u32 = 0x0F0; // Spurious Interrupt Vector

const SVR_ENABLE:   u32 = 1 << 8; // LAPIC software-enable bit
const SVR_SPURIOUS: u32 = 0xFF;   // spurious vector number

// ── Cached LAPIC virtual base ────────────────────────────────────────────────

static LAPIC_BASE: AtomicUsize = AtomicUsize::new(0);

// ── LAPIC MMIO helpers ───────────────────────────────────────────────────────

pub(crate) unsafe fn lapic_write(offset: u32, val: u32) {
    let addr = (LAPIC_BASE.load(Ordering::Relaxed) + offset as usize) as *mut u32;
    addr.write_volatile(val);
}

pub(crate) unsafe fn lapic_read(offset: u32) -> u32 {
    let addr = (LAPIC_BASE.load(Ordering::Relaxed) + offset as usize) as *const u32;
    addr.read_volatile()
}

// ── I/O APIC indirect-register helpers ──────────────────────────────────────

// The I/O APIC uses an index/data register pair at offsets 0x00 and 0x10.
unsafe fn ioapic_write(base: usize, reg: u8, val: u32) {
    (base as *mut u32).write_volatile(reg as u32);          // IOREGSEL
    ((base + 0x10) as *mut u32).write_volatile(val);        // IOWIN
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Signal end-of-interrupt to the LAPIC.  Called from interrupt handlers.
#[inline]
pub fn end_of_interrupt() {
    unsafe { lapic_write(LAPIC_EOI, 0); }
}

pub fn init() {
    let phys_off = crate::memory::phys_offset();

    let info = crate::acpi::get()
        .expect("apic::init requires ApicInfo from acpi::init");

    let lapic_virt  = info.lapic_addr  as usize + phys_off;
    let ioapic_virt = info.ioapic_addr as usize + phys_off;

    LAPIC_BASE.store(lapic_virt, Ordering::Relaxed);

    unsafe {
        // ── 1. Fully mask the 8259 PIC ───────────────────────────────────
        // pic::remap() has already moved PIC vectors to 0x20-0x2F so any
        // remaining spurious PIC interrupts won't hit CPU exception vectors.
        crate::pic::outb(0x21, 0xFF); // mask all PIC1 lines
        crate::pic::outb(0xA1, 0xFF); // mask all PIC2 lines

        // ── 2. IMCR: disconnect 8259 from the BSP INTR pin ──────────────
        // MP-spec IMCR register: select 0x70 via port 0x22, write 1 to 0x23.
        crate::pic::outb(0x22, 0x70);
        crate::pic::outb(0x23, 0x01);

        // ── 3. Enable the LAPIC ──────────────────────────────────────────
        let lapic_id = lapic_read(LAPIC_ID) >> 24;
        lapic_write(LAPIC_SVR, SVR_ENABLE | SVR_SPURIOUS);
        lapic_write(LAPIC_TPR, 0); // accept all interrupt classes

        crate::serial_println!(
            "[APIC] LAPIC id={} enabled at virt {:#010x}",
            lapic_id, lapic_virt
        );

        // ── 4. Program I/O APIC redirection for keyboard (IRQ 1) ────────
        // Global System Interrupt = IRQ1 (PCI/ISA) - gsi_base gives the
        // index into this I/O APIC's redirection table.
        let kbd_gsi   = 1u32; // keyboard is always ISA IRQ 1
        let entry_idx = kbd_gsi - info.ioapic_gsi_base;
        let ioredtbl_lo = 0x10u8 + (2 * entry_idx) as u8;
        let ioredtbl_hi = ioredtbl_lo + 1;

        // Low dword: vector=0x21, fixed delivery, physical dest, active-high,
        //            edge-triggered, not masked.
        ioapic_write(ioapic_virt, ioredtbl_lo, 0x00000021);
        // High dword: bits 31:24 = destination LAPIC ID (BSP = 0).
        ioapic_write(ioapic_virt, ioredtbl_hi, (lapic_id as u32) << 24);

        crate::serial_println!(
            "[APIC] I/O APIC virt {:#010x}: IRQ1 → vec 0x21 → LAPIC {}",
            ioapic_virt, lapic_id
        );
    }
}
