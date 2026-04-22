#![no_std]
#![no_main]

mod framebuffer;
mod serial;

use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    serial::SERIAL1.lock().init();
    serial_println!("AmaterasuOS booting...");

    if let Some(fb) = boot_info.framebuffer.as_mut() {
        let info = fb.info();
        let buffer = fb.buffer_mut();

        // Clear to black.
        for byte in buffer.iter_mut() {
            *byte = 0x00;
        }

        // Draw a test string glyph-by-glyph to verify write_glyph works.
        let msg = "AmaterasuOS";
        for (i, ch) in msg.chars().enumerate() {
            framebuffer::write_glyph(
                buffer,
                &info,
                ch,
                8 + i * framebuffer::GLYPH_W,
                8,
                [0xFF, 0xFF, 0xFF], // white fg
                [0x00, 0x00, 0x00], // black bg
            );
        }
    }

    serial_println!("Framebuffer initialized.");

    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Force-unlock in case we panicked while holding the serial lock.
    unsafe { serial::SERIAL1.force_unlock(); }
    serial_println!("KERNEL PANIC: {}", info);
    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}
