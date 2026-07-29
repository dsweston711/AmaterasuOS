use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;
use x86_64::structures::idt::InterruptStackFrame;

static EXTENDED: AtomicBool = AtomicBool::new(false);

// US QWERTY scancode set 1 — unshifted make-codes. 0x00 = no mapping.
#[rustfmt::skip]
static MAP: [u8; 58] = [
    0,      // 0x00
    0,      // 0x01 Escape
    b'1',   // 0x02
    b'2',   // 0x03
    b'3',   // 0x04
    b'4',   // 0x05
    b'5',   // 0x06
    b'6',   // 0x07
    b'7',   // 0x08
    b'8',   // 0x09
    b'9',   // 0x0A
    b'0',   // 0x0B
    b'-',   // 0x0C
    b'=',   // 0x0D
    0x08,   // 0x0E Backspace
    b'\t',  // 0x0F Tab
    b'q',   // 0x10
    b'w',   // 0x11
    b'e',   // 0x12
    b'r',   // 0x13
    b't',   // 0x14
    b'y',   // 0x15
    b'u',   // 0x16
    b'i',   // 0x17
    b'o',   // 0x18
    b'p',   // 0x19
    b'[',   // 0x1A
    b']',   // 0x1B
    b'\n',  // 0x1C Enter
    0,      // 0x1D Left Ctrl
    b'a',   // 0x1E
    b's',   // 0x1F
    b'd',   // 0x20
    b'f',   // 0x21
    b'g',   // 0x22
    b'h',   // 0x23
    b'j',   // 0x24
    b'k',   // 0x25
    b'l',   // 0x26
    b';',   // 0x27
    b'\'',  // 0x28
    b'`',   // 0x29
    0,      // 0x2A Left Shift
    b'\\',  // 0x2B
    b'z',   // 0x2C
    b'x',   // 0x2D
    b'c',   // 0x2E
    b'v',   // 0x2F
    b'b',   // 0x30
    b'n',   // 0x31
    b'm',   // 0x32
    b',',   // 0x33
    b'.',   // 0x34
    b'/',   // 0x35
    0,      // 0x36 Right Shift
    b'*',   // 0x37 Keypad *
    0,      // 0x38 Left Alt
    b' ',   // 0x39 Space
];

// Shifted equivalents, parallel to MAP.
#[rustfmt::skip]
static SHIFT_MAP: [u8; 58] = [
    0,      // 0x00
    0,      // 0x01 Escape
    b'!',   // 0x02
    b'@',   // 0x03
    b'#',   // 0x04
    b'$',   // 0x05
    b'%',   // 0x06
    b'^',   // 0x07
    b'&',   // 0x08
    b'*',   // 0x09
    b'(',   // 0x0A
    b')',   // 0x0B
    b'_',   // 0x0C
    b'+',   // 0x0D
    0x08,   // 0x0E Backspace
    b'\t',  // 0x0F Tab
    b'Q',   // 0x10
    b'W',   // 0x11
    b'E',   // 0x12
    b'R',   // 0x13
    b'T',   // 0x14
    b'Y',   // 0x15
    b'U',   // 0x16
    b'I',   // 0x17
    b'O',   // 0x18
    b'P',   // 0x19
    b'{',   // 0x1A
    b'}',   // 0x1B
    b'\n',  // 0x1C Enter
    0,      // 0x1D Left Ctrl
    b'A',   // 0x1E
    b'S',   // 0x1F
    b'D',   // 0x20
    b'F',   // 0x21
    b'G',   // 0x22
    b'H',   // 0x23
    b'J',   // 0x24
    b'K',   // 0x25
    b'L',   // 0x26
    b':',   // 0x27
    b'"',   // 0x28
    b'~',   // 0x29
    0,      // 0x2A Left Shift
    b'|',   // 0x2B
    b'Z',   // 0x2C
    b'X',   // 0x2D
    b'C',   // 0x2E
    b'V',   // 0x2F
    b'B',   // 0x30
    b'N',   // 0x31
    b'M',   // 0x32
    b'<',   // 0x33
    b'>',   // 0x34
    b'?',   // 0x35
    0,      // 0x36 Right Shift
    b'*',   // 0x37 Keypad *
    0,      // 0x38 Left Alt
    b' ',   // 0x39 Space
];

struct Modifiers {
    shift: bool,
    caps_lock: bool,
    ctrl: bool,
}

static MODIFIERS: Mutex<Modifiers> = Mutex::new(Modifiers { shift: false, caps_lock: false, ctrl: false });

