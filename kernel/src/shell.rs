use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

const BUF_CAP:         usize = 256;
pub(crate) const HIST_CAP: usize = 16;

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

pub(crate) struct Cmd {
    pub(crate) name:  &'static str,
    pub(crate) usage: &'static str,
    pub(crate) run:   fn(&mut Shell, Option<String>),
}

pub(crate) static COMMANDS: &[Cmd] = &[
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
    Cmd { name: "export",   usage: "export [VAR=value | VAR | (none)]",   run: Shell::cmd_export   },
    Cmd { name: "history",  usage: "history [n]",                         run: Shell::cmd_history  },
    Cmd { name: "help",     usage: "help [command]",                      run: Shell::cmd_help     },
];

pub struct Shell {
    buf:              [char; BUF_CAP],
    len:              usize,
    cursor_pos:       usize,
    prompt_col:       usize,
    pub(crate) history:     [[char; BUF_CAP]; HIST_CAP],
    pub(crate) hist_len:    [usize; HIST_CAP],
    pub(crate) hist_count:  usize,
    pub(crate) hist_cursor: usize,
    live_buf:         [char; BUF_CAP],
    live_len:         usize,
}

impl Shell {
    pub const fn new() -> Self {
        Self {
            buf:         ['\0'; BUF_CAP],
            len:         0,
            cursor_pos:  0,
            prompt_col:  0,
            history:     [['\0'; BUF_CAP]; HIST_CAP],
            hist_len:    [0; HIST_CAP],
            hist_count:  0,
            hist_cursor: 0,
            live_buf:    ['\0'; BUF_CAP],
            live_len:    0,
        }
    }

    fn cursor_char(&self) -> char {
        if self.cursor_pos < self.len { self.buf[self.cursor_pos] } else { ' ' }
    }

    pub fn push_char(&mut self, ch: char) {
        match ch {
            '\x08'   => self.backspace(),
            '\n'     => self.submit(),
            '\t'     => self.complete(),
            '\x01'   => self.cursor_to_start(),
            '\x03'   => self.ctrl_c(),
            '\x05'   => self.cursor_to_end(),
            '\x0c'   => self.ctrl_l(),
            '\x17'   => self.ctrl_w(),
            '\x15'   => self.ctrl_u(),
            ch       => {
                if self.len < BUF_CAP {
                    if self.cursor_pos == self.len {
                        // Fast path: append at end.
                        self.buf[self.len] = ch;
                        self.len += 1;
                        self.cursor_pos = self.len;
                        print!("{}", ch);
                    } else {
                        // Insert in middle: shift right, paint from the inserted char.
                        let draw_from = self.cursor_pos;
                        self.buf.copy_within(self.cursor_pos..self.len, self.cursor_pos + 1);
                        self.buf[self.cursor_pos] = ch;
                        self.len += 1;
                        self.cursor_pos += 1;
                        self.redraw_from(draw_from);
                        return; // redraw_from calls cursor_show
                    }
                }
            }
        }
        hal::framebuffer::cursor_show(self.cursor_char());
    }

    fn backspace(&mut self) {
        if self.cursor_pos == 0 { return; }
        if self.cursor_pos == self.len {
            // Fast path: delete at end.
            self.len -= 1;
            self.cursor_pos -= 1;
            if let Some(w) = hal::framebuffer::WRITER.lock().as_mut() {
                w.backspace();
            }
        } else {
            // Delete in middle: shift left, repaint from the deletion point.
            self.buf.copy_within(self.cursor_pos..self.len, self.cursor_pos - 1);
            self.len -= 1;
            self.cursor_pos -= 1;
            self.redraw_from(self.cursor_pos);
        }
    }

    /// Repaint buf[from..len] starting at screen column prompt_col+from,
    /// write a trailing space to erase any stale char (handles deletion),
    /// then reposition and show the cursor at prompt_col+cursor_pos.
    fn redraw_from(&mut self, from: usize) {
        if let Some(w) = hal::framebuffer::WRITER.lock().as_mut() {
            w.cursor_hide();
            w.set_col(self.prompt_col + from);
        }
        for i in from..self.len {
            print!("{}", self.buf[i]);
        }
        print!(" "); // erase stale trailing char after deletion
        if let Some(w) = hal::framebuffer::WRITER.lock().as_mut() {
            w.set_col(self.prompt_col + self.cursor_pos);
            let ch = self.cursor_char();
            w.cursor_show(ch);
        }
    }

    pub fn cursor_left(&mut self) {
        if self.cursor_pos == 0 { return; }
        self.cursor_pos -= 1;
        if let Some(w) = hal::framebuffer::WRITER.lock().as_mut() {
            w.cursor_hide();
            w.set_col(self.prompt_col + self.cursor_pos);
            w.cursor_show(self.cursor_char());
        }
    }

