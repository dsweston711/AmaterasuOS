# ADR-003: File Extension Conventions

**Date:** 2026-05-20

**Status:** Accepted

**Issue:** #14

**Milestone:** v0.1 (naming locked now; `.kami` loader deferred to post-v0.6)

## Context

Standard Unix/Linux file extensions (`.so`, `.ko`, `.elf`) carry connotations of
Linux/POSIX conventions that don't reflect AmaterasuOS's identity. The OS is themed
around the Japanese sun goddess Amaterasu; file extensions are an early, low-cost
opportunity to reinforce that narrative consistently across documentation, tooling,
and source comments.

Establishing conventions now prevents terminology drift: once loaders, linkers, and
docs reference extension names, changing them becomes a broad refactor.

---

## Decision

Adopt the following extension scheme for all AmaterasuOS file types:

| Role               | Extension | Rationale                                                        |
|--------------------|-----------|------------------------------------------------------------------|
| Shared library     | `.sol`    | *Sol* — Sun in Latin/Spanish; shared source of light            |
| Kernel module/driver | `.kami` | 神 (God) — elevated-privilege code that interfaces with the kernel |
| Executable         | `.amx`    | **Am**aterasuOS e**X**ecutable — short and unique               |
| Config / manifest  | `.torii`  | 鳥居 — the gate between userspace and the OS                    |

These extensions replace `.so`, `.ko`, `.elf`/none, and `.conf`/`.toml` respectively
in AmaterasuOS-specific contexts. Standard host tooling (Rust, QEMU, GDB) continues
to use its own extensions internally.

---

## Consequences

- **Positive:** Consistent, memorable naming that reinforces the OS identity across
  docs, tooling, and source comments from this point forward.
- **Positive:** `.amx` and `.torii` can be adopted immediately in documentation and
  any future userspace tooling with no loader infrastructure needed.
- **Deferred:** `.kami` and `.sol` are the load-bearing extensions — they require
  a module loader and dynamic linker respectively before they are meaningful at
  runtime. This decision is not binding on the loader implementation until the
  `.kami` module loading milestone (post-v0.6).
- **Risk:** `.amx` conflicts with the AMX instruction-set extension name on x86.
  In context (file suffix vs. ISA feature) the collision is unlikely to cause
  confusion, but tooling that parses both may need disambiguation.

## Revisit Before

`.kami` module loader milestone (post-v0.6) — confirm extension is still appropriate
once the loader ABI is defined.

## Notes on process

Extension scheme proposed by project owner in issue #14. Rationale and consequences
reviewed with Claude (claude-sonnet-4-6).
