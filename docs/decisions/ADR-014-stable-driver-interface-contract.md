# ADR-014: Stable Driver Interface Contract

**Date:** 2026-05-21

**Status:** Accepted

**Milestone:** Intent — driver interface machinery arrives with first real device driver (storage, USB, or NIC)

## Context

AmaterasuOS's architectural decisions so far — HAL (ADR-008), layer separation (ADR-009), unified object model (ADR-010), subsystem personalities (ADR-011), IRQL discipline (ADR-013) — each solve a local problem. This ADR names the single goal they all serve:

**A hardware vendor should be able to write a driver for AmaterasuOS once, and that driver should continue to work across OS updates without modification.**

This is the Windows philosophy, codified in WDM (Windows Driver Model) and later KMDF. It is the *opposite* of the Linux position, which explicitly does not guarantee a stable in-kernel ABI for out-of-tree drivers. Linux's position is defensible (it allows aggressive internal refactoring), but it means every kernel update can silently break third-party hardware support. For an OS that wants broad hardware compatibility without requiring vendors to track internal changes, stability is the correct tradeoff.

The mechanism that makes the Windows promise *keepable* is the **miniport model**: Microsoft owns and maintains the complex port driver (which embeds all OS integration logic); the vendor writes only a thin **miniport** that knows how to talk to their hardware registers. When the OS evolves, Microsoft updates the port driver. The miniport rarely changes.

---

## Decision

**AmaterasuOS adopts the stable driver interface contract as a first-class architectural principle. The OS owns the interface; drivers own the hardware.**

### The contract

A driver for AmaterasuOS:

1. **Implements a declared capability interface** — a fixed set of function pointers (analogous to a vtable) that the OS calls at defined points in the device lifecycle. The OS promises these entry points will not change without a major version bump.

2. **Registers with the driver manager** — declares what hardware it handles (device class, vendor ID, device ID, or bus type) and what IRQL level its ISR runs at. The OS matches it to devices; the driver does not enumerate hardware itself.

3. **Never calls hardware directly outside its own ISR and miniport functions** — all DMA, interrupt acknowledgment, and bus transactions go through HAL APIs (ADR-008), not raw port I/O.

4. **Obeys IRQL discipline** (ADR-013) — the contract specifies at which level each entry point is called. A driver that violates this is incorrect regardless of whether it currently works.

5. **Returns errors; never panics** — a driver fault must be isolatable. The OS handles the fault; it does not propagate up to the shell or crash the system for a single bad device.

### The interface structure (intent, not final API)

```
struct DriverEntry {
    // Identity
    name:     &'static str,
    matches:  fn(device: &DeviceInfo) -> bool,  // called at PASSIVE_LEVEL

    // Lifecycle — all called at PASSIVE_LEVEL
    probe:    fn(dev: &mut Device) -> Result<(), DriverError>,
    start:    fn(dev: &mut Device) -> Result<(), DriverError>,
    stop:     fn(dev: &mut Device),
    remove:   fn(dev: &mut Device),

    // Interrupt service — called at DEVICE_LEVEL
    isr:      Option<fn(dev: &mut Device) -> IrqResult>,

    // Deferred work — called at DISPATCH_LEVEL
    dpc:      Option<fn(dev: &mut Device)>,
}
```

The OS calls these entry points. The driver implements them. The driver never needs to know which interrupt controller is in use, which DMA engine is available, or how the I/O manager routes requests — all of that is the OS's concern.

### What the OS promises

- The `DriverEntry` struct layout does not change within a major version.
- The HAL APIs a driver may call (port I/O, DMA mapping, interrupt registration) are stable within a major version.
- IRQL levels assigned to each entry point do not change.
- A driver compiled against AmaterasuOS v1.x will load on any AmaterasuOS v1.y without recompilation.

### How this relates to existing ADRs

| ADR | Role under this contract |
|-----|--------------------------|
| ADR-008 (HAL) | Provides the stable hardware APIs drivers call instead of raw port I/O |
| ADR-009 (Layer separation) | Ensures drivers live at Layer 1; they cannot reach into Executive or shell |
| ADR-010 (Unified Object Model) | Device handles are objects with a stable handle type |
| ADR-011 (Subsystem Personalities) | Drivers are not personalities; they are HAL-layer modules |
| ADR-013 (IRQL) | Defines the interrupt level contract each entry point runs at |

ADR-014 is the roof. ADR-008 through ADR-013 are the load-bearing walls beneath it.

---

## Current state

No driver manager or `DriverEntry` infrastructure exists yet. The keyboard and timer are currently hard-wired in `keyboard.rs` and `timer.rs` — they are embryonic drivers that do not yet conform to this interface.

**Migration path:**
1. Define the `DriverEntry` struct and a static driver registry in `kernel/src/driver.rs`
2. Refactor `keyboard.rs` to register as the first conforming driver
3. Add a `DeviceInfo` enumeration stub (keyboard, timer, future storage/NIC)
4. Wire probe/start/stop/isr/dpc to the existing keyboard and timer code

This work belongs in v1.0 milestone, after the process model lands and Layer 1 boundaries are clean.

---

## Consequences

- **Positive:** Hardware vendors have a single, stable target. The OS can evolve internally without breaking drivers.
- **Positive:** The driver lifecycle (probe → start → isr → dpc → stop → remove) makes driver state explicit and auditable.
- **Positive:** IRQL + the miniport split means a bad driver cannot deadlock the kernel by doing slow work in an ISR.
- **Positive:** The `matches` function enables automatic driver binding — plug in a device, the OS finds the right driver without shell intervention.
- **Negative:** The port driver / driver manager infrastructure is non-trivial to build correctly. Until it exists, current drivers are informal and do not carry these guarantees.
- **Negative:** Committing to interface stability constrains future OS refactoring — any change to `DriverEntry` requires a major version bump and a migration guide.

## Revisit before

Merging the first out-of-tree driver, or tagging v1.0.