    pub fn cursor_right(&mut self) {
        if self.cursor_pos == self.len { return; }
        self.cursor_pos += 1;
        if let Some(w) = hal::framebuffer::WRITER.lock().as_mut() {
            w.cursor_hide();
            w.set_col(self.prompt_col + self.cursor_pos);
            w.cursor_show(self.cursor_char());
        }
    }

    pub fn cursor_to_start(&mut self) {
        self.cursor_pos = 0;
        if let Some(w) = hal::framebuffer::WRITER.lock().as_mut() {
            w.cursor_hide();
            w.set_col(self.prompt_col);
            w.cursor_show(self.cursor_char());
        }
    }

    pub fn cursor_to_end(&mut self) {
        self.cursor_pos = self.len;
        if let Some(w) = hal::framebuffer::WRITER.lock().as_mut() {
            w.cursor_hide();
            w.set_col(self.prompt_col + self.len);
            w.cursor_show(self.cursor_char());
        }
    }

    fn clear_line(&mut self) {
        for _ in 0..self.len {
            if let Some(w) = hal::framebuffer::WRITER.lock().as_mut() {
                w.backspace();
            }
        }
        self.len = 0;
        self.cursor_pos = 0;
    }

    fn ctrl_c(&mut self) {
        println!("^C");
        self.len = 0;
        self.cursor_pos = 0;
        self.hist_cursor = 0;
        self.print_prompt();
    }

    fn ctrl_l(&mut self) {
        if let Some(w) = hal::framebuffer::WRITER.lock().as_mut() {
            w.clear();
        }
        self.print_prompt();
        for i in 0..self.len {
            print!("{}", self.buf[i]);
        }
        hal::framebuffer::cursor_show(self.cursor_char());
    }

    fn ctrl_w(&mut self) {
        // delete back through trailing whitespace then through the preceding word
        while self.len > 0 && self.buf[self.len - 1] == ' ' {
            if let Some(w) = hal::framebuffer::WRITER.lock().as_mut() { w.backspace(); }
            self.len -= 1;
            self.cursor_pos = self.len;
        }
        while self.len > 0 && self.buf[self.len - 1] != ' ' {
            if let Some(w) = hal::framebuffer::WRITER.lock().as_mut() { w.backspace(); }
            self.len -= 1;
            self.cursor_pos = self.len;
        }
    }

    fn ctrl_u(&mut self) {
        self.clear_line();
    }

    fn submit(&mut self) {
        print!("\n");
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
        self.cursor_pos = 0;
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
        hal::framebuffer::cursor_show(self.cursor_char());
    }

    pub fn history_down(&mut self) {
        if self.hist_cursor == 0 { return; }
        self.hist_cursor -= 1;
        if self.hist_cursor == 0 {
            self.load_live();
        } else {
            self.load_history_entry();
        }
        hal::framebuffer::cursor_show(self.cursor_char());
    }

    fn load_history_entry(&mut self) {
        let idx = (self.hist_count - self.hist_cursor) % HIST_CAP;
        let new_len = self.hist_len[idx];
        self.erase_line();
        self.buf[..new_len].copy_from_slice(&self.history[idx][..new_len]);
        self.len = new_len;
        self.cursor_pos = new_len;
        self.reprint_buf();
    }

    fn load_live(&mut self) {
        self.erase_line();
        self.buf[..self.live_len].copy_from_slice(&self.live_buf[..self.live_len]);
        self.len = self.live_len;
        self.cursor_pos = self.live_len;
        self.reprint_buf();
    }

    fn erase_line(&mut self) {
        if let Some(w) = hal::framebuffer::WRITER.lock().as_mut() {
            for _ in 0..self.len { w.backspace(); }
        }
        self.len = 0;
        self.cursor_pos = 0;
    }

