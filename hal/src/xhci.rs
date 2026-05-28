// Minimal XHCI HID Boot Keyboard driver.
//
// Architecture: polling-only, single keyboard device, no MSI interrupts.
// Called from the 1 ms LAPIC timer tick (hal::timer).
// Public API:
//   init(phys_off)           — full initialization, safe to call before timer starts
//   take_hid_report()        — called every timer tick; returns Some([u8;8]) on new key event

use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

extern crate alloc;

// ── XHCI capability register offsets ────────────────────────────────────────
const CAP_CAPLENGTH:  usize = 0x00; // u8  — length of capability register block
const CAP_HCSPARAMS1: usize = 0x04; // u32 — MaxSlots[7:0], MaxIntrs[18:8], MaxPorts[31:24]
const CAP_HCSPARAMS2: usize = 0x08; // u32 — scratchpad counts
const CAP_HCCPARAMS1: usize = 0x10; // u32 — CSZ bit 2, XECP bits [31:16]
const CAP_DBOFF:      usize = 0x14; // u32 — doorbell array offset from cap base
const CAP_RTSOFF:     usize = 0x18; // u32 — runtime register space offset from cap base

// ── XHCI operational register offsets (relative to op base = cap + CAPLENGTH) ──
const OP_USBCMD:  usize = 0x00;
const OP_USBSTS:  usize = 0x04;
const _OP_DNCTRL: usize = 0x14;
const OP_CRCR:    usize = 0x18; // 64-bit Command Ring Control Register
const OP_DCBAAP:  usize = 0x30; // 64-bit Device Context Base Address Array Pointer
const OP_CONFIG:  usize = 0x38;

// Port register set: op + 0x400 + 0x10 * (port_index)
// port_index is 0-based; USB ports are 1-based so port N is at index N-1.
const OP_PORTSC_BASE: usize = 0x400;

// ── Runtime interrupter 0 offsets (from rt_base + 0x20) ─────────────────────
const _IR_IMAN:  usize = 0x00;
const _IR_IMOD:  usize = 0x04;
const IR_ERSTSZ: usize = 0x08;
// offset 0x0C reserved
const IR_ERSTBA: usize = 0x10; // 64-bit
const IR_ERDP:   usize = 0x18; // 64-bit

// ── PORTSC bits ──────────────────────────────────────────────────────────────
const PORTSC_CCS: u32 = 1 << 0;  // Current Connect Status
const PORTSC_PED: u32 = 1 << 1;  // Port Enabled/Disabled
const PORTSC_PR:  u32 = 1 << 4;  // Port Reset
const _PORTSC_PLS_MASK: u32 = 0xF << 5;
const PORTSC_PP:  u32 = 1 << 9;  // Port Power
const PORTSC_CSC: u32 = 1 << 17; // Connect Status Change
const PORTSC_PEC: u32 = 1 << 18; // Port Enable/Disable Change
const PORTSC_PRC: u32 = 1 << 21; // Port Reset Change
const PORTSC_CHANGE_BITS: u32 = PORTSC_CSC | PORTSC_PEC | PORTSC_PRC
    | (1 << 19) | (1 << 20) | (1 << 22) | (1 << 23); // all RW1C change bits

// ── TRB type codes ───────────────────────────────────────────────────────────
const TRB_NORMAL:    u32 = 1;
const TRB_SETUP:     u32 = 2;
const TRB_DATA:      u32 = 3;
const TRB_STATUS:    u32 = 4;
const TRB_LINK:      u32 = 6;
const TRB_ENABLE_SLOT:  u32 = 9;
const TRB_ADDR_DEVICE:  u32 = 11;
const TRB_CFG_EP:       u32 = 12;
const TRB_EVT_TRANSFER: u32 = 32;
const TRB_EVT_CMD:      u32 = 33;
const _TRB_EVT_PORT:    u32 = 34;

// Completion codes (bits [31:24] of event TRB dw2)
const CC_SUCCESS:   u8 = 1;
const CC_SHORT_PKT: u8 = 13;

// ── DMA-safe types ───────────────────────────────────────────────────────────

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Trb { dw: [u32; 4] }
impl Trb {}

const CMD_N: usize = 16; // command ring slots (last = Link TRB)
const EVT_N: usize = 64; // event ring slots
const EP0_N: usize = 8;  // EP0 transfer ring slots (last = Link TRB)
const INT_N: usize = 8;  // interrupt endpoint transfer ring slots (last = Link TRB)

#[repr(C, align(64))] struct Dcbaa    { e: [u64; 256] }
#[repr(C, align(64))] struct SpArr    { e: [u64; 32]  } // scratchpad buffer pointer array
#[repr(C, align(64))] struct CmdRing  { t: [Trb; CMD_N] }
#[repr(C, align(64))] struct EvtRing  { t: [Trb; EVT_N] }
#[repr(C, align(16))] struct ErstEntry { addr: u64, size: u16, _pad: [u8; 6] }
#[repr(C, align(64))] struct Erst     { e: [ErstEntry; 1] }
#[repr(C, align(64))] struct InCtx    { e: [[u32; 8]; 33] } // Input Control + 32 device contexts
#[repr(C, align(64))] struct OutCtx   { e: [[u32; 8]; 32] } // 32 device contexts (slot + 31 EPs)
#[repr(C, align(64))] struct Ep0Ring  { t: [Trb; EP0_N] }
#[repr(C, align(64))] struct IntRing  { t: [Trb; INT_N] }
#[repr(C, align(64))] struct HidBuf   { d: [u8; 8] }
#[repr(C, align(4096))] struct SpPage { _d: [u8; 4096] }

