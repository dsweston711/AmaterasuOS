# AmaterasuOS — Architectural Workshopping

Ideas being actively thought through. These are not yet ADRs and carry no
implementation obligation. Each section names the decision space, sketches the
leading candidate, and lists what still needs to be resolved before it can
graduate to an ADR.

---

## 1. ARP I/O model on top of DriverEntry

**The gap this fills:** ADR-014 defines how a driver *registers and manages its lifecycle* (`DriverEntry` with `probe`/`start`/`stop`/`remove`/`isr`/`dpc`). It does not define how *I/O requests* flow through a running driver. A keyboard read, a disk write, an ioctl — these need a request format and a dispatch path. That is what the ARP model addresses.

**Leading candidate:** The AmaterasuOS Request Packet (ARP) is the I/O request
layer that sits above `DriverEntry`, modeled on Windows NT's IRP.

```
                 ┌──────────────────────────────────────────┐
                 │             amtos_io                      │
                 │  - device registry (DriverEntry table)    │
                 │  - ARP allocator + routing                │
                 └────────────────┬─────────────────────────┘
                                  │ routes ARP to driver's dispatch entry
                                  ▼
                 ┌──────────────────────────────────────────┐
                 │             DriverEntry (ADR-014)         │
                 │  dispatch: [Option<fn(ARP)>; N_MAJOR]     │
                 │  probe / start / stop / remove            │
                 │  isr / dpc                                │
                 └──────────────────────────────────────────┘
```

How it works:

1. A driver registers a `DriverEntry` with `amtos_io`. As part of registration it
   populates a dispatch table — one slot per major function code (Read, Write,
   DeviceControl, etc.).
2. An I/O request originates in the shell or a future subsystem. It calls into
   `amtos_io`, which allocates an ARP, fills in the major function code and
   parameters, and routes it to the target device's driver dispatch slot.
3. The driver processes the ARP synchronously (for now) and calls a completion
   routine. `amtos_io` propagates the result back to the caller.
4. Lifecycle calls (`probe`, `start`, `stop`, `remove`) bypass the ARP path
   entirely — the kernel calls these directly through the `DriverEntry` struct.
   They are not I/O requests; they are lifecycle events.

**Why not IRPs directly?** The naming (ARP vs IRP) is intentional. We borrow the
*concept* (typed request packets, layered dispatch, completion routines) but we
are not trying to binary-compatible clone NT. The ARP struct can be smaller and
simpler than a full IRP while still supporting the same architectural patterns.

**What `amtos_io` owns vs. what a driver owns:**

| Concern | Owner |
|---------|-------|
| ARP struct definition | `amtos_io` |
| ARP allocation and routing | `amtos_io` |
| Device-to-driver binding | `amtos_io` registry |
| Dispatch table entries | driver (set during `probe`) |
| ISR / DPC | driver (set in `DriverEntry`) |
| Lifecycle sequencing | kernel (calls `probe` → `start` via `DriverEntry`) |

**Open questions before this becomes an ADR:**

- **Sync vs. async completion:** Starting synchronous is correct for the current
  workload. The question is what the completion signature looks like so it can be
  extended to async later without changing driver code. A `CompletionStatus` return
  value from the dispatch function is probably enough for now.
- **Layered drivers (filter stacks):** The handoff calls out that filter drivers
  must be composable on top of base drivers. How does an ARP pass through a filter
  driver to the base driver? NT does this with `IoCallDriver` and a stack of device
  objects. Do we need that now, or can we stub it?
- **ARP struct layout:** What fields are required vs. optional? At minimum: major
  function, device handle, buffer pointer, buffer length, status. What else?
- **Where does `amtos_io` live relative to ADR-009's layers?** It is Executive
  Services (Layer 3). Drivers are HAL-adjacent (Layer 1 or a new Layer 1.5). The
  dependency direction is `drivers → amtos_ddk → amtos_io → kernel`. This matches
  the ADR-009 downward-only rule if amtos_io is Layer 3 and drivers are Layer 1.

---

## 2. Static driver registration

**Decision space:** Should drivers be compiled into the kernel binary, or loaded
at runtime from storage?

**Leading candidate:** Static registration — drivers are `&'static DriverEntry`
entries in a compile-time array. No loader, no dynamic linking, no ELF parser
required. A driver table is initialized before `amtos_io` starts, and `amtos_io`
iterates it at boot to probe devices.

**Why this and not dynamic loading:**

- Dynamic loading requires a loader, which requires a working filesystem, which
  requires a storage driver — a circular dependency that cannot be resolved before
  the first real driver exists.
