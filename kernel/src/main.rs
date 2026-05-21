#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod acpi;
mod allocator;
mod apic;
mod framebuffer;
mod idt;
mod keyboard;
mod memory;
mod pic;
mod serial;
mod shell;
mod time;

use bootloader_api::{entry_point, BootInfo, BootloaderConfig};
use bootloader_api::config::Mapping;
use core::panic::PanicInfo;

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    let t0 = time::rdtsc();

    serial::SERIAL1.lock().init();
    let t_serial = time::rdtsc();
    serial_println!("AmaterasuOS booting...");
    serial_println!("[BOOT] t0={} (baseline)", t0);
    serial_println!("[BOOT] serial_init:      +{} ns", time::cycles_to_ns(t_serial - t0));

    let phys_offset = boot_info.physical_memory_offset
        .into_option()
        .expect("physical memory mapping not provided") as usize;

    memory::init(&boot_info.memory_regions, phys_offset);
    let t_mem = time::rdtsc();
    serial_println!("[BOOT] memory_init:      +{} ns", time::cycles_to_ns(t_mem - t0));

    allocator::init(memory::heap_start_virt(), memory::heap_size());
    let t_alloc = time::rdtsc();
    serial_println!("[BOOT] allocator_init:   +{} ns", time::cycles_to_ns(t_alloc - t0));

    if let Some(rsdp_phys) = boot_info.rsdp_addr.into_option() {
        acpi::init(rsdp_phys, phys_offset);
    } else {
        serial_println!("[ACPI] no RSDP address from bootloader");
    }
    let t_acpi = time::rdtsc();
    serial_println!("[BOOT] acpi_init:        +{} ns", time::cycles_to_ns(t_acpi - t0));

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

    unsafe { pic::remap(); } // move PIC vectors to 0x20-0x2F before disabling
    apic::init();            // mask PIC, enable LAPIC + I/O APIC, route keyboard
    let t_apic = time::rdtsc();
    serial_println!("[BOOT] apic_init:        +{} ns", time::cycles_to_ns(t_apic - t0));

    idt::init();
    unsafe { core::arch::asm!("sti"); }

    let t_done = time::rdtsc();
    serial_println!("[BOOT] kernel_ready:     +{} ns (total)", time::cycles_to_ns(t_done - t0));

    shell::prompt();

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
