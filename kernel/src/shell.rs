use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

const BUF_CAP: usize = 256;

pub static SHELL: Mutex<Shell> = Mutex::new(Shell::new());

struct Cmd {
    name: &'static str,
    run:  fn(&mut Shell, Option<String>),
}

static COMMANDS: &[Cmd] = &[
    Cmd { name: "clear",  run: Shell::cmd_clear  },
    Cmd { name: "heap",   run: Shell::cmd_heap   },
    Cmd { name: "uptime", run: Shell::cmd_uptime },
    Cmd { name: "ls",     run: Shell::cmd_ls     },
    Cmd { name: "cat",    run: Shell::cmd_cat    },
    Cmd { name: "stat",   run: Shell::cmd_stat   },
];

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
        if self.len == 0 { return; }
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

    fn dispatch(&mut self) {
        let chars = &self.buf[..self.len];

        let start = chars.iter().position(|c| !c.is_whitespace()).unwrap_or(self.len);
        let end   = chars.iter().rposition(|c| !c.is_whitespace()).map(|i| i + 1).unwrap_or(start);
        let cmd   = &chars[start..end];

        if cmd.is_empty() { return; }

        let arg = cmd_arg(cmd);
        if let Some(entry) = COMMANDS.iter().find(|e| name_eq(cmd, e.name)) {
            (entry.run)(self, arg);
            return;
        }

        crate::print!("unknown command: ");
        for &ch in cmd { crate::print!("{}", ch); }
        crate::print!("\n");
    }

    fn cmd_clear(&mut self, _: Option<String>) {
        if let Some(w) = crate::framebuffer::WRITER.lock().as_mut() {
            w.clear();
        }
    }

    fn cmd_heap(&mut self, _: Option<String>) {
        let s = crate::allocator::stats();
        crate::println!("heap start : {:#012x}", s.heap_start);
        crate::println!("heap size  : {} MB",    s.heap_size / (1024 * 1024));
        crate::println!("bump used  : {} / {} KB", s.bump_used / 1024, s.bump_capacity / 1024);
        crate::println!("slabs:");
        for i in 0..6 {
            let used = s.slab_total[i] - s.slab_free[i];
            crate::println!("  {:>3}B  {}/{}", s.slab_sizes[i], used, s.slab_total[i]);
        }
        let mut probe: Vec<u32> = Vec::new();
        probe.push(0xDEAD_BEEF);
        probe.push(0xCAFE_BABE);
        crate::println!("alloc probe: Vec({}) OK", probe.len());
    }

    fn cmd_uptime(&mut self, _: Option<String>) {
        let ms = crate::timer::uptime_ms();
        crate::println!("Uptime: {}s {}ms", ms / 1000, ms % 1000);
    }

    fn cmd_ls(&mut self, path: Option<String>) {
        let path_str: &str = path.as_deref().unwrap_or("/");
        let is_root = path_str.split('/').filter(|s| !s.is_empty()).next().is_none();

        if is_root {
            match crate::vfs::with_root(|n| n.readdir()) {
                None                                    => crate::println!("ls: no filesystem mounted"),
                Some(names) if names.is_empty()         => crate::println!("(empty)"),
                Some(names)                             => { for name in &names { crate::println!("{}", name); } }
            }
            return;
        }

        match crate::vfs::lookup(path_str) {
            None       => crate::println!("ls: not found: {}", path_str),
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

    fn cmd_cat(&mut self, arg: Option<String>) {
        let path = match arg {
            Some(p) => p,
            None    => { crate::println!("usage: cat <path>"); return; }
        };
        match crate::vfs::lookup(&path) {
            None       => crate::println!("cat: not found: {}", path),
            Some(node) => match node.kind() {
                crate::vfs::NodeKind::Dir  => crate::println!("cat: {}: is a directory", path),
                crate::vfs::NodeKind::File => {
                    let size = node.size();
                    if size == 0 { return; }
                    let mut buf: Vec<u8> = Vec::new();
                    buf.resize(size, 0u8);
                    let n = node.read(&mut buf, 0);
                    match core::str::from_utf8(&buf[..n]) {
                        Ok(s)  => {
                            crate::print!("{}", s);
                            if !s.ends_with('\n') { crate::print!("\n"); }
                        }
                        Err(_) => crate::println!("cat: {}: binary file ({} bytes)", path, n),
                    }
                }
            },
        }
    }

    fn cmd_stat(&mut self, arg: Option<String>) {
        let path = match arg {
            Some(p) => p,
            None    => { crate::println!("usage: stat <path>"); return; }
        };
        match crate::vfs::lookup(&path) {
            None       => crate::println!("stat: not found: {}", path),
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

fn name_eq(chars: &[char], s: &str) -> bool {
    let end = chars.iter().position(|c| c.is_whitespace()).unwrap_or(chars.len());
    chars_eq(&chars[..end], s)
}

fn cmd_arg(chars: &[char]) -> Option<String> {
    let sp    = chars.iter().position(|c| c.is_whitespace())?;
    let rest  = &chars[sp + 1..];
    let start = rest.iter().position(|c| !c.is_whitespace())?;
    let trimmed = &rest[start..];
    let end   = trimmed.iter().rposition(|c| !c.is_whitespace()).map(|i| i + 1).unwrap_or(trimmed.len());
    Some(trimmed[..end].iter().collect())
}
