use alloc::string::String;
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
        } else if name_eq(cmd, "ls") {
            self.cmd_ls(cmd_arg(cmd));
        } else if name_eq(cmd, "cat") {
            match cmd_arg(cmd) {
                Some(path) => self.cmd_cat(&path),
                None => crate::println!("usage: cat <path>"),
            }
        } else if name_eq(cmd, "stat") {
            match cmd_arg(cmd) {
                Some(path) => self.cmd_stat(&path),
                None => crate::println!("usage: stat <path>"),
            }
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

    fn cmd_ls(&self, path: Option<String>) {
        let path_str: &str = path.as_deref().unwrap_or("/");
        let is_root = path_str.split('/').filter(|s| !s.is_empty()).next().is_none();

        if is_root {
            match crate::vfs::with_root(|n| n.readdir()) {
                None => crate::println!("ls: no filesystem mounted"),
                Some(names) if names.is_empty() => crate::println!("(empty)"),
                Some(names) => { for name in &names { crate::println!("{}", name); } }
            }
            return;
        }

        match crate::vfs::lookup(path_str) {
            None => crate::println!("ls: not found: {}", path_str),
            Some(node) => match node.kind() {
                crate::vfs::NodeKind::Dir => {
                    let names = node.readdir();
                    if names.is_empty() {
                        crate::println!("(empty)");
                    } else {
                        for name in &names { crate::println!("{}", name); }
                    }
                }
                crate::vfs::NodeKind::File => {
                    let leaf = path_str.split('/').filter(|s| !s.is_empty()).last().unwrap_or(path_str);
                    crate::println!("{}", leaf);
                }
            },
        }
    }

    fn cmd_cat(&self, path: &str) {
        match crate::vfs::lookup(path) {
            None => crate::println!("cat: not found: {}", path),
            Some(node) => match node.kind() {
                crate::vfs::NodeKind::Dir => crate::println!("cat: {}: is a directory", path),
                crate::vfs::NodeKind::File => {
                    let size = node.size();
                    if size == 0 { return; }
                    let mut buf: Vec<u8> = Vec::new();
                    buf.resize(size, 0u8);
                    let n = node.read(&mut buf, 0);
                    match core::str::from_utf8(&buf[..n]) {
                        Ok(s) => {
                            crate::print!("{}", s);
                            if !s.ends_with('\n') { crate::print!("\n"); }
                        }
                        Err(_) => crate::println!("cat: {}: binary file ({} bytes)", path, n),
                    }
                }
            },
        }
    }

    fn cmd_stat(&self, path: &str) {
        match crate::vfs::lookup(path) {
            None => crate::println!("stat: not found: {}", path),
            Some(node) => {
                let kind_str = match node.kind() {
                    crate::vfs::NodeKind::File => "file",
                    crate::vfs::NodeKind::Dir  => "directory",
                };
                crate::println!("path: {}", path);
                crate::println!("type: {}", kind_str);
                crate::println!("size: {} bytes", node.size());
            }
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

/// Match the first whitespace-delimited word of `chars` against `s`.
fn name_eq(chars: &[char], s: &str) -> bool {
    let end = chars.iter().position(|c| c.is_whitespace()).unwrap_or(chars.len());
    chars_eq(&chars[..end], s)
}

/// Extract the argument after the first word, trimmed of whitespace. Returns `None` if absent.
fn cmd_arg(chars: &[char]) -> Option<String> {
    let sp = chars.iter().position(|c| c.is_whitespace())?;
    let rest = &chars[sp + 1..];
    let start = rest.iter().position(|c| !c.is_whitespace())?;
    let trimmed = &rest[start..];
    let end = trimmed.iter().rposition(|c| !c.is_whitespace()).map(|i| i + 1).unwrap_or(trimmed.len());
    Some(trimmed[..end].iter().collect())
}
