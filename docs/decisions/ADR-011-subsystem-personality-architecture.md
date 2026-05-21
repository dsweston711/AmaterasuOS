# ADR-011: Subsystem Personality Architecture

**Date:** 2026-05-21

**Status:** Accepted

**Milestone:** Partially applicable now; full implementation requires process model

## Context

Windows NT's most underappreciated design decision is that Win32 is not the kernel.
The NT kernel exposes raw NT system calls (`NtCreateFile`, `NtCreateProcess`, etc.).
Win32 is a *personality* — a subsystem server (`csrss.exe`) and a set of DLLs
(`kernel32.dll`, `user32.dll`) that translate Win32 calls into NT calls. In theory,
other personalities (OS/2, POSIX) can run on the same kernel. In practice, Win32
dominated, but the architectural boundary was real and has value: the kernel does not
know or care what API style the user-space program prefers.

AmaterasuOS faces the same design choice as it adds user-facing interfaces. Currently
the shell is the only interface. It calls kernel primitives directly and runs in ring 0
as part of the kernel binary. This is expedient but could become an architectural
trap: if shell concepts bleed into kernel code (Layer 3 or below starts depending on
shell behavior, or the shell's argument parsing is treated as the kernel's calling
convention), then moving the shell to user space later becomes a major restructuring.

---

## Decision

**The AmaterasuOS shell is a subsystem personality, not a kernel component. Kernel
layers (1–3 in ADR-009) must not depend on shell concepts. When a process model
exists, the shell will move to user space.**

Specifically:

1. **The kernel defines a raw system call interface.** It does not define a "shell
   command" interface. The shell interprets text input and translates it into system
   calls; the kernel has no knowledge of command names, argument strings, or shell
   flags.

2. **Kernel code (Layers 1–3) must not depend on `shell.rs`.** VFS does not call
   shell functions. The allocator does not call shell functions. Error presentation
   is the shell's job, not the kernel's. If a Layer 3 module needs to report an
   error, it returns a result type; the shell decides how to print it.

3. **The shell's argument parsing and flag conventions (see ADR-007) are shell
   concerns, not kernel concerns.** The kernel's system call interface uses typed
   parameters, not strings. When the shell parses `head -n 5 /etc/motd`, that
   parsing happens in the shell before any kernel call is made.

4. **Future user interfaces are also personalities.** A graphical shell, a network
   console, a scripting interpreter — each is a separate subsystem personality
   implemented on top of the same kernel system call interface. They do not share
   code with each other except through Layer 3 services.

5. **The shell migration plan:** Currently the shell runs in ring 0 for the
   practical reason that no process model exists. This is acceptable as a temporary
   state. When the process model lands, the shell is the first candidate for
   migration to ring 3. The layer boundaries in ADR-009 are designed so this
   migration requires no changes to Layers 1–3.

---

## Current state and guardrails

The current `shell.rs` structure is broadly correct:
- It calls `vfs::lookup()`, `pic::outb()`, `cpu::vendor()` — downward calls only. ✓
- `vfs.rs` and `ramfs.rs` contain no references to `shell.rs`. ✓
- Error display (`crate::println!("cat: not found: ...")`) happens in shell handlers,
  not in VFS functions. ✓

The main risk going forward is *feature creep into Layer 3*: as the shell gains more
capabilities, there will be temptation to push logic "closer to the data" by adding
shell-aware behavior to VFS or the allocator. This ADR is the explicit prohibition
against that.

---

## What "personality" means in practice

A personality provides:
- A human-readable or script-readable interface to kernel services
- Argument parsing, flag handling, output formatting
- Error message text
- Convenience abstractions (e.g., `PATH` lookup, shell expansion)

A personality does NOT provide:
- Security policy (that is the Object Manager / Security Reference Monitor)
- Resource allocation (that is the Memory Manager)
- I/O dispatch (that is the I/O Manager)

If code does any of the "does not provide" items, it belongs in Layer 3, not in
the shell.

---

## Consequences

- **Positive:** The kernel system call interface is stable independent of which
  shell or user interface is in use. Replacing the shell does not require touching
  kernel code.
- **Positive:** Shell migration to user space is a contained change, not an
  architectural restructuring.
- **Positive:** Multiple concurrent interfaces (serial console + graphical shell,
  for example) are architecturally natural — each is its own personality process.
- **Negative:** Some features that would be trivial to implement by coupling shell
  and kernel must instead be implemented via a clean system call. This is the
  intended cost of maintaining the boundary.

## Revisit before

- **Process model milestone:** Shell migration to ring 3 should be a first-class
  deliverable of the process model milestone, not deferred further.
- **Second user interface:** When any second user-facing interface is added (network
  console, GUI), this ADR should be reviewed to ensure both interfaces are implemented
  as personalities without privilege.

## Notes on process

Decision framed in discussion with Claude (claude-sonnet-4-6) while reviewing which
Windows NT architectural principles apply to AmaterasuOS. The NT Win32 subsystem
model was the primary reference. The immediate practical constraint (no process model
yet) is acknowledged; the ADR documents intent rather than requiring an immediate
restructuring.
