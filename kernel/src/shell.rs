use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

const BUF_CAP:  usize = 256;
const HIST_CAP: usize = 16;

pub static SHELL: Mutex<Shell> = Mutex::new(Shell::new());
static CWD:      Mutex<String> = Mutex::new(String::new());
static PREV_CWD: Mutex<String> = Mutex::new(String::new());

pub(crate) struct ParsedArgs {
    pub flags:      Vec<char>,
    pub flag_vals:  Vec<(char, String)>,
    pub positional: Vec<String>,
}

impl ParsedArgs {
    pub fn has_flag(&self, c: char) -> bool {
        self.flags.contains(&c) || self.flag_vals.iter().any(|(f, _)| *f == c)
    }
    pub fn flag_val(&self, c: char) -> Option<&str> {
        self.flag_vals.iter().find(|(f, _)| *f == c).map(|(_, v)| v.as_str())
    }
    pub fn get(&self, i: usize) -> Option<&str> {
        self.positional.get(i).map(|s| s.as_str())
    }
}

struct Cmd {
    name:  &'static str,
    usage: &'static str,
    run:   fn(&mut Shell, Option<String>),
}

static COMMANDS: &[Cmd] = &[
    Cmd { name: "echo",     usage: "echo [text]",                         run: Shell::cmd_echo     },
    Cmd { name: "uname",    usage: "uname [-a|-s|-r|-m]",                 run: Shell::cmd_uname    },
    Cmd { name: "hostname", usage: "hostname",                            run: Shell::cmd_hostname },
    Cmd { name: "wc",       usage: "wc [-l|-w|-c] <path>",               run: Shell::cmd_wc       },
    Cmd { name: "head",     usage: "head [-n N] <path>",                  run: Shell::cmd_head     },
    Cmd { name: "tail",     usage: "tail [-n N] <path>",                  run: Shell::cmd_tail     },
    Cmd { name: "grep",     usage: "grep [-i|-n|-c] <pat> <path>",       run: Shell::cmd_grep     },
    Cmd { name: "clear",    usage: "clear",                               run: Shell::cmd_clear    },
    Cmd { name: "ls",       usage: "ls [path]",                           run: Shell::cmd_ls       },
    Cmd { name: "cat",      usage: "cat <path>",                          run: Shell::cmd_cat      },
    Cmd { name: "stat",     usage: "stat <path>",                         run: Shell::cmd_stat     },
    Cmd { name: "cd",       usage: "cd [path]",                           run: Shell::cmd_cd       },
    Cmd { name: "pwd",      usage: "pwd",                                 run: Shell::cmd_pwd      },
    Cmd { name: "cpu",      usage: "cpu",                                 run: Shell::cmd_cpu      },
    Cmd { name: "uptime",   usage: "uptime",                              run: Shell::cmd_uptime   },
    Cmd { name: "heap",     usage: "heap",                                run: Shell::cmd_heap     },
    Cmd { name: "reboot",   usage: "reboot",                              run: Shell::cmd_reboot   },
    Cmd { name: "shutdown", usage: "shutdown",                            run: Shell::cmd_shutdown },
    Cmd { name: "history",  usage: "history [n]",                         run: Shell::cmd_history  },
    Cmd { name: "help",     usage: "help [command]",                      run: Shell::cmd_help     },
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
            '\x08'   => self.backspace(),
            '\n'     => self.submit(),
            '\t'     => self.complete(),
            '\x03'   => self.ctrl_c(),
            '\x0c'   => self.ctrl_l(),
            '\x17'   => self.ctrl_w(),
            '\x15'   => self.ctrl_u(),
            ch       => {
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

    fn clear_line(&mut self) {
        for _ in 0..self.len {
            if let Some(w) = crate::framebuffer::WRITER.lock().as_mut() {
                w.backspace();
            }
        }
        self.len = 0;
    }

    fn ctrl_c(&mut self) {
        crate::println!("^C");
        self.len = 0;
        self.hist_cursor = 0;
        self.print_prompt();
    }

    fn ctrl_l(&mut self) {
        if let Some(w) = crate::framebuffer::WRITER.lock().as_mut() {
            w.clear();
        }
        self.print_prompt();
        for i in 0..self.len {
            crate::print!("{}", self.buf[i]);
        }
    }

    fn ctrl_w(&mut self) {
        // delete back through trailing whitespace then through the preceding word
        while self.len > 0 && self.buf[self.len - 1] == ' ' {
            if let Some(w) = crate::framebuffer::WRITER.lock().as_mut() { w.backspace(); }
            self.len -= 1;
        }
        while self.len > 0 && self.buf[self.len - 1] != ' ' {
            if let Some(w) = crate::framebuffer::WRITER.lock().as_mut() { w.backspace(); }
            self.len -= 1;
        }
    }

    fn ctrl_u(&mut self) {
        self.clear_line();
    }

    fn submit(&mut self) {
        crate::print!("\n");
        if self.len > 0 {
            let last_slot = self.hist_count.wrapping_sub(1) % HIST_CAP;
            let is_dup = self.hist_count > 0
                && self.hist_len[last_slot] == self.len
                && self.history[last_slot][..self.len] == self.buf[..self.len];
            if !is_dup {
                let slot = self.hist_count % HIST_CAP;
                self.history[slot][..self.len].copy_from_slice(&self.buf[..self.len]);
                self.hist_len[slot] = self.len;
                self.hist_count += 1;
            }
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

    fn complete(&mut self) {
        let chars = &self.buf[..self.len];

        let cmd_start = match chars.iter().position(|c| !c.is_whitespace()) {
            Some(i) => i,
            None    => return,
        };

        let space_pos = chars[cmd_start..].iter().position(|c| c.is_whitespace())
            .map(|i| cmd_start + i);

        let (candidates, prefix_len) = match space_pos {
            None => {
                // Still typing the command name.
                let prefix: String = chars[cmd_start..].iter().collect();
                let matches = COMMANDS.iter()
                    .filter(|c| c.name.starts_with(prefix.as_str()))
                    .map(|c| String::from(c.name))
                    .collect();
                (matches, chars[cmd_start..].len())
            }
            Some(sp) => {
                let cmd_name: String = chars[cmd_start..sp].iter().collect();
                let arg_start = chars[sp..].iter().position(|c| !c.is_whitespace())
                    .map(|i| sp + i)
                    .unwrap_or(self.len);
                let arg: String = chars[arg_start..].iter().collect();

                if cmd_name == "help" {
                    let matches = COMMANDS.iter()
                        .filter(|c| c.name.starts_with(arg.as_str()))
                        .map(|c| String::from(c.name))
                        .collect();
                    (matches, arg.len())
                } else {
                    complete_path(&arg)
                }
            }
        };

        match candidates.len() {
            0 => {}
            1 => {
                for ch in candidates[0][prefix_len..].chars() {
                    if self.len < BUF_CAP {
                        self.buf[self.len] = ch;
                        self.len += 1;
                        crate::print!("{}", ch);
                    }
                }
            }
            _ => {
                let lcp = longest_common_prefix(&candidates);
                if lcp > prefix_len {
                    for ch in candidates[0][prefix_len..lcp].chars() {
                        if self.len < BUF_CAP {
                            self.buf[self.len] = ch;
                            self.len += 1;
                            crate::print!("{}", ch);
                        }
                    }
                } else {
                    crate::print!("\n");
                    for c in &candidates { crate::print!("{}  ", c); }
                    crate::print!("\n");
                    self.print_prompt();
                    self.reprint_buf();
                }
            }
        }
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

        let typed: String = cmd.iter().collect();
        crate::print!("unknown command: {}", typed);
        let prefix_match = COMMANDS.iter().find(|e| e.name.starts_with(typed.as_str()));
        let suggestion = if let Some(entry) = prefix_match {
            Some(entry.name)
        } else {
            let mut best_dist = usize::MAX;
            let mut best_name = "";
            for entry in COMMANDS {
                let d = edit_distance(cmd, entry.name);
                if d < best_dist { best_dist = d; best_name = entry.name; }
            }
            if best_dist <= 2 { Some(best_name) } else { None }
        };
        if let Some(name) = suggestion {
            crate::println!(" -- did you mean '{}'?", name);
        } else {
            crate::println!();
        }
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
                Some(mut names)                 => {
                    names.sort_unstable();
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
                    let mut names = node.readdir();
                    names.sort_unstable();
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
            None    => { cwd_set(String::from("/")); return; }
        };
        if path == "-" {
            let prev = { PREV_CWD.lock().clone() };
            let dest = if prev.is_empty() { String::from("/") } else { prev };
            cwd_set(dest);
            return;
        }
        let resolved = normalize(&resolve(&path));
        if resolved == "/" {
            cwd_set(String::from("/"));
            return;
        }
        match crate::vfs::lookup(&resolved) {
            None       => crate::println!("cd: not found: {}", path),
            Some(node) => match node.kind() {
                crate::vfs::NodeKind::File => crate::println!("cd: not a directory: {}", path),
                crate::vfs::NodeKind::Dir  => cwd_set(resolved),
            },
        }
    }

    fn cmd_grep(&mut self, arg: Option<String>) {
        let s = match arg {
            Some(s) => s,
            None    => { crate::println!("usage: grep [-i] [-n] [-c] <pattern> <file>"); return; }
        };
        let parsed = parse_args(&s);
        let pattern = match parsed.get(0) {
            Some(p) => String::from(p),
            None    => { crate::println!("usage: grep [-i] [-n] [-c] <pattern> <file>"); return; }
        };
        let path = match parsed.get(1) {
            Some(p) => normalize(&resolve(p)),
            None    => { crate::println!("usage: grep [-i] [-n] [-c] <pattern> <file>"); return; }
        };
        let content = match read_file_str(&path) {
            Some(c) => c,
            None    => { crate::println!("grep: {}: not found", path); return; }
        };
        let case_insensitive = parsed.has_flag('i');
        let show_numbers     = parsed.has_flag('n');
        let count_only       = parsed.has_flag('c');
        let pat_lower = if case_insensitive { pattern.to_lowercase() } else { pattern.clone() };
        let mut matches: usize = 0;
        for (i, line) in content.lines().enumerate() {
            let haystack = if case_insensitive { line.to_lowercase() } else { String::from(line) };
            if haystack.contains(pat_lower.as_str()) {
                matches += 1;
                if !count_only {
                    if show_numbers {
                        crate::println!("{}:{}", i + 1, line);
                    } else {
                        crate::println!("{}", line);
                    }
                }
            }
        }
        if count_only { crate::println!("{}", matches); }
    }

    fn cmd_tail(&mut self, arg: Option<String>) {
        let s = match arg {
            Some(s) => s,
            None    => { crate::println!("usage: tail [-n <count>] <file>"); return; }
        };
        let parsed = parse_args(&s);
        let count: usize = parsed.flag_val('n')
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        let path = match parsed.get(0) {
            Some(p) => normalize(&resolve(p)),
            None    => { crate::println!("usage: tail [-n <count>] <file>"); return; }
        };
        let content = match read_file_str(&path) {
            Some(c) => c,
            None    => { crate::println!("tail: {}: not found", path); return; }
        };
        let lines: Vec<&str> = content.lines().collect();
        let start = lines.len().saturating_sub(count);
        for line in &lines[start..] {
            crate::println!("{}", line);
        }
    }

    fn cmd_head(&mut self, arg: Option<String>) {
        let s = match arg {
            Some(s) => s,
            None    => { crate::println!("usage: head [-n <count>] <file>"); return; }
        };
        let parsed = parse_args(&s);
        let count: usize = parsed.flag_val('n')
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        let path = match parsed.get(0) {
            Some(p) => normalize(&resolve(p)),
            None    => { crate::println!("usage: head [-n <count>] <file>"); return; }
        };
        let content = match read_file_str(&path) {
            Some(c) => c,
            None    => { crate::println!("head: {}: not found", path); return; }
        };
        for line in content.lines().take(count) {
            crate::println!("{}", line);
        }
    }

    fn cmd_wc(&mut self, arg: Option<String>) {
        let s = match arg {
            Some(s) => s,
            None    => { crate::println!("usage: wc [-l] [-w] [-c] <file>"); return; }
        };
        let parsed = parse_args(&s);
        let path = match parsed.get(0) {
            Some(p) => normalize(&resolve(p)),
            None    => { crate::println!("usage: wc [-l] [-w] [-c] <file>"); return; }
        };
        let content = match read_file_str(&path) {
            Some(c) => c,
            None    => { crate::println!("wc: {}: not found", path); return; }
        };
        let lines = content.lines().count();
        let words = content.split_whitespace().count();
        let bytes = content.len();
        let all = parsed.flags.is_empty() && parsed.flag_vals.is_empty();
        if all || parsed.has_flag('l') { crate::print!("{:8}", lines); }
        if all || parsed.has_flag('w') { crate::print!("{:8}", words); }
        if all || parsed.has_flag('c') { crate::print!("{:8}", bytes); }
        crate::println!(" {}", path);
    }

    fn cmd_hostname(&mut self, _: Option<String>) {
        match crate::vfs::lookup("/etc/hostname") {
            Some(node) if node.kind() == crate::vfs::NodeKind::File => {
                let size = node.size();
                let mut buf = alloc::vec![0u8; size];
                let n = node.read(&mut buf, 0);
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    crate::println!("{}", s.trim());
                }
            }
            _ => crate::println!("amaterasu"),
        }
    }

    fn cmd_uname(&mut self, arg: Option<String>) {
        const SYSNAME: &str = "AmaterasuOS";
        const RELEASE: &str = env!("CARGO_PKG_VERSION");
        const MACHINE: &str = "x86_64";

        let s = arg.unwrap_or_default();
        let parsed = parse_args(&s);

        if parsed.flags.is_empty() && parsed.flag_vals.is_empty() || parsed.has_flag('a') {
            crate::println!("{} {} {}", SYSNAME, RELEASE, MACHINE);
            return;
        }
        let mut parts: Vec<&str> = Vec::new();
        if parsed.has_flag('s') { parts.push(SYSNAME); }
        if parsed.has_flag('r') { parts.push(RELEASE); }
        if parsed.has_flag('m') { parts.push(MACHINE); }
        if parts.is_empty() { parts.push(SYSNAME); }
        crate::println!("{}", parts.join(" "));
    }

    fn cmd_echo(&mut self, arg: Option<String>) {
        let s = arg.unwrap_or_default();
        let parsed = parse_args(&s);
        let output = parsed.positional.join(" ");
        if parsed.has_flag('n') {
            crate::print!("{}", output);
        } else {
            crate::println!("{}", output);
        }
    }

    fn cmd_shutdown(&mut self, _: Option<String>) {
        // QEMU PIIX4 ACPI S5: SLP_EN | SLP_TYP=5 written to PM1a control block.
        unsafe { crate::pic::outw(0x604, 0x2000); }
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

    fn cmd_history(&mut self, arg: Option<String>) {
        let available = self.hist_count.min(HIST_CAP);
        if available == 0 { return; }
        let limit = match arg {
            Some(s) => s.trim().parse::<usize>().unwrap_or(available).min(available),
            None    => available,
        };
        let skip = available - limit;
        let start_slot = (self.hist_count - available + skip) % HIST_CAP;
        let start_num  =  self.hist_count - available + skip + 1;
        for i in 0..limit {
            let slot = (start_slot + i) % HIST_CAP;
            let len  = self.hist_len[slot];
            let s: String = self.history[slot][..len].iter().collect();
            crate::println!("{:4}  {}", start_num + i, s);
        }
    }

    fn cmd_help(&mut self, arg: Option<String>) {
        match arg {
            None => {
                crate::println!("AmaterasuOS -- available commands\n");
                for cmd in COMMANDS {
                    crate::println!("  {}", cmd.usage);
                }
                crate::println!("\nType 'help <command>' for detailed usage.");
                crate::println!("Type 'help tab' for tab completion tips.");
            }
            Some(cmd) => {
                let path = alloc::format!("/sys/help/{}", cmd);
                if !print_file(&path) {
                    if let Some(entry) = COMMANDS.iter().find(|e| e.name == cmd.as_str()) {
                        crate::println!("{}", entry.usage);
                    } else {
                        crate::println!("no help found for {}", cmd);
                    }
                }
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
    Some(tilde_expand(trimmed[..end].iter().collect()))
}

/// Read a VFS file at `path` and return its UTF-8 content, or None on error.
fn read_file_str(path: &str) -> Option<String> {
    let node = crate::vfs::lookup(path)?;
    if node.kind() != crate::vfs::NodeKind::File { return None; }
    let size = node.size();
    if size == 0 { return Some(String::new()); }
    let mut buf = alloc::vec![0u8; size];
    let n = node.read(&mut buf, 0);
    core::str::from_utf8(&buf[..n]).ok().map(String::from)
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

/// Set CWD to `new`, saving the previous value in PREV_CWD.
fn cwd_set(new: String) {
    let old = core::mem::replace(&mut *CWD.lock(), new);
    *PREV_CWD.lock() = old;
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

/// Given a partial path argument, return (candidates, prefix_len).
/// Each candidate is a name (with trailing `/` for directories).
/// prefix_len is how many bytes of the last path component were already typed.
fn complete_path(partial: &str) -> (Vec<String>, usize) {
    let (dir_str, prefix) = match partial.rfind('/') {
        Some(pos) => (&partial[..=pos], &partial[pos + 1..]),
        None      => ("", partial),
    };

    let dir_abs = if dir_str.is_empty() {
        cwd_get()
    } else {
        normalize(&resolve(dir_str))
    };

    let names = if dir_abs == "/" {
        match crate::vfs::with_root(|n| n.readdir()) {
            Some(n) => n,
            None    => return (Vec::new(), prefix.len()),
        }
    } else {
        match crate::vfs::lookup(&dir_abs) {
            Some(node) if node.kind() == crate::vfs::NodeKind::Dir => node.readdir(),
            _ => return (Vec::new(), prefix.len()),
        }
    };

    let dir_prefix = dir_abs.trim_end_matches('/');
    let mut candidates: Vec<String> = Vec::new();
    for name in &names {
        if name.starts_with(prefix) {
            let full = alloc::format!("{}/{}", dir_prefix, name);
            let slash = match crate::vfs::lookup(&full) {
                Some(n) if n.kind() == crate::vfs::NodeKind::Dir => "/",
                _ => "",
            };
            candidates.push(alloc::format!("{}{}", name, slash));
        }
    }

    (candidates, prefix.len())
}

/// Parse a raw argument string into flags and positional arguments.
///
/// Disambiguation rules (no flag-spec needed for v0.8 commands):
///   `-la`  → boolean flags ['l', 'a']        (remaining chars are alpha → more flags)
///   `-n10` → flag_val ('n', "10")             (remaining chars start with digit → value)
///   `-n 5` → flag_val ('n', "5")             (lone flag, next token is numeric → value)
///   `-n p`  → boolean flag 'n', positional p  (lone flag, next token is non-numeric)
///   `--`   → stop flag parsing; rest are positional
pub(crate) fn parse_args(input: &str) -> ParsedArgs {
    let mut flags:      Vec<char>          = Vec::new();
    let mut flag_vals:  Vec<(char, String)> = Vec::new();
    let mut positional: Vec<String>        = Vec::new();

    let tokens: Vec<&str> = input.split_whitespace().collect();
    let mut i = 0;
    let mut stop = false;

    while i < tokens.len() {
        let tok = tokens[i];
        if stop || !tok.starts_with('-') || tok == "-" {
            positional.push(tilde_expand(String::from(tok)));
            i += 1;
            continue;
        }
        if tok == "--" {
            stop = true;
            i += 1;
            continue;
        }

        let chars: Vec<char> = tok[1..].chars().collect();
        let mut j = 0;
        while j < chars.len() {
            let flag = chars[j];
            j += 1;
            if j < chars.len() {
                if chars[j].is_ascii_digit() {
                    // Digits following the flag letter are its value.
                    let val: String = chars[j..].iter().collect();
                    flag_vals.push((flag, val));
                    break;
                }
                // More alpha chars → combined boolean flags; keep looping.
                flags.push(flag);
            } else {
                // Last letter in this token — look at the next token.
                let next_is_num = tokens.get(i + 1)
                    .map(|t| t.starts_with(|c: char| c.is_ascii_digit()))
                    .unwrap_or(false);
                if next_is_num {
                    flag_vals.push((flag, String::from(tokens[i + 1])));
                    i += 1;
                } else {
                    flags.push(flag);
                }
            }
        }
        i += 1;
    }

    ParsedArgs { flags, flag_vals, positional }
}

fn tilde_expand(s: String) -> String {
    if s.starts_with('~') {
        let mut out = String::from("/");
        out.push_str(&s[1..]);
        out
    } else {
        s
    }
}

/// Levenshtein edit distance between a char slice and a &str.
fn edit_distance(a: &[char], b: &str) -> usize {
    let bv: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), bv.len());
    let mut row: Vec<usize> = (0..=n).collect();
    for i in 1..=m {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=n {
            let old = row[j];
            row[j] = if a[i - 1] == bv[j - 1] {
                prev
            } else {
                1 + prev.min(row[j]).min(row[j - 1])
            };
            prev = old;
        }
    }
    row[n]
}

/// Return the byte length of the longest common prefix across all candidates.
fn longest_common_prefix(candidates: &[String]) -> usize {
    if candidates.is_empty() { return 0; }
    let first = candidates[0].as_bytes();
    let mut len = first.len();
    for c in &candidates[1..] {
        let b = c.as_bytes();
        len = len.min(b.len());
        len = (0..len).take_while(|&i| first[i] == b[i]).count();
        if len == 0 { break; }
    }
    len
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
