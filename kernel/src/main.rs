#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

// Bring hal's print!/println!/serial_print!/serial_println! into scope for the
// entire kernel binary without per-file use statements.
#[macro_use]
extern crate hal;

mod allocator;
mod cmd;
mod env;
mod idt;
mod keyboard;
mod memory;
mod ramfs;
mod shell;
mod vfs;

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
    let t0 = hal::time::rdtsc();

    hal::serial::SERIAL1.lock().init();
    let t_serial = hal::time::rdtsc();
    hal::time::calibrate(); // PIT-based TSC calibration (~10 ms); must precede cycles_to_ns
    serial_println!("AmaterasuOS booting...");
    serial_println!("[BOOT] t0={} (baseline)", t0);
    serial_println!("[BOOT] TSC:              {} MHz", hal::time::tsc_mhz());
    serial_println!("[BOOT] serial_init:      +{} ns", hal::time::cycles_to_ns(t_serial - t0));

    let phys_offset = boot_info.physical_memory_offset
        .into_option()
        .expect("physical memory mapping not provided") as usize;

    memory::init(&boot_info.memory_regions, phys_offset);
    let t_mem = hal::time::rdtsc();
    serial_println!("[BOOT] memory_init:      +{} ns", hal::time::cycles_to_ns(t_mem - t0));

    allocator::init(memory::heap_start_virt(), memory::heap_size());
    let t_alloc = hal::time::rdtsc();
    serial_println!("[BOOT] allocator_init:   +{} ns", hal::time::cycles_to_ns(t_alloc - t0));

    let rd_addr = boot_info.ramdisk_addr.into_option().unwrap_or(0);
    let rd_len  = boot_info.ramdisk_len as usize;
    ramfs::init(rd_addr, rd_len, phys_offset);
    env::init();

    if let Some(rsdp_phys) = boot_info.rsdp_addr.into_option() {
        hal::acpi::init(rsdp_phys, phys_offset);
    } else {
        serial_println!("[ACPI] no RSDP address from bootloader");
    }
    let t_acpi = hal::time::rdtsc();
    serial_println!("[BOOT] acpi_init:        +{} ns", hal::time::cycles_to_ns(t_acpi - t0));

    if let Some(fb) = boot_info.framebuffer.as_mut() {
        let info = fb.info();
        let buffer = fb.buffer_mut();

        for byte in buffer.iter_mut() {
            *byte = 0x00;
        }

        *hal::framebuffer::WRITER.lock() =
            Some(hal::framebuffer::FramebufferWriter::new(buffer, info));
    }
    let t_fb = hal::time::rdtsc();
    serial_println!("[BOOT] framebuffer_init: +{} ns", hal::time::cycles_to_ns(t_fb - t0));

    if !shell::print_file("/sys/welcome") {
        println!("AmaterasuOS");
    }

    unsafe { hal::pic::remap(); } // move PIC vectors to 0x20-0x2F before disabling
    hal::apic::init(phys_offset); // mask PIC, enable LAPIC + I/O APIC, route keyboard
    let t_apic = hal::time::rdtsc();
    serial_println!("[BOOT] apic_init:        +{} ns", hal::time::cycles_to_ns(t_apic - t0));

    idt::init();
    unsafe { core::arch::asm!("sti"); }

    hal::timer::init(); // calibrate LAPIC timer and start 1 ms periodic IRQ
    let t_done = hal::time::rdtsc();
    serial_println!("[BOOT] kernel_ready:     +{} ns (total)", hal::time::cycles_to_ns(t_done - t0));

    shell::prompt();

    loop {
        unsafe { core::arch::asm!("hlt"); }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe {
        hal::serial::SERIAL1.force_unlock();
        hal::framebuffer::WRITER.force_unlock();
    }

    serial_println!("\n--- KERNEL PANIC ---");
    if let Some(loc) = info.location() {
        serial_println!("Location: {}:{}:{}", loc.file(), loc.line(), loc.column());
    }
    serial_println!("Message:  {}", info.message());
    serial_println!("--------------------");

    if let Some(w) = hal::framebuffer::WRITER.lock().as_mut() {
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
