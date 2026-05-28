# AmaterasuOS — Debugging Guide

## Philosophy

Never debug something on real hardware that you haven't already confirmed in QEMU.
Real hardware is for verifying that QEMU-validated code works on metal.
It is not a primary debugging environment.

When QEMU works and real hardware doesn't, the problem space collapses to a
known-small set: ACPI table differences, firmware-specific APIC routing quirks,
or hardware initialization timing. Serial output tells you exactly where behavior
diverges. That is a tractable problem. Debugging blind on real hardware is not.

---

## The debugging stack

From least to most information, in order of when to reach for each:

```
Serial output (serial_println!)      → always on, zero cost, first tool
QEMU interrupt tracing (-d int)      → see every vector that fires
QEMU GDB stub (-s -S)                → full debugger, breakpoints, register inspect
Logic analyzer (physical signals)    → when you need to see the wire, not the code
Real hardware + serial               → confirm QEMU-validated code works on metal
```

Start at the top. Only move down when the layer above can't answer the question.

---

## Standard QEMU run command

For all development work, use this invocation as the baseline:

```bash
qemu-system-x86_64 \
  -machine q35 \
  -drive format=raw,file=amaterasuos.img \
  -serial stdio \
  -d int,cpu_reset \
  -no-reboot
```

**What each flag does:**

`-machine q35` — uses Intel Q35 chipset emulation instead of the legacy i440fx
default. Q35 generates ACPI tables that more closely match real UEFI hardware
(PCIe topology, IOAPIC configuration). Use this as the default; switch to
`-machine pc` only to reproduce a specific legacy-BIOS behavior.

`-serial stdio` — pipes COM1 directly to your terminal. Every `serial_println!`
appears in real time. This is your primary window into kernel state.

`-d int` — logs every interrupt and exception to stderr. Redirect to a file
with `2>interrupts.log` when you need to search the output.

`-d cpu_reset` — logs the CPU state at the moment of a triple fault, before
the machine resets. Without this, triple faults are silent reboots.

`-no-reboot` — keeps QEMU alive after a crash so you can read the last serial
output before the panic. Without this, a bad panic handler kills the window.

---

## Debugging workflow

Follow these steps in order. Do not skip to real hardware.

### Step 1 — Reproduce in QEMU

If a bug only appears on real hardware and not in QEMU, the first job is to
understand the difference — not to debug on metal. Check:

- ACPI tables: QEMU's MADT may differ from your board's. Compare APIC routing.
- Timing: QEMU is not cycle-accurate. TSC calibration values will differ.
- Firmware behavior: your board's UEFI may initialize hardware differently than OVMF.

Use `-machine q35` (the standard command above) for closer-to-real ACPI behavior.

### Step 2 — Read the serial log first

Before doing anything else, read the full serial output. The boot log timestamps
every initialization phase. The line where output stops is where the kernel died.

If there is no output at all: serial init failed or the kernel triple-faulted
before serial was initialized. Run with `-d int,cpu_reset 2>log` and look for
an early exception vector.

### Step 3 — Trace the interrupt (for driver/IRQ problems)

```bash
qemu-system-x86_64 \
  -machine q35 \
  -drive format=raw,file=amaterasuos.img \
  -serial stdio \
  -d int \
  2>interrupts.log
```

Trigger the condition (press a key, wait for a timer tick, etc.).
Search `interrupts.log` for the expected vector:

| Hardware              | IRQ | Vector (APIC)           | Notes                                 |
|-----------------------|-----|-------------------------|---------------------------------------|
| PS/2 keyboard (QEMU)  | 1   | 0x21 (KBD_VECTOR)       | QEMU PS/2 delivers via IRQ1           |
| USB keyboard (Z390)   | —   | 0x20 (TIMER_VECTOR)     | Timer tick polls XHCI event ring      |
| LAPIC timer           | —   | 0x20 (TIMER_VECTOR)     | Configured in `timer::init()`         |
| Spurious              | —   | 0xFF (SPURIOUS_VECTOR)  | No EOI needed; safe to ignore         |

**If the vector appears in the log:** the hardware and APIC routing are correct.
The problem is in your handler — add a `serial_println!` as the first line of
the handler to confirm it is being called.

**If the vector does not appear:** the interrupt is not reaching the CPU. The
problem is in APIC routing, IDT registration, or the hardware itself. Check:
- I/O APIC redirection table entry for the IRQ
- IDT entry for the target vector
- That `sti` has been called before the interrupt is expected

### Step 4 — Diagnose XHCI keyboard issues

USB keyboards on real hardware are driven by `hal::xhci`, not PS/2. Vector 0x21
will never fire for a USB keyboard — keyboard data arrives via the LAPIC timer
(vector 0x20) polling the XHCI event ring every 1 ms.

