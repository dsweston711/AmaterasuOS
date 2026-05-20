use spin::Mutex;

const BUF_CAP: usize = 256;

pub static SHELL: Mutex<Shell> = Mutex::new(Shell::new());

pub struct Shell {
    buf: [char; BUF_CAP],
    len: usize,
}

impl Shell {
    pub const fn new() -> Self {
        Self {
            buf: ['\0'; BUF_CAP],
            len: 0,
        }
    }

    pub fn push_char(&mut self, ch: char) {
        match ch {
            '\x08' => self.backspace(),
            '\n'   => self.submit(),
            ch     => {
                if self.len < BUF_CAP {
                    self.buf[self.len] = ch;
                    self.len += 1;
                    crate::print!("{}", ch);
                }
            }
        }
    }

    fn backspace(&mut self) {
        if self.len == 0 {
            return;
        }
        self.len -= 1;
        if let Some(w) = crate::framebuffer::WRITER.lock().as_mut() {
            w.backspace();
        }
    }

    fn submit(&mut self) {
        crate::print!("\n");
        self.dispatch();
        self.len = 0;
        self.print_prompt();
    }

    fn dispatch(&self) {
        let chars = &self.buf[..self.len];

        // Trim leading/trailing whitespace.
        let start = chars.iter().position(|c| !c.is_whitespace()).unwrap_or(self.len);
        let end   = chars.iter().rposition(|c| !c.is_whitespace()).map(|i| i + 1).unwrap_or(start);
        let cmd   = &chars[start..end];

        if cmd.is_empty() {
            return;
        }

        if chars_eq(cmd, "clear") {
            if let Some(w) = crate::framebuffer::WRITER.lock().as_mut() {
                w.clear();
            }
        } else {
            crate::print!("unknown command: ");
            for &ch in cmd {
                crate::print!("{}", ch);
            }
            crate::print!("\n");
        }
    }

    pub fn print_prompt(&self) {
        crate::print!("> ");
    }
}

pub fn prompt() {
    SHELL.lock().print_prompt();
}

fn chars_eq(chars: &[char], s: &str) -> bool {
    chars.len() == s.chars().count() && chars.iter().zip(s.chars()).all(|(a, b)| *a == b)
}