// Letter scancodes: Q-P row, A-L row, Z-M row.
fn is_letter(scancode: u8) -> bool {
    matches!(scancode, 0x10..=0x19 | 0x1E..=0x26 | 0x2C..=0x32)
}

pub fn scancode_to_char(scancode: u8, shift: bool, caps_lock: bool) -> Option<char> {
    // Caps lock inverts shift for letters only; symbols always follow shift state.
    let use_shift = if is_letter(scancode) { shift ^ caps_lock } else { shift };
    let map = if use_shift { &SHIFT_MAP } else { &MAP };
    let b = *map.get(scancode as usize)?;
    if b == 0 { None } else { Some(b as char) }
}

// ── Input ring buffer ─────────────────────────────────────────────────────────
//
// SPSC: ISR pushes to head, main-loop drain() pulls from tail.
// Navigation keys are encoded as private control bytes so everything goes
// through one path and the ISR never touches the shell or framebuffer.
//
//   0x02 = cursor_left   (ctrl-B)
//   0x06 = cursor_right  (ctrl-F)
//   0x0E = history_down  (ctrl-N)
//   0x10 = history_up    (ctrl-P)
//   0x01/0x05 = Home/End already handled by push_char as ctrl-A/ctrl-E

const RB_CAP:  usize = 64;
const RB_MASK: usize = RB_CAP - 1;

struct RingBuf { buf: UnsafeCell<[u8; RB_CAP]> }
unsafe impl Sync for RingBuf {}

static RB:      RingBuf    = RingBuf { buf: UnsafeCell::new([0u8; RB_CAP]) };
static RB_HEAD: AtomicUsize = AtomicUsize::new(0); // producer index (ISR)
static RB_TAIL: AtomicUsize = AtomicUsize::new(0); // consumer index (main loop)

fn rb_push(b: u8) {
    let head = RB_HEAD.load(Ordering::Relaxed);
    let next = (head + 1) & RB_MASK;
    if next == RB_TAIL.load(Ordering::Acquire) { return; } // full — drop
    unsafe { (*RB.buf.get())[head] = b; }
    RB_HEAD.store(next, Ordering::Release);
}

// Announces the first XHCI HID report on the framebuffer, once. Diagnostic
// only — lets real-hardware testing without a serial adapter confirm the
// interrupt endpoint is actually delivering reports, not just that init()
// completed. Safe to check here: drain() runs at PASSIVE_LEVEL, never from
// interrupt context (ADR-013).
static XHCI_REPORT_ANNOUNCED: AtomicBool = AtomicBool::new(false);

/// Drain all buffered bytes into the shell. Call from the main `hlt` loop after
/// every interrupt wakeup — never from interrupt context.
pub fn drain() {
    if hal::xhci::reports_received() > 0
        && !XHCI_REPORT_ANNOUNCED.swap(true, Ordering::Relaxed)
    {
        println!("[dbg] first XHCI HID report received");
    }
    loop {
        let tail = RB_TAIL.load(Ordering::Relaxed);
        if tail == RB_HEAD.load(Ordering::Acquire) { break; }
        let b = unsafe { (*RB.buf.get())[tail] };
        RB_TAIL.store((tail + 1) & RB_MASK, Ordering::Release);
        crate::shell::SHELL.lock().push_char(b as char);
    }
}

// ── PS/2 scancode processing ──────────────────────────────────────────────────

/// Process a raw PS/2 scancode. Called from the keyboard IRQ handler and
/// from the timer-based polling fallback.
pub fn process_scancode(scancode: u8) {
    // 0xE0 prefix marks an extended two-byte sequence.
    if scancode == 0xE0 {
        EXTENDED.store(true, Ordering::Relaxed);
        return;
    }

    let extended = EXTENDED.swap(false, Ordering::Relaxed);
    if extended {
        match scancode {
            0x48 => rb_push(0x10), // up    → history_up
            0x50 => rb_push(0x0E), // down  → history_down
            0x4B => rb_push(0x02), // left  → cursor_left
            0x4D => rb_push(0x06), // right → cursor_right
            0x47 => rb_push(0x01), // home  → cursor_to_start
            0x4F => rb_push(0x05), // end   → cursor_to_end
            _ => {}
        }
        return;
    }

    match scancode {
        0x2A | 0x36 => { MODIFIERS.lock().shift = true;  return; }
        0xAA | 0xB6 => { MODIFIERS.lock().shift = false; return; }
        0x1D        => { MODIFIERS.lock().ctrl  = true;  return; }
        0x9D        => { MODIFIERS.lock().ctrl  = false; return; }
        0x3A        => { let mut m = MODIFIERS.lock(); m.caps_lock ^= true; return; }
        sc if sc & 0x80 != 0 => return,
        _ => {}
    }

    let (ch, ctrl) = {
        let m = MODIFIERS.lock();
        (scancode_to_char(scancode, m.shift, m.caps_lock), m.ctrl)
    };

    if let Some(ch) = ch {
        let effective = if ctrl && ch.is_ascii_alphabetic() {
            char::from(ch.to_ascii_lowercase() as u8 & 0x1F)
        } else {
            ch
        };
        rb_push(effective as u8);
    }
}

