// PS/2 controller (Intel 8042) initialization.
//
// After UEFI ExitBootServices() the controller is often in a stale state:
// OBF set with a 0xFE "Resend" byte, keyboard port disabled, IRQ1 disabled.
// This init sequence resets it to a known-good state so USB legacy emulation
// (SMM-based) can deliver keystrokes to port 0x60.

use crate::pic::{inb, outb, io_wait};

const CMD:  u16 = 0x64; // command/status port
const DATA: u16 = 0x60; // data port

// Wait for the Input Buffer Full flag to clear (safe to write a command/data).
// Returns false on timeout.
unsafe fn wait_write() -> bool {
    for _ in 0..0x10000u32 {
        if inb(CMD) & 0x02 == 0 { return true; }
        io_wait();
    }
    false
}

// Wait for the Output Buffer Full flag to set (data is ready to read).
// Returns false on timeout.
unsafe fn wait_read() -> bool {
    for _ in 0..0x10000u32 {
        if inb(CMD) & 0x01 != 0 { return true; }
        io_wait();
    }
    false
}

// Drain any stale bytes sitting in the output buffer.
unsafe fn flush() {
    for _ in 0..16 {
        if inb(CMD) & 0x01 == 0 { break; }
        inb(DATA);
        io_wait();
    }
}

pub unsafe fn init() {
    // Step 1: Disable both PS/2 ports during configuration.
    wait_write(); outb(CMD, 0xAD); // disable port 1 (keyboard)
    wait_write(); outb(CMD, 0xA7); // disable port 2 (mouse); no-op if absent

    // Step 2: Flush all stale output (including the 0xFE Resend byte).
    flush();

    // Step 3: Read the current Controller Configuration Byte.
    wait_write(); outb(CMD, 0x20);
    let config = if wait_read() { inb(DATA) } else { 0x00 };
    crate::serial_println!("[PS2] config byte = {:#04x}", config);
    crate::println!(    "[PS2] config byte = {:#04x}", config);

    // Step 4: Modify config:
    //   Set   bit 0 — enable IRQ1 (keyboard interrupt)
    //   Clear bit 4 — enable port 1 clock (was disabled by 0xAD above)
    //   Leave bit 6 (translation) unchanged: QEMU needs it on (Set 2 → Set 1);
    //   on real UEFI hardware the PS/2 path is replaced by XHCI so it's moot.
    let new_config = (config | 0x01) & !0x10;
    wait_write(); outb(CMD, 0x60); // Write Configuration Byte command
    wait_write(); outb(DATA, new_config);

    // Step 5: Re-enable port 1.
    wait_write(); outb(CMD, 0xAE);

    crate::serial_println!("[PS2] init done, new config = {:#04x}", new_config);
    crate::println!(    "[PS2] init done, new config = {:#04x}", new_config);
}
