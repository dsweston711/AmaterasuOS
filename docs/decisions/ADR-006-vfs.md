# ADR-006: VFS Abstraction, VNode Trait Design, and ustar Ramdisk

**Date:** 2026-05-21

**Status:** Accepted

**Issue:** #32

**Milestone:** v0.6

## Context

The v0.6 milestone ("Storage Backbone") adds the first filesystem layer to AmaterasuOS.
Before this milestone, shell commands interacted directly with whatever data happened to
be in memory. Three problems motivated a proper abstraction:

1. **Shell/storage coupling.** Without indirection, each shell command must know the
   storage format. Adding a second backing store (persistent disk, tmpfs, procfs) would
   require editing every command.
2. **No boot-time file data.** The kernel needs read access to initial file content at
   boot — e.g., for a future init script or config. A ramdisk provides this without
   requiring any block driver.
3. **Extension point.** The OS-specific file types (`.kami` modules, `.amx`
   executables) require a stable path resolution interface before loaders can be written.

The scope for v0.6 is deliberately narrow: read-only access, a single mount point, and
a ramdisk-only backing store. Writable filesystems and persistent storage are
post-v0.6 work.

---

## VFS abstraction layer

### Options considered

**Option A: Global `match` on a storage-type enum**

A `static STORAGE: Mutex<StorageKind>` where `StorageKind` is an enum of all known
backends, each matched explicitly. Shell commands call into a top-level function that
matches and dispatches.

- **Pros:** No heap allocation, no dynamic dispatch, no `unsafe`.
- **Cons:** Every new backend requires editing the core dispatch match. Shell commands
  become coupled to the enum. Not extensible without touching the VFS module.

**Option B: `VNode` trait with dynamic dispatch (chosen)**

Define a `VNode: Send + Sync` trait that all filesystem nodes implement. The global
root is `Mutex<Option<Box<dyn VNode>>>`. Shell commands call `vfs::lookup()` and
receive a `Box<dyn VNode>`, with no knowledge of the backing type.

- **Pros:** New backends implement `VNode`; the VFS core and all shell commands are
  unchanged. Mirrors the VFS designs in Linux and BSD at an appropriately minimal
  scale.
- **Cons:** Heap allocation per `lookup()` call; vtable dispatch overhead. Both are
  acceptable — the kernel allocator is in place and filesystem calls are rare.

**Verdict:** Option B. The extensibility benefit justifies the allocation cost.

### The `VNode` interface

```
trait VNode: Send + Sync {
    fn kind(&self) -> NodeKind;          // File or Dir
    fn size(&self) -> usize;
    fn read(&self, buf: &mut [u8], offset: usize) -> usize;
    fn lookup(&self, name: &str) -> Option<Box<dyn VNode>>;
    fn readdir(&self) -> Vec<String>;    // default: empty
}
```

`lookup` returns an owned `Box<dyn VNode>` rather than a reference, avoiding lifetime
complexity from the `Mutex` guard. Each call either re-slices static data (files) or
wraps a raw pointer to the static tree (dirs).

### Single mount point

`static ROOT: Mutex<Option<Box<dyn VNode>>>` is the sole filesystem root. `vfs::mount`
is called once during `kernel_main`, immediately after `ramfs::init`. `vfs::lookup`
walks absolute paths by splitting on `/` and calling `lookup` at each component.

There is no mount table, no union mount, and no bind mount. A tree of named vnodes is
the whole model for v0.6.

---

## VNode trait vs. enum for ramdisk nodes

Even within a single backend, nodes could be modeled two ways:

### Internal enum (used during parsing)

`ramfs.rs` uses a private `enum Node { File(&'static [u8]), Dir(Vec<(String, Node)>) }`
during the ustar parse pass. This is fine internally: the tree is fully known at parse
time, no trait dispatch is needed, and it avoids boxing intermediate nodes before the
tree is fully built.

### Box<dyn VNode> (used after parsing)

Once parsing completes, `to_vnode()` converts the internal enum tree into a
`Box<dyn VNode>` tree and calls `vfs::mount()`. From that point, all access goes
through the trait interface. This means:

- `RamfsFile` holds `&'static [u8]` — zero-copy, sliced directly from the ramdisk
  memory region provided by the bootloader.
- `RamfsDir` holds `Vec<(String, Box<dyn VNode>)>` — the boxed child list.
- `lookup()` on a dir returns an `OwnedFile` (allocated copy of file bytes) for files,
  and a `DirRef` (thin `*const dyn VNode` wrapper) for directories. This is required
  because `Box<dyn VNode>` is not `Clone` — returning a new box requires either
  copying data or aliasing through a raw pointer. The `'static` lifetime of the tree
  (mounted once, never unmounted) makes the raw pointer safe.

---

## Ramdisk format: ustar vs. custom

### ustar (chosen)

POSIX ustar is the payload format produced by `tar -c --format=ustar`. Each entry is a
512-byte header (name, type, octal size) followed by the data, padded to 512-byte
alignment. The archive ends with two zero blocks.

