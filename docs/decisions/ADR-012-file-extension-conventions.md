# ADR-012: File Extension Conventions (revised)

**Date:** 2026-05-21

**Status:** Accepted — supersedes ADR-003

## Context

ADR-003 established aesthetic, project-specific extensions (`.sol`, `.kami`, `.amx`,
`.torii`) to reinforce AmaterasuOS's identity. That decision was made at v0.1, before
the project had a stated compatibility policy or a clear picture of the binary formats
the kernel would use.

Two things have changed:

1. **ADR-007 commits AmaterasuOS to a POSIX practical subset.** If shell commands
   behave like their POSIX counterparts and argument conventions match Linux/macOS,
   the file system conventions used by those commands should follow the same principle.
   A developer who already knows Unix should be able to read an AmaterasuOS file tree
   without a glossary.

2. **The toolchain produces ELF, not a custom format.** `rustc` and LLVM target
   `x86_64-unknown-none` and emit ELF binaries and objects. An extension that implies
   a different binary format (`.sol` reads as a custom format; `.kami` has no
   established meaning) is inaccurate. Extensions should describe what a file actually
   is, not reinforce project branding.

The additional insight from reviewing Windows NT architecture: `.dll` works on Windows
because it matches the PE binary format the linker produces. Adopting `.dll` on an
ELF-based OS — or any custom extension on an ELF OS — is a false label. The lesson is
that extension conventions are meaningful precisely because they are accurate, not
because they are unique.

Identity is expressed through what AmaterasuOS does, not through what its `.sol`
libraries are called.

---

## Decision

**Adopt standard, format-accurate extensions. Drop all aesthetic-only extensions
from ADR-003.**

| Role | Extension | Rationale |
|------|-----------|-----------|
| Shared library | `.so` | ELF Shared Object — format-accurate; what the linker produces |
| Kernel module / driver | `.ko` | Kernel Object — established Linux/ELF convention; describes privilege level and load target |
| Executable | *(none)* | POSIX convention — `cat`, `ls`, `sh` have no extension; the executable bit carries the signal |
| OS text / data files | *(none)* | Path provides context; `/sys/help/cat` is self-describing without a suffix |
| Structured config | `.conf` | Simple key=value config files, if needed |
| Structured data | `.toml` | Already used by Cargo; established for structured configuration |

### Notes on each role

**`.so`** — ELF shared objects on Linux and most POSIX systems. When AmaterasuOS has
a dynamic linker, the objects it loads will be ELF. No other extension is accurate.

**`.ko`** — The Linux convention for loadable kernel modules, adopted broadly enough
to be understood immediately by any kernel developer. Accurately describes the file's
target (kernel address space, ring 0 loader). Supersedes `.kami`.

**No extension for executables** — Every POSIX-standard utility (`cat`, `grep`, `ls`)
has no extension. The ELF magic bytes and executable bit convey the file type to the
kernel; the name conveys it to the user. An extension adds nothing. Supersedes `.amx`;
also avoids the `.amx` / AMX instruction-set name collision noted in ADR-003.

**No extension for OS text files** — The path already identifies the file. `/sys/help`
is the help index. `/sys/help/cat` is the `cat` help page. `/sys/welcome` is the
welcome screen. Appending `.torii` (or `.txt`) adds noise without adding information.
Supersedes `.torii` for all runtime text files.

---

## Migration: existing `.torii` files

ADR-003 was implemented for OS text files only — `.sol`, `.kami`, and `.amx` have no
runtime artifacts yet. The migration is therefore limited to `.torii` files in the
initrd and the kernel code that references them.

**Files to rename** (drop `.torii` suffix):

```
initrd/sys/help.torii          → initrd/sys/help
initrd/sys/welcome.torii       → initrd/sys/welcome
initrd/sys/help/cat.torii      → initrd/sys/help/cat
initrd/sys/help/cd.torii       → initrd/sys/help/cd
initrd/sys/help/clear.torii    → initrd/sys/help/clear
initrd/sys/help/cpu.torii      → initrd/sys/help/cpu
initrd/sys/help/heap.torii     → initrd/sys/help/heap
initrd/sys/help/help.torii     → initrd/sys/help/help
initrd/sys/help/ls.torii       → initrd/sys/help/ls
initrd/sys/help/pwd.torii      → initrd/sys/help/pwd
initrd/sys/help/reboot.torii   → initrd/sys/help/reboot
initrd/sys/help/shutdown.torii → initrd/sys/help/shutdown
initrd/sys/help/stat.torii     → initrd/sys/help/stat
initrd/sys/help/tab.torii      → initrd/sys/help/tab
initrd/sys/help/uptime.torii   → initrd/sys/help/uptime
```

**Kernel source changes required** (3 locations):

- `kernel/src/main.rs:84` — `"/sys/welcome.torii"` → `"/sys/welcome"`
- `kernel/src/shell.rs:403` — `"/sys/help.torii"` → `"/sys/help"`
- `kernel/src/shell.rs:404` — `"/sys/help/{}.torii"` → `"/sys/help/{}"`

This migration should be a single atomic commit on a dedicated branch so the rename
and the source update land together and the build never has a broken intermediate state.

---

## What this decision does NOT cover

- **Binary format.** This ADR does not specify ELF vs. a custom format; it says
  extensions must be accurate to whichever format is chosen. ELF is the current
  de-facto choice given the toolchain, but a future ADR should make this explicit
  when a binary loader is designed.
- **MIME types or file type databases.** No `file(1)` equivalent exists yet. When
  one is added, it should use magic bytes, not extensions.
- **Source file extensions.** `.rs`, `.toml`, `.md` — host tooling conventions
  are not in scope.

---

## Consequences

- **Positive:** Any developer who knows Unix can read an AmaterasuOS file tree.
  Extensions transfer existing knowledge rather than requiring a glossary.
- **Positive:** Extensions are format-accurate. `.so` means ELF shared object.
  `.ko` means ELF kernel module. No-extension executables mean POSIX binary.
- **Positive:** Eliminates the `.amx` / AMX ISA name collision flagged in ADR-003.
- **Negative:** The project-identity aesthetic from ADR-003 is abandoned. This is
  a deliberate tradeoff — standardization and readability outweigh branding at
  this stage of the project.
- **Negative:** 15 files and 3 source locations must be updated. Work is bounded
  and mechanical; tracked separately as a v0.7.5 task.

## Revisit before

- **Binary loader milestone:** Confirm `.so` and `.ko` match the actual ELF object
  types the loader will accept. If a custom format is chosen at that point, this ADR
  must be updated to match.

## Notes on process

Decision reached in discussion with Claude (claude-sonnet-4-6) after reviewing
Windows NT architectural principles (specifically why `.dll` works — it matches the
PE format the linker produces) and recognizing that ADR-003's aesthetic extensions
were inconsistent with ADR-007's POSIX compatibility commitment.
