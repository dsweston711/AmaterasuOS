# AmaterasuOS (AMTOS)

A bare-metal x86_64 operating system written in Rust, optimized for boot time.

**Goal:** Power button to usable shell in under 3 seconds on real x86_64 UEFI hardware.

**Status:** Pre-alpha. Boots via UEFI/OVMF in QEMU with an interactive shell and ~20 built-in commands. UEFI boot path verified end-to-end. Next milestone: bootable USB image for first real-hardware boot.

## Requirements

- Nightly Rust (`rust-toolchain.toml` handles this automatically)
- QEMU: `sudo apt install qemu-system-x86`
- OVMF (for UEFI boot and integration tests): `sudo apt install ovmf`

## Build & Run

**BIOS boot (quick iteration):**
```
make run
```

**UEFI boot (matches real hardware path):**
```
make run-uefi
```

Both targets build the kernel and disk image automatically before launching QEMU. Serial output — boot timing markers and any panic info — streams to your terminal.

## Testing

```
make test          # run all tests
make test-unit     # host-side unit tests (pure logic, no QEMU)
make test-integration  # boot kernel in QEMU via UEFI/OVMF, assert kernel_ready
```

Unit tests cover the shell's pure-logic layers: env expansion, command splitting (`&&` and `;`), flag parsing, and tilde expansion. Integration tests boot the full kernel in QEMU and check boot stage timings against regression budgets.

CI runs both test tiers on every pull request. Direct pushes to `main` are blocked — all changes go through a PR with green CI.

## Boot time

Current baseline (QEMU, UEFI/OVMF, GitHub Actions, debug build): **~222 ms** from kernel entry to ready.

| Stage | Time |
|-------|------|
| serial_init | ~451 µs |
| memory_init | ~25.6 ms |
| framebuffer_init | ~204 ms |
| **kernel_ready** | **~222 ms** |

Framebuffer init dominates (~92% of total). See [docs/boot-time-log.md](docs/boot-time-log.md) for full history.

## Shell commands

`cat` `cd` `clear` `cpu` `echo` `export` `grep` `head` `heap` `help` `history` `hostname` `ls` `pwd` `reboot` `shutdown` `stat` `tail` `uname` `uptime` `wc`

## Design principles

- **Speed first.** Every design decision is evaluated against boot time and runtime performance.
- **No legacy baggage.** Targeting modern x86_64 UEFI only — no BIOS, no 32-bit, no ancient hardware support.
- **Parallelism from day one.** Async driver init, no serial probing.
- **Static where possible.** Compile-time knowledge of hardware over runtime discovery.
