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
    let bios_path = out_dir.join("amaterasu-bios.img");
    bootloader::BiosBoot::new(&kernel)
        .create_disk_image(&bios_path)
        .expect("Failed to create BIOS disk image");

    let uefi_path = out_dir.join("amaterasu-uefi.img");
    bootloader::UefiBoot::new(&kernel)
        .create_disk_image(&uefi_path)
        .expect("Failed to create UEFI disk image");

    // Print the paths so the runner can pick them up.
    println!("{}", bios_path.display());
    println!("{}", uefi_path.display());
}