    fn reprint_buf(&self) {
        for i in 0..self.len { print!("{}", self.buf[i]); }
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
                        print!("{}", ch);
                    }
                }
                self.cursor_pos = self.len;
            }
            _ => {
                let lcp = longest_common_prefix(&candidates);
                if lcp > prefix_len {
                    for ch in candidates[0][prefix_len..lcp].chars() {
                        if self.len < BUF_CAP {
                            self.buf[self.len] = ch;
                            self.len += 1;
                            print!("{}", ch);
                        }
                    }
                    self.cursor_pos = self.len;
                } else {
                    print!("\n");
                    for c in &candidates { print!("{}  ", c); }
                    print!("\n");
                    self.print_prompt();
                    self.reprint_buf();
                    hal::framebuffer::cursor_show(self.cursor_char());
                }
            }
        }
    }

    fn dispatch(&mut self) {
        let input: String = self.buf[..self.len].iter().collect();
        let segments = split_commands(&input);
        for seg in segments {
            let seg = seg.trim();
            if seg.is_empty() { continue; }
            let chars: Vec<char> = seg.chars().collect();
            self.dispatch_one(&chars);
        }
    }

    fn dispatch_one(&mut self, cmd: &[char]) {
        let start = cmd.iter().position(|c| !c.is_whitespace()).unwrap_or(cmd.len());
        let end   = cmd.iter().rposition(|c| !c.is_whitespace()).map(|i| i + 1).unwrap_or(start);
        let raw: String = cmd[start..end].iter().collect();
        let expanded = crate::env::expand(&raw);
        let cmd: alloc::vec::Vec<char> = expanded.chars().collect();
        let cmd = cmd.as_slice();

        if cmd.is_empty() { return; }

        let arg = cmd_arg(cmd);
        if let Some(entry) = COMMANDS.iter().find(|e| name_eq(cmd, e.name)) {
            (entry.run)(self, arg);
            return;
        }

        let typed: String = cmd.iter().collect();
        print!("unknown command: {}", typed);
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
            println!(" -- did you mean '{}'?", name);
        } else {
            println!();
        }
    }

    pub fn print_prompt(&mut self) {
        let display = {
            let cwd = CWD.lock();
            if cwd.is_empty() { String::from("/") } else { String::from(cwd.as_str()) }
        };
        print!("amaterasu:");
        hal::framebuffer::set_fg(hal::framebuffer::COLOR_PROMPT);
        print!("{}", display);
        hal::framebuffer::reset_colors();
        print!("> ");
        self.prompt_col = if let Some(w) = hal::framebuffer::WRITER.lock().as_mut() {
            w.get_col()
        } else { 0 };
        self.cursor_pos = self.len;
        hal::framebuffer::cursor_show(self.cursor_char());
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
    let raw: String = trimmed[..end].iter().collect();
    let unquoted = if raw.len() >= 2
        && ((raw.starts_with('"')  && raw.ends_with('"'))
         || (raw.starts_with('\'') && raw.ends_with('\'')))
    {
        String::from(&raw[1..raw.len() - 1])
    } else {
        raw
    };
    Some(tilde_expand(unquoted))
}

/// Read a VFS file at `path` and return its UTF-8 content, or None on error.
pub(crate) fn read_file_str(path: &str) -> Option<String> {
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
                    print!("{}", s);
                    if !s.ends_with('\n') { print!("\n"); }
                }
            }
            true
        }
        _ => false,
    }
}

/// Return the current working directory, defaulting to "/" if unset.
pub(crate) fn cwd_get() -> String {
    let cwd = CWD.lock();
    if cwd.is_empty() { String::from("/") } else { cwd.clone() }
}

/// Return the previous working directory (for `cd -`), defaulting to "/".
pub(crate) fn cwd_prev_get() -> String {
    let prev = PREV_CWD.lock();
    if prev.is_empty() { String::from("/") } else { prev.clone() }
}

/// Set CWD to `new`, saving the previous value in PREV_CWD.
pub(crate) fn cwd_set(new: String) {
    let old = core::mem::replace(&mut *CWD.lock(), new);
    *PREV_CWD.lock() = old;
}

/// Resolve `path` to an absolute path against the current CWD.
/// Paths that already start with '/' are returned as-is.
pub(crate) fn resolve(path: &str) -> String {
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

    let tokens: Vec<String> = tokenize_quoted(input);
    let mut i = 0;
    let mut stop = false;

    while i < tokens.len() {
        let tok = tokens[i].as_str();
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
                    flag_vals.push((flag, tokens[i + 1].clone()));
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

/// Split on `;` or `&&` outside of quoted spans; returns each segment as a &str.
/// `&&` is treated as unconditional chaining (commands have no exit-status return).
fn split_commands(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut in_quote: Option<char> = None;
    let mut start = 0;
    let chars: Vec<(usize, char)> = input.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (byte_pos, ch) = chars[i];
        match in_quote {
            Some(q) if ch == q => { in_quote = None; i += 1; }
            Some(_)            => { i += 1; }
            None => match ch {
                '"' | '\'' => { in_quote = Some(ch); i += 1; }
                ';' => {
                    out.push(&input[start..byte_pos]);
                    start = byte_pos + 1;
                    i += 1;
                }
                '&' if i + 1 < chars.len() && chars[i + 1].1 == '&' => {
                    out.push(&input[start..byte_pos]);
                    start = chars[i + 1].0 + 1;
                    i += 2;
                }
                _ => { i += 1; }
            }
        }
    }
    out.push(&input[start..]);
    out
}

/// Split input into tokens respecting single- and double-quoted spans.
/// Quotes are stripped from the resulting tokens.
fn tokenize_quoted(input: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quote: Option<char> = None;
    for ch in input.chars() {
        match in_quote {
            Some(q) if ch == q => in_quote = None,
            Some(_)            => current.push(ch),
            None => match ch {
                '"' | '\'' => in_quote = Some(ch),
                c if c.is_whitespace() => {
                    if !current.is_empty() {
                        tokens.push(core::mem::take(&mut current));
                    }
                }
                c => current.push(c),
            },
        }
    }
    if !current.is_empty() { tokens.push(current); }
    tokens
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
pub(crate) fn normalize(path: &str) -> String {
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
