use std::path::PathBuf;

fn main() {
    // The path to the kernel binary is passed via an env var set by cargo.
    let kernel_path = std::env::var("KERNEL_PATH").expect("KERNEL_PATH env var not set");
    let kernel = PathBuf::from(kernel_path);

    let out_dir = PathBuf::from(
        std::env::var("OUT_DIR").unwrap_or_else(|_| "target".into())
    );

    // Build both BIOS and UEFI images. BIOS is simpler to boot in QEMU
    // without extra flags, so we'll use it for now.
    let ramdisk_path = std::env::var("RAMDISK_PATH").ok().map(PathBuf::from);

    let bios_path = out_dir.join("amaterasu-bios.img");
    let mut bios = bootloader::BiosBoot::new(&kernel);
    if let Some(ref rd) = ramdisk_path { bios.set_ramdisk(rd); }
    bios.create_disk_image(&bios_path)
        .expect("Failed to create BIOS disk image");

    let uefi_path = out_dir.join("amaterasu-uefi.img");
    let mut uefi = bootloader::UefiBoot::new(&kernel);
    if let Some(ref rd) = ramdisk_path { uefi.set_ramdisk(rd); }
    uefi.create_disk_image(&uefi_path)
        .expect("Failed to create UEFI disk image");

    // Print the paths so the runner can pick them up.
    println!("{}", bios_path.display());
    println!("{}", uefi_path.display());
}