# AmaterasuOS (AMTOS)

An operating system written in Rust, optimized for boot time.

**Goal:** Power button to usable shell in under 3 seconds on modern x86_64 UEFI hardware.

**Status:** Pre-alpha. Kernel boots in QEMU — framebuffer, serial output, and panic handler working.

## Requirements

- Nightly Rust (`rust-toolchain.toml` handles this automatically)
- QEMU: `sudo apt install qemu-system-x86`

## Build & Run

```
cargo krun
```

Builds the kernel, creates a bootable disk image, and launches it in QEMU. Serial output (boot timing, panic info) goes to stdout.

## Boot time

Current baseline (QEMU, BIOS, debug build): **~364 ms** from kernel entry to ready.

See [docs/boot-time-log.md](docs/boot-time-log.md) for full history and methodology.

## Specifications

- Speed first. Every design decision is evaluated against boot-time and runtime performance.
- No legacy baggage. Targeting modern x86_64 UEFI only. No BIOS, no 32-bit, no ancient hardware support.
- Parallelism from day one. Async driver init, no serial probing.
- Static where possible. Compile-time knowledge of hardware > runtime discovery.