// ── HID Boot Protocol report processing ──────────────────────────────────────

// HID Usage ID → ASCII. Returns 0 for unrecognised keys.
// caps_lock inverts shift for letters (0x04–0x1D) only; symbols follow shift.
fn hid_to_ascii(usage: u8, shift: bool, caps_lock: bool) -> u8 {
    match usage {
        0x04..=0x1D => {
            let base = b'a' + (usage - 0x04);
            if shift ^ caps_lock { base - 32 } else { base }
        }
        0x1E => if shift { b'!' } else { b'1' },
        0x1F => if shift { b'@' } else { b'2' },
        0x20 => if shift { b'#' } else { b'3' },
        0x21 => if shift { b'$' } else { b'4' },
        0x22 => if shift { b'%' } else { b'5' },
        0x23 => if shift { b'^' } else { b'6' },
        0x24 => if shift { b'&' } else { b'7' },
        0x25 => if shift { b'*' } else { b'8' },
        0x26 => if shift { b'(' } else { b'9' },
        0x27 => if shift { b')' } else { b'0' },
        0x28 => b'\n',
        0x2A => 0x08, // Backspace
        0x2B => b'\t',
        0x2C => b' ',
        0x2D => if shift { b'_' } else { b'-' },
        0x2E => if shift { b'+' } else { b'=' },
        0x2F => if shift { b'{' } else { b'[' },
        0x30 => if shift { b'}' } else { b']' },
        0x31 => if shift { b'|' } else { b'\\' },
        0x33 => if shift { b':' } else { b';' },
        0x34 => if shift { b'"' } else { b'\'' },
        0x35 => if shift { b'~' } else { b'`' },
        0x36 => if shift { b'<' } else { b',' },
        0x37 => if shift { b'>' } else { b'.' },
        0x38 => if shift { b'?' } else { b'/' },
        _ => 0,
    }
}

static HID_CAPS_LOCK: AtomicBool  = AtomicBool::new(false);
static HID_PREV:      Mutex<[u8; 6]> = Mutex::new([0u8; 6]);

/// Process one 8-byte HID Boot Protocol keyboard report.
/// byte 0 = modifiers, byte 1 = reserved, bytes 2-7 = keycodes (HID usage IDs).
pub fn process_hid_report(report: [u8; 8]) {
    let mods  = report[0];
    let shift = mods & 0x22 != 0; // bits 1 (L-Shift) and 5 (R-Shift)
    let ctrl  = mods & 0x11 != 0; // bits 0 (L-Ctrl)  and 4 (R-Ctrl)
    let caps  = HID_CAPS_LOCK.load(Ordering::Relaxed);

    let mut prev = HID_PREV.lock();

    for &usage in &report[2..8] {
        if usage == 0 || usage == 0x01 { continue; } // 0x00 = none, 0x01 = rollover
        if prev.contains(&usage) { continue; }        // still held from previous report

        match usage {
            0x39 => { HID_CAPS_LOCK.store(!caps, Ordering::Relaxed); continue; }
            0x52 => { rb_push(0x10); continue; } // up    → history_up
            0x51 => { rb_push(0x0E); continue; } // down  → history_down
            0x50 => { rb_push(0x02); continue; } // left  → cursor_left
            0x4F => { rb_push(0x06); continue; } // right → cursor_right
            0x4A => { rb_push(0x01); continue; } // home  → cursor_to_start
            0x4D => { rb_push(0x05); continue; } // end   → cursor_to_end
            _ => {}
        }

        let b = hid_to_ascii(usage, shift, caps);
        if b != 0 {
            let ch = b as char;
            let effective = if ctrl && ch.is_ascii_alphabetic() {
                char::from(ch.to_ascii_lowercase() as u8 & 0x1F)
            } else {
                ch
            };
            rb_push(effective as u8);
        }
    }

    prev.copy_from_slice(&report[2..8]);
}

pub extern "x86-interrupt" fn keyboard_handler(_frame: InterruptStackFrame) {
    unsafe {
        let scancode = hal::pic::inb(0x60);
        hal::apic::end_of_interrupt();
        process_scancode(scancode);
    }
}
