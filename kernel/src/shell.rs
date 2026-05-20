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
        let _line = &self.buf[..self.len];
        self.len = 0;
        crate::print!("\n");
        // TODO(#17): dispatch(_line)
        self.print_prompt();
    }

    pub fn print_prompt(&self) {
        crate::print!("> ");
    }
}

pub fn prompt() {
    SHELL.lock().print_prompt();
}