unsafe fn alloc_zeroed<T>() -> *mut T {
    let b = Box::new(core::mem::zeroed::<T>());
    Box::into_raw(b)
}

// ── Global state for keyboard polling ────────────────────────────────────────

static KBD_READY:     AtomicBool  = AtomicBool::new(false);
static KBD_SLOT:      AtomicUsize = AtomicUsize::new(0);
static KBD_EP_DB:     AtomicU32   = AtomicU32::new(0);   // doorbell target (ep context index)
static KBD_DB_BASE:   AtomicUsize = AtomicUsize::new(0);
static KBD_RT_BASE:   AtomicUsize = AtomicUsize::new(0);
static KBD_EVT_BASE:  AtomicUsize = AtomicUsize::new(0);
static KBD_EVT_PHYS:  AtomicUsize = AtomicUsize::new(0);
static KBD_EVT_DEQ:   AtomicUsize = AtomicUsize::new(0);
static KBD_EVT_CYC:   AtomicU32   = AtomicU32::new(1);   // consumer cycle, starts 1
static KBD_TR_BASE:   AtomicUsize = AtomicUsize::new(0);
static KBD_TR_PHYS:   AtomicUsize = AtomicUsize::new(0);
static KBD_TR_ENQ:    AtomicUsize = AtomicUsize::new(0);
static KBD_TR_CYC:    AtomicU32   = AtomicU32::new(1);
static KBD_BUF_VIRT:  AtomicUsize = AtomicUsize::new(0);
static KBD_BUF_PHYS:  AtomicUsize = AtomicUsize::new(0);

// ── MMIO helpers ─────────────────────────────────────────────────────────────

#[inline] unsafe fn rd32(base: usize, off: usize) -> u32 {
    ((base + off) as *const u32).read_volatile()
}
#[inline] unsafe fn wr32(base: usize, off: usize, v: u32) {
    ((base + off) as *mut u32).write_volatile(v);
}
// lo dword first (standard for most 64-bit XHCI registers)
#[inline] unsafe fn wr64_lo(base: usize, off: usize, v: u64) {
    ((base + off    ) as *mut u32).write_volatile(v as u32);
    ((base + off + 4) as *mut u32).write_volatile((v >> 32) as u32);
}
// hi dword first (required for CRCR — lo write activates the ring)
#[inline] unsafe fn wr64_hi(base: usize, off: usize, v: u64) {
    ((base + off + 4) as *mut u32).write_volatile((v >> 32) as u32);
    ((base + off    ) as *mut u32).write_volatile(v as u32);
}
#[inline] fn phys(virt: usize, po: usize) -> usize { virt - po }
#[inline] unsafe fn rd8(base: usize, off: usize) -> u8 {
    ((base + off) as *const u8).read_volatile()
}

// ── Timeout helpers (rdtsc-based, calibrated TSC required) ───────────────────

fn deadline_cycles(us: u64) -> u64 {
    crate::time::rdtsc().wrapping_add(us * crate::time::tsc_mhz() as u64)
}
fn past(deadline: u64) -> bool {
    crate::time::rdtsc().wrapping_sub(deadline) < (1u64 << 63)
}

// ── XHCI ownership handoff ───────────────────────────────────────────────────

unsafe fn bios_handoff(cap: usize) {
    let hccparams1 = rd32(cap, CAP_HCCPARAMS1);
    let xecp_words = (hccparams1 >> 16) & 0xFFFF;
    if xecp_words == 0 { return; }

    let mut ptr = cap + xecp_words as usize * 4;
    loop {
        let hdr = rd32(ptr, 0);
        if (hdr & 0xFF) == 1 {
            // USB Legacy Support capability
            let bios_owned = hdr & (1 << 16);
            if bios_owned != 0 {
                // Set OS Owned Semaphore (bit 24)
                wr32(ptr, 0, hdr | (1 << 24));
                let dl = deadline_cycles(1_000_000); // 1 second
                while rd32(ptr, 0) & (1 << 16) != 0 {
                    if past(dl) {
                        crate::serial_println!("[XHCI] BIOS handoff timeout");
                        break;
                    }
                    core::hint::spin_loop();
                }
                crate::serial_println!("[XHCI] BIOS handoff done");
            }
            return;
        }
        let next = (hdr >> 8) & 0xFF;
        if next == 0 { break; }
        ptr += next as usize * 4;
    }
}

// ── Controller reset ─────────────────────────────────────────────────────────

unsafe fn xhci_reset(op: usize) -> bool {
    // Stop if running
    let cmd = rd32(op, OP_USBCMD);
    if cmd & 1 != 0 {
        wr32(op, OP_USBCMD, cmd & !1);
        let dl = deadline_cycles(100_000);
        while rd32(op, OP_USBSTS) & 1 == 0 { // wait for HCH=1
            if past(dl) { crate::serial_println!("[XHCI] stop timeout"); return false; }
            core::hint::spin_loop();
        }
    }
    // Reset
    wr32(op, OP_USBCMD, 1 << 1);
    let dl = deadline_cycles(100_000);
    loop {
        if rd32(op, OP_USBCMD) & (1 << 1) == 0 { break; }
        if past(dl) { crate::serial_println!("[XHCI] reset timeout"); return false; }
        core::hint::spin_loop();
    }
    // Wait for CNR=0
    let dl = deadline_cycles(100_000);
    while rd32(op, OP_USBSTS) & (1 << 11) != 0 {
        if past(dl) { crate::serial_println!("[XHCI] CNR timeout"); return false; }
        core::hint::spin_loop();
    }
    true
}

