# AmaterasuOS — Claude Code Guide

## Project overview

Bare-metal Rust kernel targeting x86_64. No std, no OS services. All code runs
in ring 0 as a single flat kernel. The primary user-facing component is an
interactive shell backed by a ramfs VFS.

**Build goal:** power button to usable shell in under 3 seconds on real
x86_64 UEFI hardware.

---

## Versioning scheme

Format: `v<major>.<minor>[.<patch>]`

| Level | Meaning | When to bump |
|-------|---------|--------------|
| **Major** | Architectural era | `0` = pre-alpha; bump to `1` when the 3-second boot goal is verified on real hardware |
| **Minor** | Capability milestone | A new subsystem or significant feature area lands (new driver, new shell subsystem, VFS writes, process model, etc.) |
| **Patch** | Polish within a minor | Bugfixes, small additions, no new subsystems (e.g. `v0.7.5` adds sorted ls inside the existing shell) |

Milestones in GitHub track these versions. Every issue belongs to exactly one
milestone before work begins.

---

## Build

Always build for the bare-metal target — **never the host**:

```
cargo build --package amaterasu_kernel --target x86_64-unknown-none
```

Run a build check before every commit. Building for the host target (`cargo build`
with no flags) fails with a duplicate `panic_impl` error — that is expected and
not a bug to fix.

To build and run in QEMU:

```
make run
```

---

## Workflow rules

### Branching
- One branch per issue: `feat/<issue-number>-<slug>` (e.g. `feat/43-tab-completion`)
- Merge via PR, delete branch after merge
- Use `closes #N` in the commit message that lands the feature to auto-close the issue

### Commits
- One logical chunk per commit — don't bundle unrelated changes in one commit
- Build must pass before every commit
- Commit message format: `<type>(<scope>): <description>`
  - e.g. `feat(shell): pwd command`, `fix(ramfs): strip leading ./`, `docs(shell): add help for cpu`
- Always append:
  ```
  Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
  ```

### Pull requests
- Open a PR for every branch; include `Closes #N` in the body
- Merge with `--merge` (no squash, no rebase) to preserve the per-chunk commit history
- Delete the branch after merge

---

## Adding a shell command

Every new shell command requires **all three**:

1. **Handler** — `fn cmd_<name>(&mut self, arg: Option<String>)` added to `Shell`,
   registered in the `COMMANDS` static in `kernel/src/shell.rs`
2. **Help file** — `initrd/sys/help/<name>.torii` following the style of existing
   help files (command synopsis, description, examples)
3. **Main listing** — a one-line entry added to `initrd/sys/help.torii`

---

## `no_std` constraints

- `alloc` crate is available; `std` is not
- No `HashMap`, `BTreeMap`, `Mutex` from std — use `spin::Mutex` and alloc collections
- Inline asm on x86_64: **`rbx` is reserved by LLVM** — wrap any asm that reads
  EBX output (e.g. CPUID leaf 0) with `xchg {tmp:e}, ebx` before and after
- Port I/O helpers live in `kernel/src/pic.rs`: `outb`, `outw`, `inb`, `io_wait`

---

## Key file layout

| Path | Purpose |
|------|---------|
| `kernel/src/shell.rs` | Interactive shell and all built-in commands |
| `kernel/src/vfs.rs` | Virtual filesystem trait and lookup |
| `kernel/src/ramfs.rs` | In-memory filesystem backed by the initrd |
| `kernel/src/pic.rs` | Port I/O helpers and PIC init |
| `kernel/src/cpu.rs` | CPUID helpers |
| `initrd/sys/help/` | Per-command help files (`.torii` extension) |
| `initrd/sys/help.torii` | Main help listing printed by `help` |
| `docs/decisions/` | Architecture Decision Records (ADRs) |
| `docs/boot-time-log.md` | Boot time measurements across milestones |