When keyboard input is not working on real hardware, trace the XHCI init log
in serial output step by step:

| Serial line                                 | What it means                          |
|---------------------------------------------|----------------------------------------|
| `[PCI] XHCI 00:14.0  BAR0=...`             | Controller found at PCI 0:20.0         |
| `[XHCI] cap=... op=... db=... rt=...`       | Register offsets read correctly        |
| `[XHCI] controller running`                 | Reset and start succeeded              |
| `[XHCI] port N connected, speed=S`          | Port has a device; speed: 3=HS, 4=SS  |
| `[XHCI] port N slot=N`                      | Enable Slot command succeeded          |
| `[XHCI] slot N addressed`                   | Address Device command succeeded       |
| `[XHCI] slot N dev: class=...`              | Device descriptor read OK              |
| `[XHCI] HID kbd: cfg=N iface=N ep=0x81...` | HID boot keyboard interface found      |
| `[XHCI] Configure Endpoint cc=1`           | Endpoint configured (cc=1 = SUCCESS)   |
| `[XHCI] keyboard ready on slot N`           | Driver armed; keyboard should work     |

**Common completion codes (`cc=N`):**

| Code | Name                   | Likely cause                                     |
|------|------------------------|--------------------------------------------------|
| 1    | SUCCESS                | —                                                |
| 5    | USB_TRANSACTION_ERROR  | Device not responding; check USB cable/port      |
| 6    | TRB_ERROR              | Bad TRB construction; likely a driver bug        |
| 13   | SHORT_PACKET           | Success — device returned fewer bytes than asked |
| 22   | STOPPED                | Ring stopped; re-arm or reset needed             |
| 255  | timeout (driver)       | XHCI not completing within the deadline          |

**If `[PCI] no XHCI controller found`:** Check that the XHCI BAR0 is enabled in
PCI config space. The PCI scan only checks function 0 of each device. If the
controller is at a non-zero function, extend the scan in `pci.rs`.

**If init stalls with no serial output after `[XHCI] cap=...`:** The BIOS
ownership handoff may be looping. The controller's BIOS-owned semaphore (bit 16
of the Legacy Support capability) should clear within 1 second. If it doesn't,
the firmware is not releasing ownership — try disabling "Legacy USB Support" in
the UEFI setup (this forces the firmware to release the controller earlier).

**If keyboard still silent after `[XHCI] keyboard ready`:** Check that the
interrupt endpoint was armed. The TRB was queued at init; after the first
completion, `take_hid_report()` re-arms it. Add a serial print inside
`take_hid_report()` to confirm the event ring is delivering Transfer Events.

### Step 5 — Attach GDB for logic problems

When you know an interrupt is firing but behavior is wrong, use the GDB stub
to step through the handler:

```bash
# Terminal 1 — start QEMU paused
qemu-system-x86_64 \
  -machine q35 \
  -drive format=raw,file=amaterasuos.img \
  -serial stdio \
  -d int,cpu_reset \
  -no-reboot \
  -s -S

# Terminal 2 — attach GDB
gdb
(gdb) target remote :1234
(gdb) symbol-file kernel/target/x86_64-unknown-none/debug/amaterasu_kernel
(gdb) break kernel_main
(gdb) continue
```

Useful GDB commands for kernel debugging:

```
(gdb) info registers          # dump all CPU registers
(gdb) x/10i $rip             # disassemble 10 instructions at current PC
(gdb) x/4gx 0xffff8000...   # examine memory at address
(gdb) break idt::timer_handler  # break on a specific handler
(gdb) stepi                   # step one instruction
(gdb) watch *0x1234           # hardware watchpoint on a memory address
```

Note: GDB with `-s -S` works in QEMU only. For real hardware debugging at this
level, a JTAG probe or PCIe debug adapter is required (out of scope for now).

### Step 6 — Test on real hardware

Only reach this step after the behavior is confirmed correct in QEMU.

```bash
make usb DEV=/dev/sdX    # write UEFI image to USB with safety checks
```

Connect your serial cable (see Hardware Setup below).
Boot the test machine.
Watch the serial log on your daily driver via minicom.

**If QEMU worked but real hardware doesn't:**
The delta is almost always one of:
- APIC routing: your board's MADT differs from OVMF's. Print the MADT entries
  at boot and compare against QEMU output.
- GOP framebuffer: real firmware GOP implementations vary. Log the framebuffer
  base address and pixel format at init.
- Timing: TSC frequency will differ. Check calibration output.
- PS/2 controller: QEMU has a virtual PS/2 controller. On real UEFI hardware
  with no physical PS/2 port, USB legacy SMM may or may not be active after
  ExitBootServices. On Intel Z390 (and many modern boards), SMM stops writing
  to port 0x60 after ExitBootServices — keyboard input then comes from XHCI.

