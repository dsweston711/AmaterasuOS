# ADR-008: Hardware Abstraction Layer

**Date:** 2026-05-21

**Status:** Accepted

**Milestone:** Intent — no implementation milestone assigned yet

## Context

As AmaterasuOS gains new hardware interactions — PS/2 keyboard, ACPI power control,
PIC, timers, framebuffer, future storage controllers — a recurring question arises:
where does hardware-specific code live, and who is allowed to call it?

Currently `kernel/src/pic.rs` acts as an embryonic HAL: port I/O helpers (`outb`,
`outw`, `inb`, `io_wait`), PIC initialization, and keyboard drain are all in one
place rather than scattered across command handlers or subsystem code. This pattern
is already correct. This ADR names and commits to it so that future contributors
know where to put new hardware code and how it is expected to be structured.

The alternative — letting hardware interactions live wherever is convenient — has
been observed in other bare-metal projects to produce fragile, untestable code
where port addresses and register layouts are duplicated across files, and where
changing one hardware assumption requires searching the entire codebase.

---

## Decision

**All hardware-specific code lives in dedicated HAL modules. Higher-level code
never performs port I/O, MMIO, or hardware register access directly.**

Specifically:

1. **HAL modules are the only code that may use `in`/`out` instructions or
   MMIO-mapped register addresses.** Shell command handlers, VFS code, and
   subsystem code call HAL functions — they do not issue raw `outb`/`inb` or
   dereference raw hardware addresses.

2. **Each hardware subsystem gets its own module.** Current and anticipated modules:

   | Module | Hardware responsibility |
   |--------|------------------------|
   | `pic.rs` | 8259 PIC, PS/2 controller, port I/O primitives |
   | `timer.rs` | PIT/HPET, uptime tracking |
   | `framebuffer.rs` | Linear framebuffer, pixel writes |
   | `acpi.rs` *(future)* | ACPI table parsing, power management |
   | `storage.rs` *(future)* | ATA/AHCI, NVMe |

3. **Port addresses and MMIO bases are constants inside HAL modules**, never
   hard-coded at call sites. If an address changes (e.g. when real ACPI table
   parsing replaces the QEMU fixed address `0x604`), only the HAL module changes.

4. **HAL modules expose safe or clearly-unsafe interfaces.** Functions that truly
   cannot be made safe (`outb`, direct MMIO writes) are `pub unsafe`. Functions
   that wrap those with a complete safety contract are `pub fn` without `unsafe`,
   so call sites do not need `unsafe` blocks for routine hardware use.

---

## Current state and gap

`pic.rs` already satisfies this ADR for the hardware it covers. The ACPI shutdown
port (`0x604`) is currently hard-coded in `cmd_shutdown` in `shell.rs` via
`crate::pic::outw(0x604, 0x2000)` — the address is passed from the call site rather
than being a named constant in `pic.rs`. This is a minor violation of rule 3 above
and should be corrected when `shell.rs` is next touched: move the constant and the
semantics into `pic.rs` as `pub unsafe fn acpi_shutdown()`.

---

## Consequences

- **Positive:** Hardware assumptions are local. Changing port addresses, switching
  from PIC to APIC, or adding real ACPI parsing requires modifying one file.
- **Positive:** Shell and VFS code reads clearly without raw hardware magic numbers.
- **Positive:** HAL modules are the natural unit for hardware bring-up testing.
- **Negative:** An extra function call for every hardware interaction. At bare-metal
  speeds in ring 0, this cost is negligible and accepted.

## Revisit before

- **APIC milestone:** `pic.rs` will need to be split or supplemented when the 8259
  PIC is replaced by the APIC. At that point reconsider the module split.
- **Real hardware boot:** ACPI power management port must come from FADT parsing,
  not a fixed QEMU address. This is the primary known gap in the current HAL.

## Notes on process

Decision framed in discussion with Claude (claude-sonnet-4-6) while reviewing which
Windows NT architectural principles apply to AmaterasuOS. The HAL was identified as
the most immediately actionable because the pattern already exists and just needs
a stated rule.
