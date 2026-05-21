# ADR-004: Slab Allocator Design

**Date:** 2026-05-21

**Status:** Accepted

**Issue:** #21

**Milestone:** v0.4

## Context

With `Box`, `Vec`, and `String` needed throughout the kernel, a `#[global_allocator]`
is required. The 16 MB heap carved in `memory.rs` must be managed by something that:

1. Is correct in a single-core, interrupt-driven `no_std` environment
2. Supports `dealloc` (kernel data structures are created and destroyed at runtime)
3. Has predictable, bounded allocation latency (no GC pauses, no coalescing walks)
4. Is simple enough to audit and debug without a working debugger

---

## Options surveyed

### 1. Pure bump allocator

A bump pointer advances on each allocation; `dealloc` is a no-op.

**Pros:** Trivially correct, O(1) alloc, zero fragmentation initially.

**Cons:** Memory is never reclaimed. A kernel that creates and destroys data
structures (process descriptors, buffers, IPC messages) will exhaust the heap
quickly. Unsuitable as a general-purpose allocator.

**Verdict:** Retained as a *fallback* for allocations that exceed the largest slab
class, where the caller is expected to hold the allocation long-term (e.g. large
static buffers). Not used as the primary strategy.

### 2. Linked-list / boundary-tag allocator (dlmalloc style)

Maintains a free list of variable-size blocks with header/footer tags; coalesces
adjacent free blocks on dealloc.

**Pros:** Handles arbitrary sizes, low overhead for large allocations, mature design.

**Cons:** `alloc` and `dealloc` both walk the free list — O(n) worst case. Header
corruption is silent and hard to diagnose without tooling. Coalescing adds latency
spikes. Complexity is high for an early-stage kernel with no heap debugging tools.

**Verdict:** Ruled out. Too complex and too slow for the interrupt-path allocation
patterns expected in a kernel.

### 3. Slab allocator (chosen)

Fixed-size object pools, one per size class. Each pool pre-threads its objects into
a singly-linked freelist at init time. Alloc = pop from freelist (O(1)). Dealloc =
push back to freelist (O(1)).

**Pros:**
- O(1) alloc and dealloc with no traversal.
- No header/footer overhead per object — the freelist pointer is stored *in* the
  free object itself, so it costs nothing when the object is live.
- No fragmentation within a slab class.
- Each class is independently exhaustible — one hot path can't starve another.
- Trivially auditable: `free_count` tells you exactly how many objects are
  available in each class.

**Cons:**
- Internal fragmentation: a 9-byte allocation uses a 16-byte slot.
- Fixed number of objects per class — a slab can be exhausted.
- No support for arbitrary sizes without a fallback.

**Verdict:** Selected. The size ladder covers the allocation patterns expected in
this kernel; the bump fallback handles the remaining cases.

---

## Decision

**Use a slab allocator with a bump fallback.**

### Slab size ladder

| Class | Object size | Pool size | Bytes reserved |
|-------|-------------|-----------|----------------|
| 0     | 8 B         | 512       | 4 KB           |
| 1     | 16 B        | 512       | 8 KB           |
| 2     | 32 B        | 512       | 16 KB          |
| 3     | 64 B        | 512       | 32 KB          |
| 4     | 128 B       | 512       | 64 KB          |
| 5     | 256 B       | 512       | 128 KB         |
| —     | > 256 B     | bump      | ~15.75 MB      |

**Rationale for the ladder:**
- Powers of two double each step, minimising internal fragmentation (worst case
  ~50% waste within a class).
- 8 B is the minimum because the freelist pointer on x86_64 is 8 bytes.
- 256 B is the ceiling because kernel allocations above that size (ring buffers,
  page-sized structures) are expected to be long-lived; bump is appropriate.
- 512 objects per class is enough for early subsystems; the count can be raised
  before the class is added to the size ladder.

### Freelist implementation

When an object is free, its first 8 bytes store the address of the next free
object (or 0 for end-of-list). The list is built at `init()` time by iterating
the pool region. No per-object headers are needed when the object is live.

### Fallback strategy

Allocations whose size *or alignment* exceed 256 B are routed to the bump
allocator. `dealloc` on bump-allocated memory is a no-op — the bump pointer
does not retreat. This is acceptable for large, long-lived allocations (which
are the primary users of the fallback) but means the bump region is effectively
a one-way resource.

### Locking

All state is held behind a single `spin::Mutex`. This is safe on single-core
QEMU; the interrupt handler disables IRQs on entry (x86 clears IF), preventing
re-entrant allocation from keyboard interrupts.

---

## Consequences

- **Positive:** O(1) alloc and dealloc for all common kernel allocation sizes.
- **Positive:** Per-class `free_count` makes heap health trivially observable via
  the `heap` shell command.
- **Positive:** No allocator complexity to debug while higher-level kernel
  subsystems are being built.
- **Negative:** Internal fragmentation of up to ~50% within a slab class.
- **Negative:** Bump region is not reclaimed; sustained large allocations will
  exhaust it over time.
- **Negative:** Single global lock; will become a bottleneck if multi-core
  scheduling is added.

## Known limitations

1. **No compaction.** Objects are never moved; fragmentation accumulates.
2. **Bump does not free.** Large allocations are permanent for the lifetime of
   the kernel session.
3. **Fixed pool sizes.** Exhausting a slab class causes that class to silently
   fall through to the next larger class, then to bump. Exhausting bump panics.
4. **No guard pages.** Heap overflows are not detected; they corrupt adjacent
   allocations silently.

## Revisit before

Switch to a more sophisticated allocator (e.g. buddy + slab, or a lock-free
per-CPU design) when any of the following occur:

- Bump region exhaustion observed in the `heap` command under normal workloads
- Multi-core scheduling is introduced (single lock becomes a bottleneck)
- A slab class is routinely exhausted (observable via `heap` stats)

## Notes on process

Design and rationale produced in discussion with Claude (claude-sonnet-4-6).
Size ladder, pool counts, and fallback strategy reviewed against expected
early-kernel allocation patterns. Final decisions rest with the project owner.
