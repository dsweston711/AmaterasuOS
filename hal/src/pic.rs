const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

const ICW1_INIT: u8 = 0x11; // initialize + cascade + ICW4 required
const ICW4_8086: u8 = 0x01;
const EOI: u8 = 0x20;

pub unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") val,
        options(nomem, nostack, preserves_flags)
    );
}

pub unsafe fn outw(port: u16, val: u16) {
    core::arch::asm!(
        "out dx, ax",
        in("dx") port,
        in("ax") val,
        options(nomem, nostack, preserves_flags)
    );
}

pub unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!(
        "in al, dx",
        out("al") val,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    val
}

pub unsafe fn outl(port: u16, val: u32) {
    core::arch::asm!(
        "out dx, eax",
        in("dx") port, in("eax") val,
        options(nomem, nostack, preserves_flags)
    );
}

pub unsafe fn inl(port: u16) -> u32 {
    let val: u32;
    core::arch::asm!(
        "in eax, dx",
        out("eax") val, in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    val
}

// A short I/O delay by writing to an unused port; gives the PIC time to settle.
pub unsafe fn io_wait() {
    outb(0x80, 0x00);
}

/// Mask every IRQ line on both PICs without touching the CMD ports.
/// Safe to call before the IDT is loaded; only writes to DATA ports (0x21/0xA1).
/// Use this instead of remap() on UEFI systems where CMD port writes may be
/// trapped by firmware SMM handlers and cause a hang.
pub unsafe fn mask_all() {
    outb(PIC1_DATA, 0xFF);
    io_wait();
    outb(PIC2_DATA, 0xFF);
    io_wait();
}

/// Remap both PICs so IRQ0-7 -> 0x20-0x27 and IRQ8-15 -> 0x28-0x2F,
/// moving them out of the range reserved for CPU exceptions (0x00-0x1F).
/// NOTE: On some UEFI systems (e.g. Z390), writing ICW1_INIT to CMD ports
/// (0x20/0xA0) triggers an SMI trap and hangs. Prefer mask_all() + cli.
pub unsafe fn remap() {
    // Save existing masks
    let mask1 = inb(PIC1_DATA);
    let mask2 = inb(PIC2_DATA);

    // ICW1: start initialization sequence
    outb(PIC1_CMD, ICW1_INIT); io_wait();
    outb(PIC2_CMD, ICW1_INIT); io_wait();

    // ICW2: vector offsets
    outb(PIC1_DATA, 0x20); io_wait(); // IRQ0-7  -> int 0x20-0x27
    outb(PIC2_DATA, 0x28); io_wait(); // IRQ8-15 -> int 0x28-0x2F

    // ICW3: cascade wiring (PIC1 bit 2 = IRQ2 has slave; PIC2 identity = 2)
    outb(PIC1_DATA, 0x04); io_wait();
    outb(PIC2_DATA, 0x02); io_wait();

    // ICW4: 8086 mode
    outb(PIC1_DATA, ICW4_8086); io_wait();
    outb(PIC2_DATA, ICW4_8086); io_wait();

    // Restore masks
    outb(PIC1_DATA, mask1);
    outb(PIC2_DATA, mask2);
}

/// Mask every IRQ line except IRQ1 (keyboard).
pub unsafe fn enable_keyboard_only() {
    outb(PIC1_DATA, 0xFD); // 1111_1101: unmask only IRQ1
    outb(PIC2_DATA, 0xFF); // mask all of PIC2
}

/// Send end-of-interrupt to the relevant PIC(s).
pub unsafe fn end_of_interrupt(irq: u8) {
    if irq >= 8 {
        outb(PIC2_CMD, EOI);
    }
    outb(PIC1_CMD, EOI);
}

const ACPI_PM1A_CTRL: u16 = 0x0604;
const ACPI_SLP_S5:    u16 = 0x2000;

pub unsafe fn acpi_power_off() {
    outw(ACPI_PM1A_CTRL, ACPI_SLP_S5);
}
