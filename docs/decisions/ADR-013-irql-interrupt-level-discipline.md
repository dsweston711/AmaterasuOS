# ADR-013: IRQL-Style Interrupt Level Discipline

**Date:** 2026-05-21

**Status:** Accepted

**Milestone:** Intent — mechanically enforced as interrupt handling and driver infrastructure mature

## Context

AmaterasuOS already has interrupt handlers (keyboard ISR in `keyboard.rs`, timer ISR in `timer.rs`). These run asynchronously, preempting whatever code the CPU was executing. Right now the only rule is informal: "keep ISRs short." There is no named, enforced contract about what code may do at what level of interrupt context.

Without a formal model, the failure modes are subtle and hard to reproduce:
- An ISR that acquires a `spin::Mutex` already held by the preempted code → deadlock.
- A DPC-equivalent that calls a blocking operation → priority inversion.
- A driver that does significant work inside an ISR → missed interrupts, jitter.

Windows NT solved this with **IRQL (Interrupt Request Level)** — a CPU-local numeric priority. Code running at a given IRQL can only be preempted by code at a strictly higher IRQL. This makes interrupt safety a *compile-time architectural invariant*, not a per-developer discipline.

---

## Decision

**AmaterasuOS defines four named interrupt levels, modeled on Windows IRQL. Code at each level has a strict contract about what it may do.**

```
┌─────────────────────────────────────────────────────────────────┐
│  Level 3 — DEVICE_LEVEL  (hardware ISRs)                        │
│  • Runs: hardware interrupt fires                               │
│  • May do: read device register, acknowledge interrupt,         │
│            queue a deferred work item, set an AtomicBool        │
│  • May NOT do: acquire a spin lock held at lower levels,        │
│                allocate memory, call into VFS or shell          │
├─────────────────────────────────────────────────────────────────┤
│  Level 2 — DISPATCH_LEVEL  (deferred procedure calls / DPCs)    │
│  • Runs: softirq / tasklet / workqueue equivalent               │
│  • May do: acquire spin locks, do bulk data processing,         │
│            call into HAL (Layer 1) and Kernel Core (Layer 2)    │
│  • May NOT do: sleep, block, call into Executive or shell       │
├─────────────────────────────────────────────────────────────────┤
│  Level 1 — APC_LEVEL  (future: async procedure calls)          │
│  • Reserved for future process/thread model                     │
│  • May do: everything DISPATCH_LEVEL may do, plus page faults   │
├─────────────────────────────────────────────────────────────────┤
│  Level 0 — PASSIVE_LEVEL  (normal kernel / shell code)          │
│  • Runs: all shell commands, VFS, ramfs, most kernel code       │
│  • May do: anything — allocate, block (when blocking exists),   │
│            acquire any lock, call any layer                     │
└─────────────────────────────────────────────────────────────────┘
```

**Rules:**

1. **ISRs run at DEVICE_LEVEL.** An ISR must complete in microseconds. Its only job is to acknowledge the device and queue deferred work.
2. **Deferred work runs at DISPATCH_LEVEL.** Any non-trivial processing triggered by an interrupt — parsing a scancode, updating a counter, moving data — runs in a deferred work item, not the ISR.
3. **Spin locks are a DISPATCH_LEVEL construct.** Acquiring a `spin::Mutex` from an ISR (DEVICE_LEVEL) is forbidden unless the lock is *only ever acquired at DEVICE_LEVEL or above* — which effectively means it is a dedicated interrupt-only lock.
4. **PASSIVE_LEVEL code may be preempted by any level.** It must not assume it holds the CPU between two instructions unless it has raised the effective level (e.g., by holding a spin lock).
5. **Levels do not call upward.** DISPATCH_LEVEL code does not call into the shell or Executive. DEVICE_LEVEL code does not call into anything that wasn't designed for interrupt context.

---

## Current state

The current keyboard ISR (`keyboard.rs`) acquires `MODIFIERS` (a `spin::Mutex`) inside the ISR and calls `SHELL.lock().push_char()` — which acquires a second lock and does significant work (tab completion, history, dispatch). This violates Rule 3 (ISR holds too many locks) and Rule 2 (ISR does non-trivial work).

**Acceptable short-term:** the system is single-core and non-preemptive, so deadlock is currently impossible. The violation is architectural debt, not an immediate bug.

**Fix when adding a second interrupt source or multi-core:** split keyboard handling into:
- ISR: read scancode, write to a lock-free ring buffer, acknowledge interrupt
- DISPATCH_LEVEL worker: drain ring buffer, translate scancodes, call `push_char`

---

## Consequences

- **Positive:** Interrupt safety becomes a named, checkable property. Code review can ask "what level does this run at?" instead of reasoning from first principles each time.
- **Positive:** Enables multi-core safety — IRQL discipline is the foundation of Windows's SMP scalability.
- **Positive:** Makes the ISR → DPC split explicit, which is required for the stable driver interface (see ADR-014).
- **Negative:** The ISR → DPC split requires a deferred work queue (softirq equivalent) that does not yet exist. Until it does, the current single-threaded violation is tolerated.

## Revisit before

Adding a second CPU core, or adding any driver that shares an interrupt line with another device.
