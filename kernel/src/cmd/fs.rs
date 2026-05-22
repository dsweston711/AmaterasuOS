use alloc::string::String;
use alloc::vec::Vec;
use crate::shell::Shell;

impl Shell {
    pub(crate) fn cmd_ls(&mut self, path: Option<String>) {
        let resolved = path
            .map(|p| crate::shell::normalize(&crate::shell::resolve(&p)))
            .unwrap_or_else(crate::shell::cwd_get);
        let path_str = resolved.as_str();
        let is_root = path_str.split('/').filter(|s| !s.is_empty()).next().is_none();

        if is_root {
            match crate::vfs::with_root(|n| n.readdir()) {
                None                            => {
                    crate::framebuffer::set_fg(crate::framebuffer::COLOR_ERROR);
                    crate::println!("ls: no filesystem mounted");
                    crate::framebuffer::reset_colors();
                }
                Some(names) if names.is_empty() => crate::println!("(empty)"),
                Some(mut names)                 => {
                    names.sort_unstable();
                    for name in &names {
                        let full = alloc::format!("/{}", name);
                        let is_dir = matches!(crate::vfs::lookup(&full),
                            Some(n) if n.kind() == crate::vfs::NodeKind::Dir);
                        if is_dir {
                            crate::framebuffer::set_fg(crate::framebuffer::COLOR_DIR);
                            crate::println!("{}/", name);
                            crate::framebuffer::reset_colors();
                        } else {
                            crate::println!("{}", name);
                        }
                    }
                }
            }
            return;
        }

        match crate::vfs::lookup(path_str) {
            None       => {
                crate::framebuffer::set_fg(crate::framebuffer::COLOR_ERROR);
                crate::println!("ls: not found: {}", path_str);
                crate::framebuffer::reset_colors();
            }
            Some(node) => match node.kind() {
                crate::vfs::NodeKind::Dir => {
                    let mut names = node.readdir();
                    names.sort_unstable();
                    if names.is_empty() {
                        crate::println!("(empty)");
                    } else {
                        for name in &names {
                            let full = alloc::format!("{}/{}", path_str, name);
                            let is_dir = matches!(crate::vfs::lookup(&full),
                                Some(n) if n.kind() == crate::vfs::NodeKind::Dir);
                            if is_dir {
                                crate::framebuffer::set_fg(crate::framebuffer::COLOR_DIR);
                                crate::println!("{}/", name);
                                crate::framebuffer::reset_colors();
                            } else {
                                crate::println!("{}", name);
                            }
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

    pub(crate) fn cmd_cat(&mut self, arg: Option<String>) {
        let path = match arg {
            Some(p) => crate::shell::normalize(&crate::shell::resolve(&p)),
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

    pub(crate) fn cmd_stat(&mut self, arg: Option<String>) {
        let path = match arg {
            Some(p) => crate::shell::normalize(&crate::shell::resolve(&p)),
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

    pub(crate) fn cmd_cd(&mut self, arg: Option<String>) {
        let path = match arg {
            Some(p) => p,
            None    => {
                crate::shell::cwd_set(String::from("/"));
                crate::env::set("PWD", "/");
                return;
            }
        };
        if path == "-" {
            let prev = crate::shell::cwd_prev_get();
            crate::env::set("PWD", &prev);
            crate::shell::cwd_set(prev);
            return;
        }
        let resolved = crate::shell::normalize(&crate::shell::resolve(&path));
        if resolved == "/" {
            crate::shell::cwd_set(String::from("/"));
            crate::env::set("PWD", "/");
            return;
        }
        match crate::vfs::lookup(&resolved) {
            None       => crate::println!("cd: not found: {}", path),
            Some(node) => match node.kind() {
                crate::vfs::NodeKind::File => crate::println!("cd: not a directory: {}", path),
                crate::vfs::NodeKind::Dir  => {
                    crate::env::set("PWD", &resolved);
                    crate::shell::cwd_set(resolved);
                }
            },
        }
    }

    pub(crate) fn cmd_pwd(&mut self, _: Option<String>) {
        crate::println!("{}", crate::shell::cwd_get());
    }

    pub(crate) fn cmd_grep(&mut self, arg: Option<String>) {
        let s = match arg {
            Some(s) => s,
            None    => { crate::println!("usage: grep [-i] [-n] [-c] <pattern> <file>"); return; }
        };
        let parsed = crate::shell::parse_args(&s);
        let pattern = match parsed.get(0) {
            Some(p) => String::from(p),
            None    => { crate::println!("usage: grep [-i] [-n] [-c] <pattern> <file>"); return; }
        };
        let path = match parsed.get(1) {
            Some(p) => crate::shell::normalize(&crate::shell::resolve(p)),
            None    => { crate::println!("usage: grep [-i] [-n] [-c] <pattern> <file>"); return; }
        };
        let content = match crate::shell::read_file_str(&path) {
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

    pub(crate) fn cmd_tail(&mut self, arg: Option<String>) {
        let s = match arg {
            Some(s) => s,
            None    => { crate::println!("usage: tail [-n <count>] <file>"); return; }
        };
        let parsed = crate::shell::parse_args(&s);
        let count: usize = parsed.flag_val('n')
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        let path = match parsed.get(0) {
            Some(p) => crate::shell::normalize(&crate::shell::resolve(p)),
            None    => { crate::println!("usage: tail [-n <count>] <file>"); return; }
        };
        let content = match crate::shell::read_file_str(&path) {
            Some(c) => c,
            None    => { crate::println!("tail: {}: not found", path); return; }
        };
        let lines: Vec<&str> = content.lines().collect();
        let start = lines.len().saturating_sub(count);
        for line in &lines[start..] {
            crate::println!("{}", line);
        }
    }

    pub(crate) fn cmd_head(&mut self, arg: Option<String>) {
        let s = match arg {
            Some(s) => s,
            None    => { crate::println!("usage: head [-n <count>] <file>"); return; }
        };
        let parsed = crate::shell::parse_args(&s);
        let count: usize = parsed.flag_val('n')
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        let path = match parsed.get(0) {
            Some(p) => crate::shell::normalize(&crate::shell::resolve(p)),
            None    => { crate::println!("usage: head [-n <count>] <file>"); return; }
        };
        let content = match crate::shell::read_file_str(&path) {
            Some(c) => c,
            None    => { crate::println!("head: {}: not found", path); return; }
        };
        for line in content.lines().take(count) {
            crate::println!("{}", line);
        }
    }

    pub(crate) fn cmd_wc(&mut self, arg: Option<String>) {
        let s = match arg {
            Some(s) => s,
            None    => { crate::println!("usage: wc [-l] [-w] [-c] <file>"); return; }
        };
        let parsed = crate::shell::parse_args(&s);
        let path = match parsed.get(0) {
            Some(p) => crate::shell::normalize(&crate::shell::resolve(p)),
            None    => { crate::println!("usage: wc [-l] [-w] [-c] <file>"); return; }
        };
        let content = match crate::shell::read_file_str(&path) {
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
}
