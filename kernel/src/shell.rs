use alloc::vec::Vec;
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
        } else if chars_eq(cmd, "heap") {
            self.cmd_heap();
        } else if chars_eq(cmd, "uptime") {
            self.cmd_uptime();
        } else {
            crate::print!("unknown command: ");
            for &ch in cmd {
                crate::print!("{}", ch);
            }
            crate::print!("\n");
        }
    }

    fn cmd_uptime(&self) {
        let ms = crate::timer::uptime_ms();
        crate::println!("Uptime: {}s {}ms", ms / 1000, ms % 1000);
    }

    fn cmd_heap(&self) {
        let s = crate::allocator::stats();
        crate::println!("heap start : {:#012x}", s.heap_start);
        crate::println!("heap size  : {} MB",    s.heap_size / (1024 * 1024));
        crate::println!("bump used  : {} / {} KB", s.bump_used / 1024, s.bump_capacity / 1024);
        crate::println!("slabs:");
        for i in 0..6 {
            let used = s.slab_total[i] - s.slab_free[i];
            crate::println!("  {:>3}B  {}/{}", s.slab_sizes[i], used, s.slab_total[i]);
        }
        // Prove alloc works end-to-end with a live heap allocation.
        let mut probe: Vec<u32> = Vec::new();
        probe.push(0xDEAD_BEEF);
        probe.push(0xCAFE_BABE);
        crate::println!("alloc probe: Vec({}) OK", probe.len());
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