// ── Command ring helpers ─────────────────────────────────────────────────────

struct CmdState { enq: usize, cycle: u32 }

unsafe fn cmd_push(ring: *mut CmdRing, st: &mut CmdState, dw: [u32; 4]) {
    let r = &mut *ring;
    let c = st.cycle;
    r.t[st.enq].dw = [dw[0], dw[1], dw[2], (dw[3] & !1) | c];
    st.enq += 1;
    if st.enq == CMD_N - 1 {
        // Update Link TRB cycle and wrap
        r.t[CMD_N - 1].dw[3] = (r.t[CMD_N - 1].dw[3] & !1) | c;
        st.enq = 0;
        st.cycle = c ^ 1;
    }
}

unsafe fn cmd_doorbell(db: usize) { (db as *mut u32).write_volatile(0); }

// ── Event ring consumer ───────────────────────────────────────────────────────

struct EvtState { deq: usize, cycle: u32 }

// Wait for the next event. Returns the TRB dwords if an event arrives within timeout_us.
unsafe fn evt_wait(ring: *const EvtRing, st: &mut EvtState,
                   rt: usize, evt_phys: usize, timeout_us: u64) -> Option<[u32; 4]> {
    let r = &*ring;
    let dl = deadline_cycles(timeout_us);
    loop {
        let dw = r.t[st.deq].dw;
        if (dw[3] & 1) == st.cycle {
            let result = dw;
            st.deq += 1;
            if st.deq >= EVT_N { st.deq = 0; st.cycle ^= 1; }
            // Update ERDP (clear EHB bit 3 by writing 1 to it)
            let new_phys = (evt_phys + st.deq * 16) as u64 | (1 << 3);
            wr64_lo(rt + 0x20, IR_ERDP, new_phys);
            return Some(result);
        }
        if past(dl) { return None; }
        core::hint::spin_loop();
    }
}

// Wait for a Command Completion Event. Returns (completion_code, slot_id).
unsafe fn wait_cmd(ring: *const EvtRing, es: &mut EvtState,
                   rt: usize, ep: usize, db: usize, timeout_us: u64) -> (u8, u8) {
    cmd_doorbell(db);
    let dl = deadline_cycles(timeout_us);
    loop {
        if let Some(dw) = evt_wait(ring, es, rt, ep, 500) {
            let trb_type = (dw[3] >> 10) & 0x3F;
            if trb_type == TRB_EVT_CMD {
                return (((dw[2] >> 24) & 0xFF) as u8, ((dw[3] >> 24) & 0xFF) as u8);
            }
            // consumed non-cmd event, loop
        }
        if past(dl) { return (0xFF, 0); }
    }
}

// ── EP0 control transfer ──────────────────────────────────────────────────────

struct Ep0State { enq: usize, cycle: u32 }

unsafe fn ep0_push(ring: *mut Ep0Ring, st: &mut Ep0State, dw: [u32; 4]) {
    let r = &mut *ring;
    let c = st.cycle;
    r.t[st.enq].dw = [dw[0], dw[1], dw[2], (dw[3] & !1) | c];
    st.enq += 1;
    if st.enq == EP0_N - 1 {
        r.t[EP0_N - 1].dw[3] = (r.t[EP0_N - 1].dw[3] & !1) | c;
        st.enq = 0;
        st.cycle = c ^ 1;
    }
}

// Ring the EP0 doorbell for a given slot (DB Target = 1 = EP0)
unsafe fn ep0_doorbell(db: usize, slot: usize) {
    ((db + slot * 4) as *mut u32).write_volatile(1);
}

// Wait for a Transfer Event from EP0. Returns completion code.
unsafe fn ep0_wait_xfer(evt: *const EvtRing, es: &mut EvtState,
                        rt: usize, ep: usize, slot: usize,
                        db: usize, timeout_us: u64) -> u8 {
    ep0_doorbell(db, slot);
    let dl = deadline_cycles(timeout_us);
    loop {
        if let Some(dw) = evt_wait(evt, es, rt, ep, 500) {
            let trb_type = (dw[3] >> 10) & 0x3F;
            if trb_type == TRB_EVT_TRANSFER {
                let s = ((dw[3] >> 24) & 0xFF) as usize;
                if s == slot {
                    return ((dw[2] >> 24) & 0xFF) as u8;
                }
            }
        }
        if past(dl) { return 0xFF; }
    }
}

