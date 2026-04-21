#![no_std]
#![no_main]

use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // Grab the framebuffer the bootloader set up for us.
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let info = framebuffer.info();
        let buffer = framebuffer.buffer_mut();

        // Paint the whole screen a color (RGB-ish, depends on pixel format).
        // For now, just fill with a solid value so we know we're alive.
        for byte in buffer.iter_mut() {
            *byte = 0x90; // mid-gray-ish
        }

        // Poke a bright rectangle in the top-left so we know writes work.
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

    loop {
        // Halt the CPU until the next interrupt. Saves power, doesn't spin.
        unsafe { core::arch::asm!("hlt"); }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}