---

## Hardware setup for real hardware debugging

### Minimum viable (~$15)

**What you need:**
- A CP2102-based USB-to-serial adapter (~$8). Prefer CP2102 over CH340 —
  better driver support and more electrically stable.
- Jumper wires or a null modem cable (~$5).
- A second machine to receive serial output (your daily driver).

**Physical connection:**

If your test machine has an external DE9 serial port:
```
Test machine DE9     USB-serial adapter
  Pin 2 (RX)  ←───  TX
  Pin 3 (TX)  ────→  RX
  Pin 5 (GND) ─────  GND
```

If your test machine has only an internal COM header (check your motherboard
manual — most modern boards have one even without an external port):
```
Motherboard COM1 header    USB-serial adapter
  Pin 2 (RX)  ←────────  TX
  Pin 3 (TX)  ─────────→  RX
  Pin 5 (GND) ─────────   GND
```
A $5 header-to-DE9 bracket from eBay breaks out the internal header.

**Receiving on your daily driver:**

```bash
# Install
sudo apt install minicom        # Linux
brew install minicom            # Mac
# Windows: PuTTY → Serial → your COM port → 115200

# Connect (AmaterasuOS uses 115200 baud)
minicom -D /dev/ttyUSB0 -b 115200 --color=on
```

### Comfortable setup (~$100–200)

**KVM switch** — one keyboard and monitor shared between your daily driver and
test machine. A 2-port HDMI KVM is ~$30–60. You keep minicom open in a terminal
window on your daily driver while watching the test machine's framebuffer through
the same monitor.

**Logic analyzer** — a Saleae Logic 8 clone (~$15 on AliExpress) probes
physical signals. Use it to verify PS/2 clock/data lines are toggling when
you press a key, or to confirm serial TX is actually transmitting.

---

## Quick reference — common failure patterns

| Symptom | Likely cause | First check |
|---|---|---|
| No serial output at all | Triple fault before serial init, or serial not initialized | `-d int,cpu_reset 2>log`, look for early exception |
| Serial stops mid-boot | Panic or triple fault at that phase | `-no-reboot`, read last lines before stop |
| Interrupt never fires | APIC routing, IDT not set up, `sti` not called | `-d int`, search for expected vector |
| Interrupt fires, handler not called | IDT entry points to wrong address | GDB, break on expected handler, check IDT entries |
| Works in QEMU, not on hardware | Firmware difference | Compare MADT, GOP info, TSC calibration in serial log from both |
| USB keyboard silent on real hardware | XHCI init failed, or `take_hid_report()` not delivering | Check XHCI serial trace; see Step 4 above |
| PS/2 keyboard broken in QEMU | Scancode translation (bit 6 of 8042 config) disabled | `ps2::init()` must not clear bit 6; QEMU delivers Set 2 without it |
| Triple fault on boot | Stack not set up, bad GDT, null pointer dereference | `-d int,cpu_reset`, check exception vector and RIP |

---

## Invariants — do not change these without understanding why

These constraints exist in the current codebase and are not arbitrary:

- `serial::init()` must be the **first** initialization call. The panic handler
  uses serial before anything else is set up. If serial init moves later,
  early panics become silent.

- `cli` + `pic::mask_all()` must be called **before** `apic::init()`. The PIC
  must be masked before the LAPIC takes over. Only DATA ports (0x21/0xA1) are
  written — do NOT call `pic::remap()` on UEFI systems. Writing ICW1_INIT to
  the CMD ports (0x20/0xA0) triggers an SMI trap on Intel Z390 and hangs the
  boot before the IDT is loaded.

- `idt::init()` must be called **before** `sti`. Enabling interrupts before the
  IDT is loaded causes an immediate triple fault on the first interrupt.

- `sti` must be called **after** both `idt::init()` and `apic::init()`. The
  LAPIC and I/O APIC must be configured before any interrupt can be safely
  delivered.

- `timer::init()` must come **after** `sti`. The PIT-based calibration loop
  polls a hardware bit (port 0x61 bit 5) and does not require interrupts, but
  the periodic timer it arms will deliver its first tick immediately after
  `init()` returns. The IDT and APIC must be ready before that first tick.

- `xhci::init()` must come **after** `timer::init()`. The XHCI driver uses
  `time::rdtsc()` and `time::tsc_mhz()` for busy-wait timeouts — these require
  `time::calibrate()` (which runs before serial init) and produce meaningful
  values only after `timer::init()` has confirmed the TSC frequency.

- `t0 = time::rdtsc()` is the **first line of kernel_main**. All boot-time
  measurements are relative to this baseline. Any code added before this line
  will not appear in the boot log.