// Issue a USB Control IN transfer (Setup + Data IN + Status OUT).
// Returns bytes transferred on success.
unsafe fn ctrl_in(
    ep0: *mut Ep0Ring, es: &mut Ep0State,
    evt: *const EvtRing, evs: &mut EvtState,
    rt: usize, evt_phys: usize,
    db: usize, slot: usize,
    setup: [u8; 8], buf: *mut u8, len: u16,
) -> bool {
    let po = PHYS_OFF.load(Ordering::Relaxed);
    let buf_phys = buf as usize - po;

    // Setup Stage TRB: immediate data, TRT=3 (IN data stage)
    let setup_lo = u32::from_le_bytes([setup[0], setup[1], setup[2], setup[3]]);
    let setup_hi = u32::from_le_bytes([setup[4], setup[5], setup[6], setup[7]]);
    ep0_push(ep0, es, [setup_lo, setup_hi, 8, (TRB_SETUP << 10) | (1 << 6) | (3 << 16)]);

    // Data Stage TRB: IN direction
    ep0_push(ep0, es, [buf_phys as u32, (buf_phys >> 32) as u32,
                       len as u32, (TRB_DATA << 10) | (1 << 16)]);

    // Status Stage TRB: OUT direction (opposite of data), IOC=1
    ep0_push(ep0, es, [0, 0, 0, (TRB_STATUS << 10) | (1 << 5)]);

    let cc = ep0_wait_xfer(evt, evs, rt, evt_phys, slot, db, 500_000);
    cc == CC_SUCCESS || cc == CC_SHORT_PKT
}

// Issue a USB Control OUT transfer with no data stage (Setup + Status IN).
unsafe fn ctrl_out(
    ep0: *mut Ep0Ring, es: &mut Ep0State,
    evt: *const EvtRing, evs: &mut EvtState,
    rt: usize, evt_phys: usize,
    db: usize, slot: usize,
    setup: [u8; 8],
) -> bool {
    ep0_push(ep0, es, [
        u32::from_le_bytes([setup[0], setup[1], setup[2], setup[3]]),
        u32::from_le_bytes([setup[4], setup[5], setup[6], setup[7]]),
        8,
        (TRB_SETUP << 10) | (1 << 6), // IDT=1, TRT=0 (no data stage)
    ]);
    // Status Stage TRB: IN direction, IOC=1
    ep0_push(ep0, es, [0, 0, 0, (TRB_STATUS << 10) | (1 << 16) | (1 << 5)]);

    let cc = ep0_wait_xfer(evt, evs, rt, evt_phys, slot, db, 500_000);
    cc == CC_SUCCESS || cc == CC_SHORT_PKT
}

// ── USB descriptor parsing ────────────────────────────────────────────────────

// Returns (bConfigurationValue, iface_number, ep_addr, max_packet, interval)
fn find_hid_kbd_ep(cfg: &[u8]) -> Option<(u8, u8, u8, u16, u8)> {
    if cfg.len() < 9 { return None; }
    let cfg_val = cfg[5];
    let mut off = 0usize;
    let mut in_hid_kbd = false;
    let mut iface = 0u8;

    while off + 2 <= cfg.len() {
        let blen  = cfg[off] as usize;
        let btype = cfg[off + 1];
        if blen < 2 || off + blen > cfg.len() { break; }

        if btype == 0x04 && blen >= 9 {
            // Interface descriptor
            in_hid_kbd = cfg[off+5] == 0x03 && cfg[off+6] == 0x01 && cfg[off+7] == 0x01;
            if in_hid_kbd { iface = cfg[off+2]; }
        } else if btype == 0x05 && blen >= 7 && in_hid_kbd {
            // Endpoint descriptor
            let addr  = cfg[off+2];
            let attr  = cfg[off+3];
            let mpkt  = (cfg[off+4] as u16) | ((cfg[off+5] as u16) << 8);
            let ival  = cfg[off+6];
            if addr & 0x80 != 0 && attr & 0x03 == 0x03 { // IN + Interrupt
                return Some((cfg_val, iface, addr, mpkt & 0x7FF, ival));
            }
        }
        off += blen;
    }
    None
}

// ── Port reset ────────────────────────────────────────────────────────────────

unsafe fn port_reset(op: usize, port1: usize) -> bool {
    let base = op + OP_PORTSC_BASE + 0x10 * (port1 - 1);
    // Power on if not already
    if rd32(base, 0) & PORTSC_PP == 0 { wr32(base, 0, PORTSC_PP); }
    // Issue reset (preserve PP, clear change bits)
    let sc = rd32(base, 0) & !PORTSC_CHANGE_BITS;
    wr32(base, 0, (sc | PORTSC_PR) & !PORTSC_PED);
    let dl = deadline_cycles(500_000);
    loop {
        let sc = rd32(base, 0);
        if sc & PORTSC_PRC != 0 {
            // Clear PRC and other change bits
            wr32(base, 0, (sc & !PORTSC_CHANGE_BITS) | PORTSC_PRC);
            return sc & PORTSC_PED != 0; // return whether port enabled after reset
        }
        if past(dl) { crate::serial_println!("[XHCI] port {} reset timeout", port1); return false; }
        core::hint::spin_loop();
    }
}

// ── Input context builders ─────────────────────────────────────────────────────

unsafe fn setup_addr_input_ctx(ictx: *mut InCtx, ep0_ring_phys: usize, port_speed: u8, port1: usize) {
    let c = &mut (*ictx).e;
    // Input Control Context (index 0): add slot (A0) and EP0 (A1)
    c[0][0] = 0;       // drop flags
    c[0][1] = 0b11;    // add A0 (slot) + A1 (EP0)
    // Slot Context (index 1): DW0, DW1
    let ctx_entries: u32 = 1; // only EP0 for now
    c[1][0] = (ctx_entries << 27) | ((port_speed as u32) << 20);
    c[1][1] = (port1 as u32) << 16; // root hub port number
    // EP0 Context (index 2): control endpoint
    // DW1: CErr=3, EPType=4 (Control), MaxPacketSize depends on speed
    let mps: u32 = match port_speed {
        4 | 5 => 512, // SuperSpeed / SuperSpeedPlus
        3     => 64,  // High Speed
        _     => 8,   // Full/Low Speed
    };
    c[2][1] = (3 << 1) | (4 << 3) | (mps << 16);
    // DW2-3: TR Dequeue Pointer
    c[2][2] = (ep0_ring_phys as u32 & !0xF) | 1; // DCS=1
    c[2][3] = (ep0_ring_phys >> 32) as u32;
    // DW4: average TRB length
    c[2][4] = 8;
}