**Why ustar for v0.6:**

- **Tooling exists.** `tar` builds the archive; `tar -t` inspects it. No new tooling
  needed.
- **Parser is tiny.** The entire `parse_ustar` function is ~50 lines. Header fields are
  fixed offsets; size is an octal ASCII string.
- **Bootloader integration is trivial.** The `bootloader` crate accepts a ramdisk path
  and exposes `ramdisk_addr` / `ramdisk_len` in `BootInfo`. The ramdisk is mapped into
  virtual memory before `kernel_main` runs. The parser validates the `ustar` magic at
  `offset 257` and walks the archive in a single pass.
- **Zero-copy file reads.** File data is never moved — `RamfsFile` holds a `&'static
  [u8]` slice into the bootloader-mapped ramdisk region.

### Custom format (deferred)

A native AmaterasuOS archive could store extended metadata (AmaterasuOS permissions,
`.kami` load hints, content hashes), avoid tar's fixed 100-byte name limit, and use a
binary rather than octal-ASCII size field.

None of these are needed for v0.6's use case (boot-time read-only file tree). A native
format adds parser complexity and eliminates `tar` tooling with no compensating
benefit at this milestone. It remains worth revisiting if the initrd grows to carry
executable files requiring per-file metadata.

---

## File extension conventions above the VFS layer

ADR-003 defines `.sol`, `.kami`, `.amx`, and `.torii` as AmaterasuOS's canonical file
type extensions. These operate strictly above the VFS abstraction:

- The `VNode` trait has no concept of file type. `lookup()` and `read()` are
  content-agnostic.
- Extension interpretation is the responsibility of the caller — a future `.kami`
  module loader or `.amx` process loader will call `vfs::lookup()`, read the bytes,
  and dispatch based on the name suffix or a magic number in the content.
- The ramdisk can carry `.kami` or `.amx` files today; the VFS will serve them. The
  loaders themselves are post-v0.6 work.

---

## Decision

**Adopt a `VNode` trait-based VFS with a single mount point, backed by a ustar ramdisk
parser, for v0.6.**

| Module | Responsibility |
|--------|----------------|
| `vfs.rs` | `VNode` trait, `mount()`, `lookup()`, `with_root()` |
| `ramfs.rs` | ustar parser, `RamfsFile`/`RamfsDir`/`OwnedFile`/`DirRef` VNode impls, `init()` |
| `shell.rs` | `ls`, `cat`, `stat` commands; all path resolution via `vfs::lookup()` |

---

## Consequences

- **Positive:** Shell commands (`ls`, `cat`, `stat`) are fully decoupled from the
  storage backend. Adding a second filesystem requires only a new `VNode` impl and a
  `vfs::mount()` call.
- **Positive:** ustar ramdisk provides boot-time file content with zero tooling
  overhead and a minimal parser.
- **Positive:** File data reads are zero-copy — `cat` reads directly from the
  bootloader-mapped ramdisk region.
- **Negative:** Each `vfs::lookup()` call allocates a `Box<dyn VNode>`. On a kernel
  with a slab allocator this is cheap, but it is not free.
- **Negative:** `DirRef` uses a raw `*const dyn VNode` pointer. The safety invariant
  (tree is `'static`) must be maintained manually; if `vfs::mount()` is ever called
  a second time and the old tree dropped, live `DirRef`s would dangle.

## Known limitations

1. **Read-only.** `VNode` has no `write()` or `create()` methods. All mounted
   filesystems are read-only for v0.6.
2. **Single mount point.** There is one global `ROOT`. Mounting a second filesystem
   at a subdirectory path is not supported.
3. **No permission system.** `VNode` carries no owner, group, or mode bits.
4. **No symbolic links.** `lookup()` does not follow symlinks; symlink creation is
   not defined.
5. **100-byte name limit.** ustar headers store filenames in a fixed 100-byte field.
   Long paths require the GNU or POSIX extension headers, which the parser does not
   implement.
6. **No directory entry for root.** `vfs::lookup("/")` returns `None`; callers
   must use `vfs::with_root()` to access the root node directly.

## Revisit before

- **Writable filesystem milestone:** `VNode` will need `write()`, `create()`,
  `truncate()`, and `unlink()` methods, and the single-mount-point model will need to
  become a mount table.
- **Process loader (`.kami`/`.amx`):** The loader will call `vfs::lookup()` for
  executables. If per-file metadata (load address hints, hash) is needed, either
  the ustar archive gains a sidecar convention or a native initrd format is introduced.
- **Long filenames in ramdisk:** If initrd paths exceed 99 bytes, add GNU long-name
  (`L`-type entry) support to `parse_ustar`.

## Notes on process

Design and rationale produced in discussion with Claude (claude-sonnet-4-6).
Implementation verified on QEMU 8.2.2 (BIOS mode, SeaBIOS 1.16.3).
Final decisions rest with the project owner.
