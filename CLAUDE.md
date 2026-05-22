# AmaterasuOS — Claude Code Guide

## Project overview

Bare-metal Rust kernel targeting x86_64. No std, no OS services. All code runs
in ring 0 as a single flat kernel. The primary user-facing component is an
interactive shell backed by a ramfs VFS.

**Build goal:** power button to usable shell in under 3 seconds on real
x86_64 UEFI hardware.

---

## Versioning scheme

The kernel reports a **CalVer build date** (`YYYY.MM.DD`) via `uname -r` and `stat /etc/version`.
Update `initrd/etc/version` when a milestone merges and a new build is cut.
The date is for human diagnostics only — no code should ever branch on it.

GitHub milestones use **v0.x / v1.x labels** as sprint names (project management only,
not the version the kernel reports):
- `v0.x` — pre-real-hardware era
- `v1.0` — first verified boot under 3 seconds on real x86_64 UEFI hardware
- `v1.x+` — post-hardware milestones

No sub-milestones (no `v0.9.5`, `v0.9.1`, etc.) — patch work folds into the active milestone.
Every issue belongs to exactly one milestone before work begins.

**Compatibility is guaranteed by stable interfaces** (ADR-014 DriverEntry contract, VFS VNode
trait), not by version-number checks. Drivers and software probe capabilities — they never
inspect the build date.

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

1. **Handler** — `fn cmd_<name>(&mut self, arg: Option<String>)` added to `Shell`
2. **Registry entry** — a `Cmd { name, usage, run }` line added to the `COMMANDS`
   static in `kernel/src/shell.rs`; `usage` is the one-line synopsis shown by `help`
3. **Help file** — `initrd/sys/help/<name>` following the style of existing help
   files (command synopsis, description, examples); no extension

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
| `initrd/sys/help/` | Per-command help files (no extension) |
| `initrd/sys/welcome` | Splash text printed on boot |
| `docs/decisions/` | Architecture Decision Records (ADRs) |
| `docs/boot-time-log.md` | Boot time measurements across milestones |
