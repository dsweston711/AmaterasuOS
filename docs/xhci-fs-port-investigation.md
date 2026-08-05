# Investigation Log: Full Speed ports stuck at PLS=7 (Polling)

**Issue:** [#155](https://github.com/dsweston711/AmaterasuOS/issues/155) — follow-up to #153/#154
**Branch/PR:** `feat/155-xhci-keyboard-ring-fix` / PR #156
**Status as of 2026-07-30:** Unresolved. Still stuck. This document exists so a future
session (mine or otherwise) doesn't have to reconstruct this from scratch.

---

## Environment

- Board: MSI MEG Z390 ACE, CPU: Intel i7-9700K
- Keyboard: Keychron, connected via a USB-C→USB-A adapter (confirmed passive/electrically
  irrelevant — standard configuration, not a suspect)
- Also connected: the USB flash drive AmaterasuOS is booted from (shows up as its own xHCI
  device during our own enumeration — not the keyboard, a mass-storage device, see below)
- No serial adapter available this whole investigation — all real-hardware diagnostics are
  read off the framebuffer (`crate::println!`), photographed or transcribed by hand

---

## Three real bugs found and fixed already (not the current blocker — these are done)

1. **Link TRB cycle bit stomped before xHC traversal** — `arm_kbd_ep_raw()` updated the
   interrupt endpoint's Link TRB cycle bit on every call instead of only on wrap, causing the
   ring to permanently stall after exactly 7 HID reports. Fixed, QEMU-verified.
2. **Ports never powered on before checking `CCS`** — real silicon needs explicit
   `PORTSC.PP=1`; QEMU's emulated xHC didn't require this. Fixed.
3. **Device slot leak** — `MaxSlotsEn` was hardcoded to 2 with no `Disable Slot` on
   enumeration failure, starving out every port enumerated after the first couple of
   failures. Fixed: slots are now freed on failure and `MaxSlotsEn` uses the controller's
   full reported `MaxSlots`.

These are all still in the code and working. Not what this document is about.

---

## The current blocker

Every **Full Speed** port (where the Keychron actually is — ports have varied across boots
depending on what's plugged in, e.g. 2/3/7/9/13, or 2/4/7/9/13) fails with the exact same
signature, every single time, regardless of which physical device or port:

```
port N connected, speed=1
port N reset failed or not enabled, portsc=0x000206e1   (after 3 retries, all stuck at PLS=7)
...
port N slot=M
slot M Address Device(BSR=1) cc=1                        <- succeeds (no real bus traffic)
slot M pre-transfer portsc=0x000206e1 PLS=7               <- NOT U0
ctrl_in FAILED cc=4 residual=8 stage=Setup trb_idx=0       <- 0 bytes ever left the wire
slot M post-fail portsc=0x000206e1 PLS=7                   <- still stuck
slot M GET_DESCRIPTOR(Device,8) at default address failed
```

`PLS=7` is "Polling" in the xHCI Port Link State encoding — technically a SuperSpeed
link-training state, not one that should normally appear as a steady-state value on a
Full-Speed-tier port at all. The port never trains to U0, so no real data transfer can ever
succeed — this is true regardless of Slot Context contents, MaxPacketSize0, or anything else
downstream, because `Address Device(BSR=1)`/`Enable Slot` never touch the actual USB wire in
the first place (pure xHC-internal bookkeeping). `GET_DESCRIPTOR` is the first real bus
transaction attempted, and it's the first thing that fails.

The one thing that has ever succeeded — a SuperSpeed device (`speed=4`, port 24), almost
certainly the boot flash drive itself (`class=0x08` Mass Storage on its one interface) —
reaches `PLS=0` (U0) cleanly every time. This device's `bMaxPacketSize0` reads back as `9`,
which is correct for SuperSpeed (that field is a power-of-2 *exponent* there: 2⁹=512), but we
currently write the raw `9` straight into the EP0 context unconverted. **Known latent bug,
not yet fixed, not related to the FS problem** — flagged for later.

**Confirmed via UEFI/BIOS setup screens: this exact keyboard, port, and cable work.** This
proves the hardware is fully functional and the bug is entirely in our own controller/port
bring-up sequence — something firmware does that we don't.

---

## Hypotheses tried for the PLS=7 stall specifically, in order

| # | Hypothesis | What was done | Result |
|---|---|---|---|
| 1 | USB 2.0 reset-recovery time (TRSTRCY) missing | Added 10ms delay after port reset, before Enable Slot/Address Device | No effect. Also: `PORTSC` dump proved reset was already completing fine (`PED=1`) before this point — theory didn't even apply |
| 2 | Wrong guessed `MaxPacketSize0` (8) for Full Speed | Hardcoded 64 instead | Identical failure. Reverted |
| 3 | Slot Context Speed field wrong (non-default PSI mapping) | Parsed the controller's actual Supported Protocol Capability PSI table | `rev=2.0` table matches the default mapping (`id=1=12Mb/s=FS` etc.) **exactly**. Ruled out completely |
| 4 | Something interposes during the Address Device wait (disconnect, glitch) | Logged every event TRB (not just the one each waiter expects) | Clean, direct `cc=4` every time — nothing else fires in between. Ruled out |
| 5 | Guessing MPS is wrong in general (not just the specific guess) | Implemented the *proper* `BSR=1` → read real descriptor → `Evaluate Context` → `BSR=0` sequence (not another guess — this asks the device) | `Address Device(BSR=1)` succeeds cleanly on every FS port (proves Slot Context itself was never the problem). The failure **moved** to the first real transfer (`GET_DESCRIPTOR`) — kept this fix, it's correct regardless, but it didn't solve the stall |
| 6 | Don't know which TRB stage actually fails, or how much data (if any) comes back | Logged residual length + failing TRB stage from the Transfer Event | `stage=Setup`, `residual=8` — **zero bytes** ever leave the wire. Not a partial/garbled response — no response at all |
| 7 | Port might not actually be at U0 despite `PED=1` | Logged `PORTSC`/`PLS` immediately before and after the failing transfer | **This was the breakthrough.** `PLS=7` (Polling) on every FS port, both before and after, vs. `PLS=0` (U0) on the one working SS device |
| 8 | Just needs more patience / retry | Retried the full reset sequence up to 3 times if `PLS` doesn't reach `U0` within 100ms | No effect — stuck at `PLS=7` through all 3 attempts, every time |
| 9 | Genuine electrical/hardware fault | Checked keyboard in UEFI/BIOS setup screens | **Works fine.** Proves hardware is not the problem |
| 10 | PHY wedged mid-training, needs a real power cycle (not just "turn on if off") | Forced explicit `PP=0` → settle → `PP=1` → settle for every port | **Made things worse** — the previously-100%-reliable SS device regressed to `PLS=4` (Disabled) and failed too; no FS port even reconnected within the settle window. Reverted |
| 11 | Missing an Intel-specific HCRST timing quirk | Found via Linux's `drivers/usb/host/xhci.c`: Intel controllers need a 1ms delay after setting `CMD_RESET`, before touching any HC register, citing rare hangs without it (`XHCI_INTEL_HOST` quirk, not part of the generic xHCI spec at all) | Added the exact delay. **No effect** — still `PLS=7` on every FS port, identical signature. Kept the delay in (it's real, spec-adjacent, harmless), but it wasn't the fix |

---

## Resources checked

- **`Manuals/337867_CNL_PCH_LP_Datasheet_rev006.pdf`** — wrong chipset variant (LP = mobile,
  not the desktop Z390/H-series PCH) and board/power-level only anyway — no PCI config
  register or clock-gating detail for the xHCI function. Confirmed via full-text search
  (`pdftotext`) for "clock gat", "D20:F0", "usb2pdo", "port disable" etc. — nothing useful.
- **`Manuals/xHCI__Rev1.2c.pdf`** — the generic xHCI spec. Everything derivable from this has
  already been used (Slot Context layout, PORTSC bits, PSI tables, TRB formats). This is not
  where an Intel-specific quirk would live by definition.
- **Linux `drivers/usb/host/{xhci.c,xhci-pci.c,xhci-ring.c,xhci-mem.c,xhci-hub.c,xhci.h}`**
  (pulled from `torvalds/linux` via `raw.githubusercontent.com`) — searched for every
  `XHCI_INTEL_HOST` usage:
  - `xhci.c` CMD_RESET delay — tried (#11 above), no effect
  - `xhci.c` U1/U2 LPM timeout calculation (`xhci_calculate_intel_u1/u2_timeout`) — about
    link power management tuning for already-connected devices, not initial enumeration.
    Not applicable.
  - `xhci.c` `xhci_check_tier_policy` — limits U1/U2 LPM based on USB hub tier depth
    (`tier > 3`). Not applicable — this is a directly-attached root port scenario.
  - `xhci-mem.c` `xhci_get_endpoint_mult` — about isochronous endpoint bandwidth doubling
    (`Mult` field). Not applicable — nowhere near control-transfer/enumeration.
  - No other `INTEL`-specific quirks in these files touch port reset, port power, or link
    training before device addressing.
- **`Manuals/hid1_11.pdf`, `Manuals/hut1_7.pdf`** — HID class spec and usage tables. Not
  relevant yet; we never get far enough to need these for the FS ports.

---

## What's genuinely still open

- **The actual desktop Z390/300-series H-series PCH datasheet** (not the LP/mobile one) may
  have BIOS-writer's-guide-level detail the LP datasheet doesn't. Worth finding if available
  without an NDA.
- **Hub/Route String topology** — the driver assumes every device is a direct child of an
  xHC root port (Route String always 0, no TT/hub fields ever set). Never actually tested
  whether any of these "root" ports are secretly behind an onboard/internal hub chip.
- **A serial adapter** (~$8-15, CP2102 preferred) would help enormously — we're currently
  limited to whatever fits on the framebuffer, one line at a time, hand-transcribed or
  photographed. Real-time full logs would let us correlate timing much more precisely.
- **Warm Reset (WPR) vs Port Reset (PR)** for the SuperSpeed tier specifically was
  never implemented — irrelevant to the FS stall, but relevant to the still-open SS
  `bMaxPacketSize0` encoding bug noted above.
- Have not yet tried: comparing behavior with **Legacy USB Support disabled** in BIOS (README
  says it must be enabled for keyboard input to work at the OS level, but its effect on our
  own xHC bring-up sequence specifically has never been isolated as a variable).
- Have not yet tried booting **without** the USB flash drive also plugged in (to rule out any
  interaction between two simultaneously-enumerating devices during our port loop).
