use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

const BUF_CAP:  usize = 256;
const HIST_CAP: usize = 16;

pub static SHELL: Mutex<Shell> = Mutex::new(Shell::new());
static CWD: Mutex<String>      = Mutex::new(String::new());

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
    Cmd { name: "cd",     run: Shell::cmd_cd     },
    Cmd { name: "pwd",    run: Shell::cmd_pwd    },
    Cmd { name: "cpu",    run: Shell::cmd_cpu    },
    Cmd { name: "reboot", run: Shell::cmd_reboot },
    Cmd { name: "help",   run: Shell::cmd_help   },
];

pub struct Shell {
    buf:         [char; BUF_CAP],
    len:         usize,
    history:     [[char; BUF_CAP]; HIST_CAP],
    hist_len:    [usize; HIST_CAP],
    hist_count:  usize,
    hist_cursor: usize,
    live_buf:    [char; BUF_CAP],
    live_len:    usize,
}

impl Shell {
    pub const fn new() -> Self {
        Self {
            buf:         ['\0'; BUF_CAP],
            len:         0,
            history:     [['\0'; BUF_CAP]; HIST_CAP],
            hist_len:    [0; HIST_CAP],
            hist_count:  0,
            hist_cursor: 0,
            live_buf:    ['\0'; BUF_CAP],
            live_len:    0,
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
        if self.len > 0 {
            let slot = self.hist_count % HIST_CAP;
            self.history[slot][..self.len].copy_from_slice(&self.buf[..self.len]);
            self.hist_len[slot] = self.len;
            self.hist_count += 1;
        }
        self.hist_cursor = 0;
        self.dispatch();
        self.len = 0;
        self.print_prompt();
    }

    pub fn history_up(&mut self) {
        let available = self.hist_count.min(HIST_CAP);
        if self.hist_cursor >= available { return; }
        if self.hist_cursor == 0 {
            self.live_buf[..self.len].copy_from_slice(&self.buf[..self.len]);
            self.live_len = self.len;
        }
        self.hist_cursor += 1;
        self.load_history_entry();
    }

    pub fn history_down(&mut self) {
        if self.hist_cursor == 0 { return; }
        self.hist_cursor -= 1;
        if self.hist_cursor == 0 {
            self.load_live();
        } else {
            self.load_history_entry();
        }
    }

    fn load_history_entry(&mut self) {
        let idx = (self.hist_count - self.hist_cursor) % HIST_CAP;
        let new_len = self.hist_len[idx];
        self.erase_line();
        self.buf[..new_len].copy_from_slice(&self.history[idx][..new_len]);
        self.len = new_len;
        self.reprint_buf();
    }

    fn load_live(&mut self) {
        self.erase_line();
        self.buf[..self.live_len].copy_from_slice(&self.live_buf[..self.live_len]);
        self.len = self.live_len;
        self.reprint_buf();
    }

    fn erase_line(&mut self) {
        if let Some(w) = crate::framebuffer::WRITER.lock().as_mut() {
            for _ in 0..self.len { w.backspace(); }
        }
        self.len = 0;
    }

    fn reprint_buf(&self) {
        for i in 0..self.len { crate::print!("{}", self.buf[i]); }
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
        let resolved = path.map(|p| normalize(&resolve(&p))).unwrap_or_else(cwd_get);
        let path_str = resolved.as_str();
        let is_root = path_str.split('/').filter(|s| !s.is_empty()).next().is_none();

        if is_root {
            match crate::vfs::with_root(|n| n.readdir()) {
                None                            => crate::println!("ls: no filesystem mounted"),
                Some(names) if names.is_empty() => crate::println!("(empty)"),
                Some(names)                     => {
                    for name in &names {
                        let full = alloc::format!("/{}", name);
                        let suffix = match crate::vfs::lookup(&full) {
                            Some(n) if n.kind() == crate::vfs::NodeKind::Dir => "/",
                            _ => "",
                        };
                        crate::println!("{}{}", name, suffix);
                    }
                }
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
                        for name in &names {
                            let full = alloc::format!("{}/{}", path_str, name);
                            let suffix = match crate::vfs::lookup(&full) {
                                Some(n) if n.kind() == crate::vfs::NodeKind::Dir => "/",
                                _ => "",
                            };
                            crate::println!("{}{}", name, suffix);
                        }
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
            Some(p) => normalize(&resolve(&p)),
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
            Some(p) => normalize(&resolve(&p)),
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

    fn cmd_cd(&mut self, arg: Option<String>) {
        let path = match arg {
            Some(p) => p,
            None    => { crate::println!("usage: cd <path>"); return; }
        };
        let resolved = normalize(&resolve(&path));
        let is_root = resolved == "/";
        if is_root {
            *CWD.lock() = String::from("/");
            return;
        }
        match crate::vfs::lookup(&resolved) {
            None       => crate::println!("cd: not found: {}", path),
            Some(node) => match node.kind() {
                crate::vfs::NodeKind::File => crate::println!("cd: not a directory: {}", path),
                crate::vfs::NodeKind::Dir  => *CWD.lock() = resolved,
            },
        }
    }

    fn cmd_reboot(&mut self, _: Option<String>) {
        unsafe {
            // Drain keyboard controller input buffer before pulsing reset.
            while crate::pic::inb(0x64) & 0x02 != 0 {}
            crate::pic::outb(0x64, 0xFE);
        }
    }

    fn cmd_cpu(&mut self, _: Option<String>) {
        crate::println!("vendor:  {}", crate::cpu::vendor());
        match crate::cpu::brand() {
            Some(b) => crate::println!("brand:   {}", b),
            None    => crate::println!("brand:   (not available)"),
        }
    }

    fn cmd_pwd(&mut self, _: Option<String>) {
        crate::println!("{}", cwd_get());
    }

    fn cmd_help(&mut self, arg: Option<String>) {
        let path = match &arg {
            None      => String::from("/sys/help.torii"),
            Some(cmd) => alloc::format!("/sys/help/{}.torii", cmd),
        };
        if !print_file(&path) {
            match &arg {
                None      => crate::println!("see /sys/help/ for per-command docs"),
                Some(cmd) => crate::println!("no help found for {}", cmd),
            }
        }
    }

    pub fn print_prompt(&self) {
        let cwd = CWD.lock();
        let display = if cwd.is_empty() { "/" } else { cwd.as_str() };
        crate::print!("amaterasu:{}> ", display);
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

/// Read a VFS file at `path` and print its UTF-8 content. Returns false if
/// the file is not found or is a directory.
pub(crate) fn print_file(path: &str) -> bool {
    match crate::vfs::lookup(path) {
        Some(node) if node.kind() == crate::vfs::NodeKind::File => {
            let size = node.size();
            if size > 0 {
                let mut buf = alloc::vec![0u8; size];
                let n = node.read(&mut buf, 0);
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    crate::print!("{}", s);
                    if !s.ends_with('\n') { crate::print!("\n"); }
                }
            }
            true
        }
        _ => false,
    }
}

/// Return the current working directory, defaulting to "/" if unset.
fn cwd_get() -> String {
    let cwd = CWD.lock();
    if cwd.is_empty() { String::from("/") } else { cwd.clone() }
}

/// Resolve `path` to an absolute path against the current CWD.
/// Paths that already start with '/' are returned as-is.
fn resolve(path: &str) -> String {
    if path.starts_with('/') {
        return String::from(path);
    }
    let base = cwd_get();
    if base.ends_with('/') {
        alloc::format!("{}{}", base, path)
    } else {
        alloc::format!("{}/{}", base, path)
    }
}

/// Collapse `.` and `..` components from an absolute path.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => { parts.pop(); }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return String::from("/");
    }
    let mut out = String::new();
    for part in &parts {
        out.push('/');
        out.push_str(part);
    }
    out
}
