# AmaterasuOS (AMTOS)

An operating system written in Rust, optimized for boot time.

**Goal:** Power button to usable shell in under 3 seconds on modern x86_64 UEFI hardware.

**Status:** Pre-alpha. Kernel does not yet boot.

## Build

Requires nightly Rust. The `rust-toolchain.toml` file handles this automatically.

```
cargo build
```

## Run

TBD - will use QEMU with OVMF UEFI firmware.

## Specifications

- Speed first. Every design decision is evaluated against boot-time and runtime performance.
- No legacy baggage. Targeting modern x86_64 UEFI only. No BIOS, no 32-bit, no ancient hardware support.
- Parallelism from day one. Async driver init, no serial probing.
- Static where possible. Compile-time knowledge of hardware > runtime discovery.