use crate::pic::{inb, outb};
use core::fmt;
use spin::Mutex;

pub static SERIAL1: Mutex<SerialPort> = Mutex::new(SerialPort::new(0x3F8));

pub struct SerialPort {
    base: u16,
}

impl SerialPort {
    pub const fn new(base: u16) -> Self {
        Self { base }
    }

    /// Initialize COM1: 38400 baud, 8N1, FIFOs enabled.
    pub fn init(&mut self) {
        unsafe {
            outb(self.base + 1, 0x00); // disable interrupts
            outb(self.base + 3, 0x80); // enable DLAB
            outb(self.base + 0, 0x03); // divisor low byte  (115200 / 38400 = 3)
            outb(self.base + 1, 0x00); // divisor high byte
            outb(self.base + 3, 0x03); // 8 bits, no parity, 1 stop bit; clears DLAB
            outb(self.base + 2, 0xC7); // enable FIFO, clear, 14-byte threshold
            outb(self.base + 4, 0x0B); // RTS + DTR + aux output 2
        }
    }

    fn write_byte(&mut self, byte: u8) {
        unsafe {
            while inb(self.base + 5) & 0x20 == 0 {} // wait: transmit-hold-register empty
            outb(self.base, byte);
        }
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    SERIAL1.lock().write_fmt(args).unwrap();
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(concat!($fmt, "\n"), $($arg)*));
}
