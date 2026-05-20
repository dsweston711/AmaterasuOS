#![no_std]
#![no_main]

mod framebuffer;
mod serial;
mod time;

use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    let t0 = time::rdtsc();

    serial::SERIAL1.lock().init();
    let t_serial = time::rdtsc();
    serial_println!("AmaterasuOS booting...");
    serial_println!("[BOOT] t0={} (baseline)", t0);
    serial_println!("[BOOT] serial_init:      +{} ns", time::cycles_to_ns(t_serial - t0));

    if let Some(fb) = boot_info.framebuffer.as_mut() {
        let info = fb.info();
        let buffer = fb.buffer_mut();

        for byte in buffer.iter_mut() {
            *byte = 0x00;
        }

        *framebuffer::WRITER.lock() =
            Some(framebuffer::FramebufferWriter::new(buffer, info));
    }
    let t_fb = time::rdtsc();
    serial_println!("[BOOT] framebuffer_init: +{} ns", time::cycles_to_ns(t_fb - t0));

    println!("AmaterasuOS");
    println!("booting...");
    // panic!("This is a test");

    let t_done = time::rdtsc();
    serial_println!("[BOOT] kernel_ready:     +{} ns (total)", time::cycles_to_ns(t_done - t0));

    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe {
        serial::SERIAL1.force_unlock();
        framebuffer::WRITER.force_unlock();
    }

    serial_println!("\n--- KERNEL PANIC ---");
    if let Some(loc) = info.location() {
        serial_println!("Location: {}:{}:{}", loc.file(), loc.line(), loc.column());
    }
    serial_println!("Message:  {}", info.message());
    serial_println!("--------------------");

    if let Some(w) = framebuffer::WRITER.lock().as_mut() {
        w.set_colors([0xFF, 0xFF, 0xFF], [0xCC, 0x00, 0x00]);
        w.clear();
    }
    println!("--- KERNEL PANIC ---");
    if let Some(loc) = info.location() {
        println!("Location: {}:{}:{}", loc.file(), loc.line(), loc.column());
    }
    println!("Message:  {}", info.message());

    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}
