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

        for byte in buffer.iter_mut() {
            *byte = 0x00;
        }

        *framebuffer::WRITER.lock() =
            Some(framebuffer::FramebufferWriter::new(buffer, info));
    }

    println!("AmaterasuOS");
    println!("booting...");

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