unsafe fn setup_cfg_input_ctx(
    ictx: *mut InCtx,
    ep_ctx_idx: usize,   // XHCI EP context index (e.g. 3 for EP1 IN)
    int_ring_phys: usize,
    max_pkt: u16,
    interval: u8,
) {
    let c = &mut (*ictx).e;
    // Keep EP0 context from Address Device; add interrupt EP
    c[0][0] = 0;
    c[0][1] = (1 << 0) | (1 << ep_ctx_idx as u32); // A0 + A<ep_ctx_idx>
    // Update slot context entries count
    c[1][0] = (c[1][0] & !(0x1F << 27)) | ((ep_ctx_idx as u32) << 27);
    // EP context for the interrupt IN endpoint
    let ep_off = ep_ctx_idx + 1; // +1 because c[0] = input ctrl ctx, c[1] = slot ctx
    c[ep_off][0] = (interval as u32) << 16; // polling interval
    c[ep_off][1] = (3 << 1) | (7 << 3) | ((max_pkt as u32) << 16); // CErr=3, EPType=7 (Int IN)
    c[ep_off][2] = (int_ring_phys as u32 & !0xF) | 1; // TR Deq Lo | DCS
    c[ep_off][3] = (int_ring_phys >> 32) as u32;
    c[ep_off][4] = max_pkt as u32; // average TRB length
}

// ── Interrupt EP ring arming (used from both init and take_hid_report) ────────

unsafe fn arm_kbd_ep_raw(
    tr: *mut IntRing, enq: usize, cycle: u32,
    buf_phys: usize,
) -> (usize, u32) /* (new_enq, new_cycle) */ {
    let r = &mut *tr;
    let link_idx = INT_N - 1;
    // Normal TRB: 8 bytes, IOC=1
    r.t[enq].dw = [
        buf_phys as u32,
        (buf_phys >> 32) as u32,
        8,
        (TRB_NORMAL << 10) | (1 << 5) | cycle,
    ];
    // Update Link TRB cycle to match so consumer can wrap correctly
    r.t[link_idx].dw[3] = (r.t[link_idx].dw[3] & !1) | cycle;

    let new_enq = enq + 1;
    if new_enq == link_idx {
        (0, cycle ^ 1)
    } else {
        (new_enq, cycle)
    }
}

// ── Physical offset global ────────────────────────────────────────────────────

static PHYS_OFF: AtomicUsize = AtomicUsize::new(0);

// ── Public init ───────────────────────────────────────────────────────────────

pub fn init(phys_off: usize) {
    PHYS_OFF.store(phys_off, Ordering::Relaxed);

    let cap = match crate::pci::find_xhci_bar0(phys_off) {
        Some(v) => v,
        None    => { crate::serial_println!("[XHCI] no controller — keyboard disabled"); return; }
    };

    unsafe { init_unsafe(cap, phys_off) };
}

