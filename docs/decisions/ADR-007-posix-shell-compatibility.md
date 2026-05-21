# ADR-007: POSIX Shell Compatibility Target

**Date:** 2026-05-21

**Status:** Accepted

**Milestone:** v0.8 (first milestone where new commands are written under this policy)

## Context

As the AmaterasuOS shell gains more commands, a design choice emerges that affects
every command written from here forward: should commands behave like their POSIX/Unix
counterparts, or should they follow whatever conventions happen to be simplest to
implement?

The question surfaced concretely when planning v0.8 commands (`echo`, `uname`,
`hostname`, `wc`, `head`, `tail`, `grep`). Each of these has well-established
behavior on Linux, macOS, and POSIX-compliant systems — including specific flags,
output formats, and error message conventions. If we ignore this, AmaterasuOS will
develop its own idiosyncratic shell that feels alien to anyone who has used a
terminal before.

There is also a secondary concern: the existing v0.7 commands (`ls`, `cat`, `stat`,
`cd`, `grep`) were written without an explicit compatibility target. Some of them
will need retrofitting once this decision is made.

---

## Options considered

### Option A: Custom conventions — implement whatever is simplest

Commands accept the arguments that are convenient to parse. Flags are invented
ad-hoc. Output format is whatever is easiest to print.

- **Pros:** Minimal flag-parsing complexity. Each command is self-contained.
- **Cons:** Every command is a surprise. Users cannot apply years of terminal muscle
  memory. `ls -l` does nothing or errors. `grep pattern file` might have the
  argument order wrong. Documentation must describe every detail from scratch.
  This debt compounds as the command count grows.

### Option B: Full POSIX compliance — implement the complete specification

Match the IEEE POSIX.1-2017 specification for every command exactly, including
rarely-used flags, precise error codes, locale handling, and edge-case output
formatting.

- **Pros:** Maximally familiar. Scripts written for Linux could run unmodified.
- **Cons:** Prohibitively complex at this stage. POSIX `ls` alone has 25+ flags.
  Full compliance requires locale support, file permissions, symbolic links,
  process exit codes, signal handling — none of which exist in the kernel yet.
  This is a multi-year project in its own right.

### Option C: Practical POSIX subset — match familiar behavior for common usage (chosen)

Target the behavior that accounts for the vast majority of real terminal usage:
correct argument order, the flags people actually type, and output that looks right
to a human reader. Explicitly defer flags that require missing subsystems (permissions,
symlinks, processes). Document what is and is not supported.

- **Pros:** Users can apply existing knowledge immediately. Flag conventions transfer.
  Output formats are predictable. Deferred flags are clearly marked, not silently
  broken. Implementation complexity is proportional to actual usage.
- **Cons:** Not a complete implementation. Some flags will be absent or stubbed.
  Scripts that rely on obscure flags or precise exit codes will not work without
  changes.

---

## Decision

**AmaterasuOS shell commands follow Option C: practical POSIX subset.**

Specifically:

1. **Argument order is POSIX-standard.** `grep pattern file`, `head -n 5 file`,
   `wc file` — argument positions match what users expect from Linux/macOS.

2. **Common flags are implemented.** Each command implements the flags that cover
   the large majority of real usage. The v0.8 commands and their in-scope flags:

   | Command | In-scope flags |
   |---------|---------------|
   | `echo`  | `-n` (suppress trailing newline) |
   | `uname` | `-a` (all), `-s` (kernel name), `-r` (release), `-m` (machine) |
   | `wc`    | `-l` (lines), `-c` (bytes), `-w` (words) |
   | `head`  | `-n <count>` |
   | `tail`  | `-n <count>` |
   | `grep`  | `-i` (case-insensitive), `-n` (line numbers), `-c` (count only) |
   | `ls`    | `-a` (show hidden), `-l` (long format) — deferred to when VFS has metadata |
   | `cat`   | `-n` (number lines) |

3. **Output format matches POSIX where feasible.** `wc` prints right-aligned columns.
   `grep` prints `filename:line` when multiple files are searched. `ls -l` shows
   permissions once the VFS has metadata.

4. **Error messages follow the `command: reason` convention.** e.g.
   `cat: /foo: not found`, not a custom format.

5. **Flags that require missing subsystems are explicitly unsupported**, not silently
   ignored. If a user passes an unrecognised flag, the command prints a usage message.
   This is preferable to silently accepting and ignoring a flag the user depended on.

6. **This policy applies retroactively to existing commands.** `cat`, `ls`, `stat`,
   `grep` will be updated to conform as part of the milestones where they are
   revisited. No big-bang retrofit; compliance is added incrementally as commands
   are touched.

---

## Flag parsing

The v0.7 commands parse arguments as a single `Option<String>`. This is insufficient
for commands with flags. Before or during v0.8, a minimal flag parser is needed that:

- Splits the argument string into flags and positional args
- Handles combined short flags (`-la` = `-l -a`)
- Handles flags with values (`-n 10`, `-n10`)

This does not need to be a full `getopt` implementation. A simple iterative parser
over the argument characters is sufficient for the flag set in scope.

The flag parser should live in `kernel/src/shell.rs` as a free function, available
to all command handlers.

---

## What this decision does NOT cover

- **Shell language compatibility.** No decision is made about `$VAR` expansion
  syntax, quoting rules, or redirection matching bash/sh behavior. Those are
  independent decisions taken when those features are designed.
- **Exit codes.** AmaterasuOS has no process model yet. When one exists, exit codes
  should follow POSIX convention (0 = success, non-zero = error), but that is a
  future ADR.
- **Signal behavior.** Out of scope until a process model exists.
- **Locale and encoding.** All text is treated as UTF-8 bytes. No locale-aware
  collation or character classification beyond ASCII is planned for v1.0.

---

## Consequences

- **Positive:** Users coming from Linux, macOS, or WSL can apply existing knowledge
  to AmaterasuOS's shell immediately.
- **Positive:** Documentation can reference standard man pages for command behavior
  rather than describing everything from scratch.
- **Positive:** The scope boundary (practical subset, not full compliance) keeps
  implementation tractable and honest about what is and is not supported.
- **Negative:** Existing v0.7 commands (`cat`, `ls`, etc.) do not yet fully conform.
  Retrofit work is needed and will happen incrementally.
- **Negative:** A flag parser must be added before most v0.8 commands can be written
  correctly. This is a dependency that did not previously exist.

## Revisit before

- **VFS write milestone:** `ls -l` and `stat` long-format output require file
  metadata (size, type, timestamps). Revisit flag coverage for these commands when
  the VFS gains metadata support.
- **Process model milestone:** Exit code conventions should be codified as a
  companion ADR when processes are introduced.

## Notes on process

Decision framed and written in discussion with Claude (claude-sonnet-4-6) after
recognising that the v0.8 command planning session was implicitly making this
choice on a per-command basis. Better to state it once explicitly than let it
drift. All scoping decisions are the project owner's.