- A static table is consistent with the sub-3-second boot constraint: no
  filesystem reads, no ELF relocation, no symbol resolution on the boot path.
- Every driver that ships is reviewed and compiled by the kernel maintainer. This
  is currently correct for an OS with no user-facing driver API.

**Consequences:**

- Adding a new driver means recompiling the kernel. For the current project scale
  this is fine.
- The driver table could be organized into a linker section (like Linux's
  `__initcall` scheme) to avoid manually listing drivers in `main.rs`. This is an
  implementation detail, not an architectural one.

**Open questions before this becomes an ADR:**

- When (if ever) does dynamic loading become worth revisiting? Likely not until
  there is a stable storage driver, a process model, and a verified boot chain.
  Suggest a revisit trigger: "first time a vendor asks to ship a closed-source
  driver."
- Does the static table live in `amtos_io` (as the registry) or in `kernel/src/`
  as a wired-in init step? Preference: it belongs in `amtos_io/src/registry.rs`
  so the kernel does not need to know individual driver names.

---

## 3. No global config registry

**Decision space:** Should there be a central configuration store (like Windows
Registry or Linux sysfs parameters) that drivers and subsystems read at init time,
or should configuration be passed directly to each component at registration?

**Leading candidate:** No global registry. Each driver receives a config struct
(passed as a parameter to `probe`) that the kernel constructs at boot. There is
no central key-value store.

**Why this:**

- A global registry is a singleton — it must be initialized before any driver that
  reads from it, making boot ordering more complex.
- It is a single point of failure: a corrupted or missing key can affect any
  driver.
- For a system where drivers are compiled in and configuration is static (no user
  install-time config), the registry pattern provides no real benefit over direct
  struct passing.
- It can always be added later; removing one that grew too large is much harder.

**Constraints this implies:**

- Driver configuration is determined at compile time or derived from hardware
  (ACPI tables, PCI config space) — not from a persistent store.
- `amtos_io` passes an opaque config pointer to `probe`; the driver knows its own
  config struct type. The I/O manager does not inspect it.

**Open questions before this becomes an ADR:**

- What about configuration that genuinely needs to persist across reboots (e.g.,
  user-set hostname, network interface config)? At that point a file-backed config
  (read from the ramdisk or eventually a storage partition) is probably the right
  answer, not a registry. This is a future concern.
- Is there an argument for a minimal runtime-mutable config slab for things like
  IRQ routing overrides? Probably not until real hardware brings up firmware
  quirks that can't be handled with ACPI table parsing.

---

## 4. Boot phase gating

**Decision space:** How is init ordering enforced? Right now `kernel_main` has an
implicit ordering that is correct but unenforced — any refactor could silently
break it. Should phases be made explicit?

**Leading candidate:** Named boot phases with a gate function. Each phase has a
name and an entry condition. A subsystem that tries to init before its dependencies
are up either panics (in debug) or is deferred.

```
Phase 0 — SERIAL_ONLY      serial, panic handler
Phase 1 — MEMORY           physical memory map, slab allocator, heap
Phase 2 — HAL              PIC remap, APIC, IDT, framebuffer, timers
Phase 3 — EXECUTIVE        ramdisk, VFS, env, ACPI
Phase 4 — DRIVER_PROBE     amtos_io probes device table
Phase 5 — SHELL            interactive prompt
```

The current `kernel_main` already follows this order. The question is whether to
make it a data structure or leave it as sequential function calls.

**Why formalize it:**

- The existing `t0`, `t_serial`, `t_mem`, `t_alloc`... TSC measurements already
  implicitly define phase boundaries. Naming them makes the contract explicit.
- When `amtos_io` and drivers are added (Phase 4), there is a real risk of a
  driver trying to access HAL services before Phase 2 completes. A phase gate
  catches this at boot rather than producing a silent data corruption.

**Why not over-engineer it:**

- The simplest version is a global `AtomicU8` current phase, with `assert_phase_ge`
  calls at the top of any init function that has ordering requirements. This adds
  ~5 lines of code per subsystem and zero runtime overhead after boot.
- A full dependency graph resolver is not warranted for a statically ordered boot.

**Open questions before this becomes an ADR:**

- Should phase gating be debug-only (panic if out of order) or always-on?
  Probably debug-only — release builds skip the check after boot is validated.
- Does the boot-time log (`docs/boot-time-log.md`) need to align its stage names
  with the phase names decided here? Probably yes, for consistency.
- Where does the phase counter live? `kernel/src/phase.rs` is the natural home,
  but it must be initialized before anything else — even before `serial::init`.
  An `AtomicU8` with `0` as the initial value satisfies this.
