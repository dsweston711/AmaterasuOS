# ADR-010: Unified Object Model

**Date:** 2026-05-21

**Status:** Accepted (intent)

**Milestone:** Intent — implementation begins when process model is introduced

## Context

As AmaterasuOS adds processes, devices, timers, and IPC primitives, a recurring
design question will emerge: how does a process refer to a kernel resource?

The naive answer is: differently for each resource type. File descriptors for files.
Process IDs for processes. Timer IDs for timers. Each with its own lookup table,
its own validation logic, its own access rules. This is how many OS kernels evolved
historically, and it produces fragmented security models, duplicated handle
management code, and subtle bugs where access checks are implemented inconsistently
across resource types.

Windows NT took a different approach: the Object Manager. Every kernel resource —
files, processes, threads, events, semaphores, registry keys, I/O completion ports —
is a typed kernel object. All objects are accessed through handles in a unified
handle table. All access checks go through a single security reference monitor path.
All objects have a reference count and a defined lifetime.

AmaterasuOS already has the seed of this: the VFS `vnode` trait. A `vnode` is a
typed object (file or directory) with a defined interface (kind, size, read, readdir).
It is reference-counted implicitly through Rust ownership. The question is whether to
extend this pattern to all kernel resources, or let each new resource type invent
its own handle scheme.

---

## Decision

**AmaterasuOS will extend the object model rather than invent per-resource handle
types. When new kernel resources are introduced (processes, devices, timers, IPC
channels), they will be represented as typed objects conforming to a common interface,
accessed through a unified handle table.**

Specifically:

1. **The `vnode` trait is the prototype.** Future object types should follow the same
   pattern: a Rust trait defining the object's interface, with concrete implementations
   for each resource type.

2. **A unified handle table will be introduced with the process model.** Each process
   has a table mapping integer handles to object references. All user-visible resource
   access goes through this table. The table is the only place where integer IDs are
   translated to object pointers.

3. **Access checks are centralized.** When a handle is resolved, a single function
   verifies the caller has the required permission for the operation. Access checking
   is not scattered across individual object implementations.

4. **Object lifetimes are managed by reference count.** An object is destroyed when
   no handles or kernel references to it remain. This is already Rust's natural model
   for heap-allocated values; it should be preserved rather than overridden with manual
   ref-counting.

5. **Do not invent per-resource tables.** When adding a new resource type, the instinct
   to "just add a `Vec<MyThing>` indexed by an integer ID" should be resisted. The
   unified handle table is the right place.

---

## Current state

The VFS layer satisfies the spirit of this ADR for file resources. The `vnode` trait
(Layer 3 Executive) is the correct abstraction level. `ramfs.rs` provides the concrete
implementation.

No handle table exists yet because there are no processes. This ADR is primarily a
commitment against diverging before the process model arrives — it documents the
intended direction so that interim resource types (if any are added before processes)
do not entrench an incompatible pattern.

---

## What this does NOT prescribe

- **A specific handle table data structure.** `Vec`, slab-allocated array, or B-tree
  are all acceptable; that is an implementation decision for the process model ADR.
- **Capability-based vs. ACL-based security.** The access check model is a future
  decision. This ADR only requires that access checks be centralized, not what they
  check.
- **A C-style `HANDLE` integer type.** Rust traits and references may serve as the
  internal handle representation in kernel space. The integer handle is the user-space
  interface; internally, typed references are fine.

---

## Consequences

- **Positive:** One security audit path covers all resource types. No category of
  resource can accidentally bypass access checks.
- **Positive:** Handle leak detection, per-process resource limits, and debugging
  are all natural features of a centralized table.
- **Positive:** Adding a new resource type does not require designing a new ID scheme.
- **Negative:** More upfront design required when introducing the first process.
  Writing the handle table and object trait before the first process type is extra
  work that pays off only as more resource types are added.

## Revisit before

- **Process model milestone:** This ADR's intent becomes a concrete implementation
  requirement. A companion ADR should specify the handle table structure, the object
  trait interface, and the access check protocol.
- **Device driver model:** Devices are kernel objects and must fit the unified model.
  If a driver model ADR is written before the process model, it should reference this
  ADR and defer handle table details accordingly.

## Notes on process

Decision framed in discussion with Claude (claude-sonnet-4-6) while reviewing which
Windows NT architectural principles apply to AmaterasuOS. The NT Object Manager was
the primary reference. The VFS vnode trait was identified as the existing seed of
this pattern.