unsafe fn init_unsafe(cap: usize, po: usize) {
    bios_handoff(cap);

    let caplength   = rd8(cap, CAP_CAPLENGTH) as usize;
    let op          = cap + caplength;
    let hcsparams1  = rd32(cap, CAP_HCSPARAMS1);
    let hcsparams2  = rd32(cap, CAP_HCSPARAMS2);
    let hccparams1  = rd32(cap, CAP_HCCPARAMS1);
    let dboff       = rd32(cap, CAP_DBOFF) as usize;
    let rtsoff      = rd32(cap, CAP_RTSOFF) as usize;
    let db          = cap + dboff;
    let rt          = cap + rtsoff;
    let max_slots   = (hcsparams1 & 0xFF) as usize;
    let max_ports   = ((hcsparams1 >> 24) & 0xFF) as usize;
    let csz         = (hccparams1 >> 2) & 1; // context size: 0=32B, 1=64B

    crate::serial_println!(
        "[XHCI] cap={:#010x} op={:#010x} db={:#010x} rt={:#010x}",
        cap, op, db, rt
    );
    crate::serial_println!(
        "[XHCI] MaxSlots={} MaxPorts={} CSZ={}", max_slots, max_ports, csz
    );

    if csz != 0 {
        crate::serial_println!("[XHCI] 64-byte contexts not supported yet");
        return;
    }

    if !xhci_reset(op) { return; }

    // ── Allocate DMA structures ───────────────────────────────────────────────
    let dcbaa:    *mut Dcbaa   = alloc_zeroed::<Dcbaa>();
    let cmd_ring: *mut CmdRing = alloc_zeroed::<CmdRing>();
    let evt_ring: *mut EvtRing = alloc_zeroed::<EvtRing>();
    let erst:     *mut Erst    = alloc_zeroed::<Erst>();

    let dcbaa_phys    = phys(dcbaa    as usize, po);
    let cmd_ring_phys = phys(cmd_ring as usize, po);
    let evt_ring_phys = phys(evt_ring as usize, po);
    let erst_phys     = phys(erst     as usize, po);

    // ── Scratchpad ────────────────────────────────────────────────────────────
    let sp_hi  = (hcsparams2 >> 27) & 0x1F;
    let sp_lo  = (hcsparams2 >>  8) & 0x1F;
    let sp_cnt = ((sp_hi << 5) | sp_lo) as usize;
    crate::serial_println!("[XHCI] scratchpad pages needed: {}", sp_cnt);
    if sp_cnt > 0 {
        let sp_arr: *mut SpArr = alloc_zeroed::<SpArr>();
        let sp_arr_phys = phys(sp_arr as usize, po);
        for i in 0..sp_cnt.min(32) {
            let pg: *mut SpPage = alloc_zeroed::<SpPage>();
            (*sp_arr).e[i] = phys(pg as usize, po) as u64;
        }
        (*dcbaa).e[0] = sp_arr_phys as u64;
    }

    // ── Command ring: last TRB = Link TRB ────────────────────────────────────
    {
        let r = &mut *cmd_ring;
        r.t[CMD_N - 1].dw[0] = cmd_ring_phys as u32;
        r.t[CMD_N - 1].dw[1] = (cmd_ring_phys >> 32) as u32;
        r.t[CMD_N - 1].dw[3] = (TRB_LINK << 10) | (1 << 1) | 1; // TC=1, cycle=1
    }

    // ── Controller configuration ──────────────────────────────────────────────
    wr32(op, OP_CONFIG,  2_u32.min(max_slots as u32)); // max 2 device slots
    wr64_lo(op, OP_DCBAAP, dcbaa_phys as u64);
    wr64_hi(op, OP_CRCR, cmd_ring_phys as u64 | 1); // RCS=1

    // ── Event ring (interrupter 0) ────────────────────────────────────────────
    (*erst).e[0].addr = evt_ring_phys as u64;
    (*erst).e[0].size = EVT_N as u16;

    let ir0 = rt + 0x20; // interrupter 0 register set base
    wr32(ir0, IR_ERSTSZ, 1);
    wr64_lo(ir0, IR_ERDP, evt_ring_phys as u64);
    wr64_lo(ir0, IR_ERSTBA, erst_phys as u64);

    // ── Start the controller ──────────────────────────────────────────────────
    wr32(op, OP_USBCMD, (1 << 2) | 1); // INTE=1 (for IMAN), RUN=1
    let dl = deadline_cycles(100_000);
    while rd32(op, OP_USBSTS) & 1 != 0 { // wait for HCH=0
        if past(dl) { crate::serial_println!("[XHCI] start timeout"); return; }
        core::hint::spin_loop();
    }
    crate::serial_println!("[XHCI] controller running");

    // ── State for command + event ring operations ─────────────────────────────
    let mut cs = CmdState { enq: 0, cycle: 1 };
    let mut es = EvtState { deq: 0, cycle: 1 };

    // ── Port enumeration ──────────────────────────────────────────────────────
    for port1 in 1..=max_ports {
        let portsc_base = op + OP_PORTSC_BASE + 0x10 * (port1 - 1);
        let sc = rd32(portsc_base, 0);
        if sc & PORTSC_CCS == 0 { continue; } // no device

        let speed = ((sc >> 10) & 0xF) as u8; // 1=FS,2=LS,3=HS,4=SS,5=SSP
        crate::serial_println!("[XHCI] port {} connected, speed={}", port1, speed);

        if !port_reset(op, port1) {
            crate::serial_println!("[XHCI] port {} reset failed or not enabled", port1);
            // Some ports still work without PED after reset (USB 3.0); try anyway.
        }

        // Enable Slot command
        cmd_push(cmd_ring, &mut cs, [0, 0, 0, TRB_ENABLE_SLOT << 10]);
        let (cc, slot) = wait_cmd(evt_ring, &mut es, rt, evt_ring_phys, db, 1_000_000);
        if cc != CC_SUCCESS || slot == 0 {
            crate::serial_println!("[XHCI] port {} Enable Slot failed cc={}", port1, cc);
            continue;
        }
        crate::serial_println!("[XHCI] port {} slot={}", port1, slot);

        // Allocate device contexts
        let out_ctx: *mut OutCtx = alloc_zeroed::<OutCtx>();
        let in_ctx:  *mut InCtx  = alloc_zeroed::<InCtx>();
        let ep0_ring: *mut Ep0Ring = alloc_zeroed::<Ep0Ring>();
        let desc_buf: *mut [u8; 512] = alloc_zeroed::<[u8; 512]>();

        let out_ctx_phys  = phys(out_ctx  as usize, po);
        let in_ctx_phys   = phys(in_ctx   as usize, po);
        let ep0_ring_phys = phys(ep0_ring as usize, po);
        let _desc_phys    = phys(desc_buf as usize, po);

        // Point DCBAA[slot] at the output device context
        (*dcbaa).e[slot as usize] = out_ctx_phys as u64;

        // Set up EP0 ring: last TRB = Link TRB
        {
            let r = &mut *ep0_ring;
            r.t[EP0_N - 1].dw[0] = ep0_ring_phys as u32;
            r.t[EP0_N - 1].dw[1] = (ep0_ring_phys >> 32) as u32;
            r.t[EP0_N - 1].dw[3] = (TRB_LINK << 10) | (1 << 1) | 1;
        }

        // Configure input context for Address Device
        setup_addr_input_ctx(in_ctx, ep0_ring_phys, speed, port1);

        // Address Device command (BSR=0 → sends SET_ADDRESS)
        cmd_push(cmd_ring, &mut cs, [
            in_ctx_phys as u32,
            (in_ctx_phys >> 32) as u32,
            0,
            (TRB_ADDR_DEVICE << 10) | ((slot as u32) << 24),
        ]);
        let (cc, _) = wait_cmd(evt_ring, &mut es, rt, evt_ring_phys, db, 2_000_000);
        if cc != CC_SUCCESS {
            crate::serial_println!("[XHCI] slot {} Address Device failed cc={}", slot, cc);
            continue;
        }
        crate::serial_println!("[XHCI] slot {} addressed", slot);

        let mut ep0s = Ep0State { enq: 0, cycle: 1 };

        // GET_DESCRIPTOR(Device, 18 bytes)
        let dev_desc_ok = ctrl_in(
            ep0_ring, &mut ep0s, evt_ring, &mut es,
            rt, evt_ring_phys, db, slot as usize,
            [0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 18, 0], // setup pkt
            desc_buf as *mut u8, 18,
        );
        if !dev_desc_ok {
            crate::serial_println!("[XHCI] slot {} GET_DESCRIPTOR(Device) failed", slot);
            continue;
        }
        crate::serial_println!(
            "[XHCI] slot {} dev: class={:#04x}/{:#04x}/{:#04x} VID={:#06x} PID={:#06x}",
            slot,
            (*desc_buf)[4], (*desc_buf)[5], (*desc_buf)[6],
            ((*desc_buf)[8] as u16) | (((*desc_buf)[9] as u16) << 8),
            ((*desc_buf)[10] as u16) | (((*desc_buf)[11] as u16) << 8),
        );

        // GET_DESCRIPTOR(Config, 9 bytes) to get wTotalLength
        let cfg_ok = ctrl_in(
            ep0_ring, &mut ep0s, evt_ring, &mut es,
            rt, evt_ring_phys, db, slot as usize,
            [0x80, 0x06, 0x00, 0x02, 0x00, 0x00, 9, 0],
            desc_buf as *mut u8, 9,
        );
        if !cfg_ok { continue; }
        let total_len = ((*desc_buf)[2] as u16) | (((*desc_buf)[3] as u16) << 8);
        let fetch_len = total_len.min(512);

        // GET_DESCRIPTOR(Config, full)
        ctrl_in(
            ep0_ring, &mut ep0s, evt_ring, &mut es,
            rt, evt_ring_phys, db, slot as usize,
            [0x80, 0x06, 0x00, 0x02, 0x00, 0x00,
             (fetch_len & 0xFF) as u8, (fetch_len >> 8) as u8],
            desc_buf as *mut u8, fetch_len,
        );

        let cfg_slice = core::slice::from_raw_parts(desc_buf as *const u8, fetch_len as usize);
        let ep_info = find_hid_kbd_ep(cfg_slice);
        if ep_info.is_none() {
            crate::serial_println!("[XHCI] slot {} not a HID boot keyboard", slot);
            continue;
        }
        let (cfg_val, iface, ep_addr, max_pkt, ival) = ep_info.unwrap();
        let ep_num = ep_addr & 0x0F;
        let ep_in  = (ep_addr >> 7) & 1;
        let ep_ctx_idx = (ep_num * 2 + ep_in) as usize; // XHCI context index
        crate::serial_println!(
            "[XHCI] HID kbd: cfg={} iface={} ep={:#04x} mps={} ival={} ctx_idx={}",
            cfg_val, iface, ep_addr, max_pkt, ival, ep_ctx_idx
        );

        // SET_CONFIGURATION
        let ok = ctrl_out(
            ep0_ring, &mut ep0s, evt_ring, &mut es,
            rt, evt_ring_phys, db, slot as usize,
            [0x00, 0x09, cfg_val, 0, 0, 0, 0, 0],
        );
        if !ok { crate::serial_println!("[XHCI] SET_CONFIGURATION failed"); }

        // SET_PROTOCOL(0) — switch to Boot Protocol
        let ok = ctrl_out(
            ep0_ring, &mut ep0s, evt_ring, &mut es,
            rt, evt_ring_phys, db, slot as usize,
            [0x21, 0x0B, 0x00, 0x00, iface, 0, 0, 0],
        );
        if !ok { crate::serial_println!("[XHCI] SET_PROTOCOL failed (may be ok)"); }

        // Allocate interrupt endpoint ring and HID report buffer
        let int_ring: *mut IntRing = alloc_zeroed::<IntRing>();
        let hid_buf:  *mut HidBuf  = alloc_zeroed::<HidBuf>();
        let int_ring_phys = phys(int_ring as usize, po);
        let hid_buf_phys  = phys(hid_buf  as usize, po);

        // Set up interrupt ring Link TRB
        {
            let r = &mut *int_ring;
            r.t[INT_N - 1].dw[0] = int_ring_phys as u32;
            r.t[INT_N - 1].dw[1] = (int_ring_phys >> 32) as u32;
            r.t[INT_N - 1].dw[3] = (TRB_LINK << 10) | (1 << 1) | 1;
        }

        // Configure Endpoint command: add interrupt endpoint
        setup_cfg_input_ctx(in_ctx, ep_ctx_idx, int_ring_phys, max_pkt, ival);
        cmd_push(cmd_ring, &mut cs, [
            in_ctx_phys as u32,
            (in_ctx_phys >> 32) as u32,
            0,
            (TRB_CFG_EP << 10) | ((slot as u32) << 24),
        ]);
        let (cc, _) = wait_cmd(evt_ring, &mut es, rt, evt_ring_phys, db, 2_000_000);
        crate::serial_println!("[XHCI] Configure Endpoint cc={}", cc);
        if cc != CC_SUCCESS {
            crate::serial_println!("[XHCI] Configure Endpoint failed");
            continue;
        }

        // Arm the interrupt endpoint with one pending transfer
        let (new_enq, new_cyc) = arm_kbd_ep_raw(int_ring, 0, 1, hid_buf_phys);
        // Ring the interrupt endpoint doorbell
        ((db + (slot as usize) * 4) as *mut u32).write_volatile(ep_ctx_idx as u32);

        // Store polling state
        KBD_SLOT    .store(slot as usize, Ordering::Relaxed);
        KBD_EP_DB   .store(ep_ctx_idx as u32, Ordering::Relaxed);
        KBD_DB_BASE .store(db, Ordering::Relaxed);
        KBD_RT_BASE .store(rt, Ordering::Relaxed);
        KBD_EVT_BASE.store(evt_ring as usize, Ordering::Relaxed);
        KBD_EVT_PHYS.store(evt_ring_phys, Ordering::Relaxed);
        KBD_EVT_DEQ .store(es.deq, Ordering::Relaxed);
        KBD_EVT_CYC .store(es.cycle, Ordering::Relaxed);
        KBD_TR_BASE .store(int_ring as usize, Ordering::Relaxed);
        KBD_TR_PHYS .store(int_ring_phys, Ordering::Relaxed);
        KBD_TR_ENQ  .store(new_enq, Ordering::Relaxed);
        KBD_TR_CYC  .store(new_cyc, Ordering::Relaxed);
        KBD_BUF_VIRT.store(hid_buf as usize, Ordering::Relaxed);
        KBD_BUF_PHYS.store(hid_buf_phys, Ordering::Relaxed);

        KBD_READY.store(true, Ordering::Release);
        crate::serial_println!("[XHCI] keyboard ready on slot {}", slot);
        return; // found our keyboard
    }
    crate::serial_println!("[XHCI] no HID boot keyboard found");
}

