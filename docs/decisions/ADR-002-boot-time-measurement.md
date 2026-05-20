# ADR-002: Boot-time measurement strategy

**Date:** 2026-05-20

**Status:** Accepted

**Issue:** #4

**Milestone:** v0.1

## Context

We want to track how long the kernel takes to boot — from kernel entry to the last
initialization event — so we can log it in `docs/boot-time-log.md` and catch
regressions as the kernel grows.

The scope is: **kernel entry (`kernel_main`) to final milestone event**, not including
firmware POST or QEMU startup overhead. The measurement must work inside a `#![no_std]`
bare-metal kernel running under QEMU (BIOS boot path, via the `bootloader` crate).
Serial output is already in place (`SERIAL1` on COM1).

---

## Options surveyed

### 1. RDTSC (Read Timestamp Counter)

The `rdtsc` instruction returns a 64-bit cycle count from the CPU's timestamp counter.
On x86_64 it is a single instruction readable in bare metal via inline assembly — no
crates, no runtime, no boot-services context required.

```rust
#[inline(always)]
fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack));
    }
    ((hi as u64) << 32) | lo as u64
}
```

Read once at the very top of `kernel_main`, read again at each checkpoint, subtract
for a delta in cycles. Convert to nanoseconds using the TSC frequency.

**TSC frequency in QEMU:** QEMU emulates an invariant TSC (constant rate, independent
of guest P-states). The rate depends on the `-cpu` flag:
- Default (`-cpu qemu64`): typically 1 GHz virtual TSC.
- `-cpu host`: inherits host TSC rate (check with `grep 'cpu MHz' /proc/cpuinfo`).
- Can be fixed explicitly with `-cpu ...,tsc-frequency=1000000000`.

At this stage, hard-coding 1 GHz (1 cycle = 1 ns) is accurate enough for QEMU with
default settings. A proper PIT-calibration routine can replace this later when running
on real hardware is a goal.

**Pros:**
- Available from the very first instruction of `kernel_main`.
- Single instruction, negligible overhead.
- Works on BIOS boot path; no firmware services needed.
- Deltas are independent of absolute TSC value (no need to know the zero point).
- Serial is already wired up — printing timestamps is trivial.

**Cons:**
- Returns cycles; requires frequency knowledge to convert to wall-clock nanoseconds.
- TSC frequency must be calibrated or assumed (see above).
- Does not capture time before `kernel_main` (bootloader stages, BIOS POST).

---

### 2. UEFI Time Services (`EFI_RUNTIME_SERVICES.GetTime`)

UEFI firmware exposes `GetTime()` via its runtime services table, returning a
`EFI_TIME` struct with year/month/day/hour/min/sec/nanosecond fields.

**Why this does not apply here:**

1. We boot via the BIOS path. The `bootloader` crate produces both BIOS and UEFI images,
   but the runner launches QEMU with the BIOS image. UEFI runtime services are simply
   not present.
2. Even on the UEFI path, the `bootloader` crate calls `ExitBootServices()` before
   handing control to the kernel. After that point, using runtime services requires the
   kernel to have mapped the UEFI runtime memory regions — non-trivial setup.
3. Granularity: the nanosecond field exists in the struct, but most firmware
   implementations have 1 µs or coarser actual resolution. Not useful for sub-millisecond
   boot measurements.

**Verdict:** Ruled out. Not available on current BIOS path; requires significant
infrastructure even on UEFI path; insufficient resolution.

---

### 3. QEMU `-d` / `-trace` flags

QEMU's debug and trace systems can log internal events with host timestamps.

- `-d trace:<event>` logs specific QEMU trace events.
- `-d exec,nochain` logs every executed translation block.
- `-trace "event=*,file=trace.log"` writes structured trace output.

For boot-time purposes the most useful form is a host-side wall-clock wrapper: measure
the time from QEMU process spawn to when a sentinel string appears on the serial pipe.
The runner already invokes QEMU and owns the process handle; it could open a pipe to
`-serial stdio` (currently used) and record `Instant::now()` before and after.

**Pros:**
- No kernel changes; works from the runner.
- Captures total boot time including BIOS POST and bootloader stages.
- `-d trace:*` can expose firmware-level events (SeaBIOS stage boundaries, etc.).

**Cons:**
- Total time (QEMU launch → sentinel) includes QEMU startup overhead and BIOS POST,
  neither of which is "kernel time."
- Parsing serial output for a sentinel adds runner complexity.
- QEMU trace events require QEMU to be built with tracing enabled (not guaranteed in
  distro packages).
- Granularity limited by host timer resolution (typically 1 µs, but scheduling jitter
  makes it unreliable for sub-millisecond measurements).

**Verdict:** Useful as a secondary/complementary measurement of total QEMU-to-done
elapsed time, not as the primary in-kernel measurement tool.

---

### 4. External wall-clock (`std::time::Instant` in the runner)

Wrap the QEMU invocation in the runner with `Instant::now()`:

```rust
let t0 = std::time::Instant::now();
// ... launch qemu ...
let elapsed = t0.elapsed();
```

Optionally, detect a sentinel on serial output to record "kernel reached milestone X
at T+N ms from process start."

**Pros:**
- Zero kernel changes.
- Works for any boot path.
- Easy to implement.

**Cons:**
- Measures from runner process `fork/exec`, not from kernel entry.
- Includes QEMU startup (JIT compilation, device model init) and BIOS POST — both are
  significant (~200–500 ms typically).
- Does not measure within-kernel timing at all.

**Verdict:** A useful envelope measurement, but cannot substitute for in-kernel
timestamps. Best used in combination with RDTSC rather than alone.

---

## Decision

**Use RDTSC for in-kernel timing, printed to serial.**

1. Read `rdtsc()` at the top of `kernel_main` as the boot baseline (`T0`).
2. Read `rdtsc()` again at each significant milestone (serial init, framebuffer init,
   final `hlt` loop entry).
3. Print each delta (in cycles, and in nanoseconds at an assumed 1 GHz TSC) via
   `serial_println!`.
4. Record notable snapshots in `docs/boot-time-log.md`.

This gives cycle-accurate, reproducible, zero-dependency timing that works from the
very first line of `kernel_main` with the infrastructure already in place.

The runner's wall-clock time (option 4) will be added later as a secondary envelope
measurement once there is a reliable sentinel to parse from serial output.

---

## Consequences

- **Positive:** Sub-nanosecond cycle resolution; works immediately in QEMU with no
  new dependencies; serial output already exists; trivial to add new checkpoints.
- **Positive:** Foundation for a future `time` module once real-hardware calibration
  (PIT or HPET) is needed.
- **Negative:** TSC frequency is assumed (1 GHz) rather than measured. Accurate in
  QEMU with default CPU; will need PIT calibration before running on real hardware.
- **Risk:** TSC frequency assumption becomes wrong if the runner switches to `-cpu host`
  or a non-default CPU model. Mitigated by documenting the assumption and flagging it
  for the real-hardware milestone.

## Notes on process

Survey and decision produced in discussion with Claude (claude-sonnet-4-6).
The frequency assumption and scope boundaries were reviewed against current QEMU
defaults and the existing runner configuration. Final choice rests with the project
owner.
