# ADR-005: APIC over 8259 PIC

**Date:** 2026-05-21

**Status:** Accepted

**Issue:** #26

**Milestone:** v0.5

## Context

Through v0.4, all hardware interrupts were routed through the Intel 8259 Programmable
Interrupt Controller (PIC). The PIC served its purpose for basic keyboard input, but
has three hard limits that block further progress:

1. **No high-resolution timer.** The PIC's only timer source is PIT channel 0 (IRQ0),
   which fires at a fixed 18.2 Hz without reprogramming and cannot be per-CPU. A kernel
   timer subsystem needs sub-millisecond, per-core precision.
2. **No path to SMP.** The 8259 is a single, shared interrupt controller. On a
   multi-core system, all IRQs land on one CPU unless additional hardware (the APIC)
   routes them. Any future scheduling or AP bringup work requires the APIC to already
   be in place.
3. **Legacy vector space.** The PIC's default vectors (0x08–0x0F, 0x70–0x77) collide
   with CPU exception vectors. Remapping resolves the conflict but the PIC remains a
   single serialisation point.

The v0.5 milestone adds a kernel timer (`timer.rs`) and a `uptime` shell command.
Both require a reliable, calibratable interrupt source — motivating the APIC migration.

---

## Why now, not later

Migrating from PIC to APIC after a scheduler or SMP subsystem exists is significantly
harder:

- A scheduler assigns IRQ affinity and relies on per-CPU LAPIC state. Retrofitting
  LAPIC into an already-running scheduler requires coordinating across all CPUs during
  the switch.
- The LAPIC timer is the natural sleep/tick source for a scheduler. If the scheduler
  is built first, it either ties itself to the PIC timer (which must then be replaced)
  or delays its own implementation waiting for the APIC.
- The I/O APIC redirection table is the right place to express IRQ → CPU affinity.
  Building that table from scratch during a scheduler milestone adds scope that does
  not belong there.

Migrating at v0.5, while the kernel is still single-core and interrupt-light, keeps
each milestone's scope well-defined.

---

## ACPI dependency

Finding the LAPIC and I/O APIC base addresses requires parsing the ACPI MADT
(Multiple APIC Description Table). Two approaches exist:

### 1. Memory scan for RSDP

Scan the BIOS data area (0x000E0000–0x000FFFFF) for the "RSD PTR " signature.

**Pros:** Works without any bootloader cooperation.

**Cons:** Requires scanning physical memory with the `physical_memory_offset` mapping
active, and is fragile — the signature can appear spuriously and must be
checksum-verified. More code, more failure modes.

### 2. RSDP address from `bootloader_api` (chosen)

`bootloader_api::info::BootInfo` provides `rsdp_addr: Optional<u64>` — the
physical address of the RSDP, verified by the bootloader.

**Pros:** Single trusted source, no scanning, no risk of false positives. The
bootloader has already located and validated the RSDP before passing control.

**Cons:** Ties ACPI initialisation to the bootloader interface. If the bootloader
does not populate `rsdp_addr` (e.g., a custom bootloader), `acpi::init` is skipped
and `apic::init` panics.

**Verdict:** Use `rsdp_addr` from `BootInfo`. The dependency on `bootloader_api`
already exists throughout the kernel; adding one field access is not a meaningful
coupling increase.

---

## Decision

**Migrate to the APIC subsystem at v0.5, using ACPI for hardware discovery.**

### Implementation summary

| Module | Responsibility |
|--------|---------------|
| `acpi.rs` | Walk RSDP → RSDT/XSDT → MADT; extract `ApicInfo { lapic_addr, ioapic_addr, ioapic_gsi_base }` |
| `apic.rs` | Mask 8259 PIC, disconnect via IMCR, enable LAPIC (SVR + TPR), program I/O APIC redirection for keyboard IRQ1 → vector 0x21 → BSP LAPIC |
| `timer.rs` | Calibrate LAPIC timer against PIT channel 2 (hardware-fixed 1.193182 MHz); run periodic 1 ms tick at vector 0x20 |
| `time.rs` | Calibrate TSC frequency against PIT channel 2; used for accurate boot timing measurements |
| `idt.rs` | Register handlers at vector 0x20 (timer), 0x21 (keyboard), 0xFF (spurious) |

### LAPIC timer calibration

The LAPIC timer base frequency is not standardised — it varies by CPU and chipset.
Calibration is mandatory. The reference clock is PIT channel 2:

- PIT runs at exactly 1.193182 MHz (crystal-derived, host-speed-independent).
- A 10 ms one-shot window is timed by polling bit 5 of port 0x61.
- LAPIC ticks counted during that window ÷ 10 = ticks/ms.
- The periodic timer is then programmed with that count as its initial value.

The same PIT-based method calibrates the TSC frequency in `time.rs`. The earlier
RDTSC-only approach assumed a fixed 1 GHz TSC; on the development host (3.4 GHz)
this caused all boot timing measurements to read ~3× too low and the LAPIC timer
to fire ~3× too fast.

### 8259 PIC disposition

The 8259 is not simply ignored — it is actively neutralised:

1. `pic::remap()` moves PIC vectors to 0x20–0x2F (prevents spurious PIC interrupts
   from landing on CPU exception vectors 0x00–0x1F).
2. All PIC IRQ lines are masked (0xFF to both data ports).
3. The IMCR (port 0x22/0x23, register 0x70) is written to disconnect the 8259 from
   the BSP INTR pin (MP-spec symmetric I/O mode).

After step 3, the 8259 is electrically disconnected from interrupt delivery. It
remains powered but cannot raise interrupts.

---

## Consequences

- **Positive:** LAPIC timer provides sub-millisecond, per-CPU interrupt capability —
  a prerequisite for any preemptive scheduler.
- **Positive:** I/O APIC redirection table is in place; adding new IRQ routes requires
  only a register write, not PIC remapping.
- **Positive:** PIT-based calibration makes both timer tick and boot timing correct
  on any x86 host regardless of CPU clock speed.
- **Negative:** ACPI parsing adds a mandatory boot step; if `rsdp_addr` is absent,
  the kernel panics at `apic::init`.
- **Negative:** Additional 20 ms boot overhead from two PIT calibration windows
  (TSC + LAPIC timer).

## Known limitations

1. **BSP only.** Only the Bootstrap Processor's LAPIC is initialised. Application
   Processors (APs) are not started; their LAPICs are not configured.
2. **No AP IPI infrastructure.** Inter-Processor Interrupts are not implemented.
   The LAPIC ICR (Interrupt Command Register) is unused.
3. **Single redirection entry.** Only IRQ1 (keyboard) is programmed into the I/O APIC
   redirection table. Other ISA IRQs remain unrouted.
4. **No MSI support.** PCI Message Signalled Interrupts require per-device I/O APIC
   or LAPIC configuration not yet implemented.

## Revisit before

- **SMP / multi-core milestone:** AP bringup requires sending INIT–SIPI IPIs via the
  LAPIC ICR and initialising each AP's LAPIC independently.
- **PCI driver work:** MSI/MSI-X routing will require extending the I/O APIC
  redirection table and possibly adding a second I/O APIC if the system has one.
- **Real hardware testing:** IMCR presence is not guaranteed on all platforms. Add a
  capability check if boot failures are observed on non-QEMU targets.

## Notes on process

Design and rationale produced in discussion with Claude (claude-sonnet-4-6).
Implementation verified on QEMU 8.2.2 (BIOS mode, SeaBIOS 1.16.3).
Final decisions rest with the project owner.