// ── Polling (called from timer interrupt handler every 1 ms) ─────────────────

pub fn take_hid_report() -> Option<[u8; 8]> {
    if !KBD_READY.load(Ordering::Acquire) { return None; }

    let evt_base  = KBD_EVT_BASE.load(Ordering::Relaxed);
    let deq       = KBD_EVT_DEQ.load(Ordering::Relaxed);
    let cycle     = KBD_EVT_CYC.load(Ordering::Relaxed);
    let slot      = KBD_SLOT.load(Ordering::Relaxed);

    // Check if the current event ring position has a valid event
    let dw = unsafe { ((evt_base + deq * 16) as *const [u32; 4]).read_volatile() };
    if (dw[3] & 1) != cycle { return None; } // no new event

    // Consume the event
    let trb_type   = (dw[3] >> 10) & 0x3F;
    let evt_slot   = ((dw[3] >> 24) & 0xFF) as usize;
    let evt_cc     = ((dw[2] >> 24) & 0xFF) as u8;

    // Advance event ring dequeue pointer
    let new_deq = if deq + 1 >= EVT_N { 0 } else { deq + 1 };
    let new_cyc = if deq + 1 >= EVT_N { cycle ^ 1 } else { cycle };
    KBD_EVT_DEQ.store(new_deq, Ordering::Relaxed);
    KBD_EVT_CYC.store(new_cyc, Ordering::Relaxed);

    // Update hardware ERDP
    let rt        = KBD_RT_BASE.load(Ordering::Relaxed);
    let evt_phys  = KBD_EVT_PHYS.load(Ordering::Relaxed);
    let new_erdp  = (evt_phys + new_deq * 16) as u64 | (1 << 3); // EHB=1
    unsafe { wr64_lo(rt + 0x20, IR_ERDP, new_erdp); }

    // Only process Transfer Events for our keyboard slot
    if trb_type != TRB_EVT_TRANSFER || evt_slot != slot { return None; }
    if evt_cc != CC_SUCCESS && evt_cc != CC_SHORT_PKT   { return None; }

    // Read the 8-byte HID report
    let buf = KBD_BUF_VIRT.load(Ordering::Relaxed) as *const u8;
    let mut report = [0u8; 8];
    for i in 0..8 { report[i] = unsafe { buf.add(i).read_volatile() }; }

    // Re-arm the interrupt endpoint
    unsafe {
        let tr   = KBD_TR_BASE.load(Ordering::Relaxed) as *mut IntRing;
        let enq  = KBD_TR_ENQ.load(Ordering::Relaxed);
        let cyc  = KBD_TR_CYC.load(Ordering::Relaxed);
        let bphys = KBD_BUF_PHYS.load(Ordering::Relaxed);
        let (ne, nc) = arm_kbd_ep_raw(tr, enq, cyc, bphys);
        KBD_TR_ENQ.store(ne, Ordering::Relaxed);
        KBD_TR_CYC.store(nc, Ordering::Relaxed);
        let db   = KBD_DB_BASE.load(Ordering::Relaxed);
        let ep   = KBD_EP_DB.load(Ordering::Relaxed);
        ((db + slot * 4) as *mut u32).write_volatile(ep);
    }

    Some(report)
}
