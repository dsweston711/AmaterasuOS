#![no_std]
#![no_main]

mod serial;

use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    serial::SERIAL1.lock().init();
    serial_println!("AmaterasuOS booting...");

    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let info = framebuffer.info();
        let buffer = framebuffer.buffer_mut();

        for byte in buffer.iter_mut() {
            *byte = 0x90;
        }

        let stride = info.stride;
        let bytes_per_pixel = info.bytes_per_pixel;
        for y in 10..60 {
            for x in 10..200 {
                let offset = (y * stride + x) * bytes_per_pixel;
                if offset + bytes_per_pixel <= buffer.len() {
                    buffer[offset] = 0xFF;
                    buffer[offset + 1] = 0xFF;
                    buffer[offset + 2] = 0xFF;
                }
            }
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
