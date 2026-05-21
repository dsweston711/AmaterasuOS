# ADR-009: Executive Layer Separation

**Date:** 2026-05-21

**Status:** Accepted

**Milestone:** Intent — enforced incrementally as new subsystems are added

## Context

AmaterasuOS is growing: `shell.rs`, `vfs.rs`, `ramfs.rs`, `pic.rs`, `cpu.rs`,
`framebuffer.rs`, `timer.rs`, `allocator.rs`. Each module has a clear purpose, but
there is no stated rule about which modules may call which others.

Without a dependency policy, the natural tendency as a codebase grows is for every
module to call every other module. This produces a flat mesh where changing any one
component requires understanding the entire system. It also makes it impossible to
test subsystems in isolation, and makes it structurally difficult to ever move code
out of ring 0 into user space.

Windows NT solved this with the Executive: distinct, named subsystems (Object Manager,
I/O Manager, Memory Manager, Process Manager, Security Reference Monitor) with a clear
hierarchy of who calls whom. They all run in kernel mode, but the interfaces between
them are clean and directional. This is what makes NT a *hybrid* kernel — microkernel
decomposition discipline, monolithic performance.

---

## Decision

**AmaterasuOS kernel code is organized into four layers. Each layer may only call
downward. No upward or lateral calls across layer boundaries.**

```
┌─────────────────────────────────────────┐
│  Layer 4 — Subsystems / Personalities   │  shell, future user-space servers
├─────────────────────────────────────────┤
│  Layer 3 — Executive Services           │  vfs, ramfs, future object manager,
│                                         │  I/O manager, process manager
├─────────────────────────────────────────┤
│  Layer 2 — Kernel Core                  │  allocator, scheduler (future),
│                                         │  memory manager (future)
├─────────────────────────────────────────┤
│  Layer 1 — HAL                          │  pic, framebuffer, timer, cpu,
│                                         │  future acpi, storage
└─────────────────────────────────────────┘
```

**Rules:**

1. **Layer N may call Layer N-1 and below.** It may not call Layer N+1 or above.
2. **No circular dependencies between modules at the same layer.** Peers at the
   same layer communicate through Layer 3 abstractions (e.g. VFS), not directly.
3. **The shell (Layer 4) does not reach into HAL or kernel core directly.** It
   calls Layer 3 services, which call downward. Exception: `pic::outb/outw` may be
   called from Layer 4 until Layer 3 HAL wrappers exist for every operation.
4. **Layer 3 modules do not call `shell.rs` or any Layer 4 code.** VFS does not
   print to the framebuffer; it returns errors. The shell decides how to display them.

---

## Current state

The current codebase broadly respects this already:
- `shell.rs` calls `vfs`, `ramfs`, `pic`, `cpu`, `timer`, `framebuffer` — Layer 4 calling downward. ✓
- `vfs.rs` and `ramfs.rs` call the allocator — Layer 3 calling Layer 2. ✓
- `pic.rs` calls nothing above itself. ✓

Known violations to fix over time:
- `shell.rs` directly calls `crate::pic::outw(0x604, 0x2000)` with a raw address —
  should go through a `pic::acpi_shutdown()` wrapper (see ADR-008).
- As the codebase grows, watch for Layer 3 code that begins calling `crate::print!`
  or touching the framebuffer. Those calls belong at Layer 4.

---

## Consequences

- **Positive:** Any module can be reasoned about by understanding only the layers
  below it. Refactoring a layer does not require touching higher layers.
- **Positive:** When a process model arrives, Layer 4 moves to user space. The
  boundary is already drawn; no restructuring required.
- **Positive:** Layer 3 and below can be tested (eventually) without a working shell.
- **Negative:** Some code that would be "simpler" to write with upward calls must
  instead return error values and let higher layers handle presentation. This is
  the correct tradeoff.

## Revisit before

- **Process model milestone:** When user-space processes exist, Layer 4 should move
  there. This ADR's layer diagram is the map for that migration.
- **Driver model milestone:** External drivers (storage, network) need a defined
  position in the layer stack. They are likely Layer 1 (HAL) or a new Layer 1.5
  between HAL and Executive.

## Notes on process

Decision framed in discussion with Claude (claude-sonnet-4-6) while reviewing which
Windows NT architectural principles apply to AmaterasuOS. Modeled on the NT Executive
layer decomposition, adapted to AMTOS's current module structure.
