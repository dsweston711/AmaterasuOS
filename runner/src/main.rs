use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Step 1: Build the kernel.
    let status = Command::new("cargo")
        .args(["build", "--package", "amaterasu_kernel", "--target", "x86_64-unknown-none"])
        .status()
        .expect("Failed to invoke cargo for kernel build");
    assert!(status.success(), "Kernel build failed");

    // Step 2: Locate the kernel binary.
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().to_path_buf();
    let kernel_path = workspace_root
        .join("target/x86_64-unknown-none/debug/amaterasu_kernel");
    assert!(kernel_path.exists(), "Kernel binary not found at {:?}", kernel_path);

    // Step 3: Build the disk images via the `boot` crate.
    let output = Command::new("cargo")
        .args(["run", "--package", "boot"])
        .env("KERNEL_PATH", &kernel_path)
        .env("OUT_DIR", workspace_root.join("target"))
        .output()
        .expect("failed to invoke boot crate");
    assert!(output.status.success(), "image build failed: {}",
        String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    let bios_path = lines.iter().find(|l| l.contains("bios")).expect("no BIOS path");
    let _uefi_path = lines.iter().find(|l| l.contains("uefi")).expect("no UEFI path");

    // Step 4: Launch QEMU with the BIOS image.
    println!("Launching QEMU with {}", bios_path);
    let status = Command::new("qemu-system-x86_64")
        .args([
            "-drive", &format!("format=raw,file={}", bios_path),
            "-serial", "stdio",
            "-m", "128M",
        ])
        .status()
        .expect("failed to launch QEMU");

    std::process::exit(status.code().unwrap_or(1));
}