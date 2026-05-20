use x86_64::structures::idt::InterruptStackFrame;

// US QWERTY scancode set 1 make-code -> ASCII byte. 0x00 = no mapping.
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

pub fn scancode_to_char(scancode: u8) -> Option<char> {
    let b = *MAP.get(scancode as usize)?;
    if b == 0 { None } else { Some(b as char) }
}

pub extern "x86-interrupt" fn keyboard_handler(_frame: InterruptStackFrame) {
    unsafe {
        let scancode = crate::pic::inb(0x60);
        crate::pic::end_of_interrupt(1);

        // Ignore key-release events (bit 7 set)
        if scancode & 0x80 != 0 {
            return;
        }

        if let Some(ch) = scancode_to_char(scancode) {
            crate::print!("{}", ch);
        }
    }
}