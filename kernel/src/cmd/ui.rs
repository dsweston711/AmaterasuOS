use alloc::string::String;
use crate::shell::Shell;

impl Shell {
    pub(crate) fn cmd_clear(&mut self, _: Option<String>) {
        if let Some(w) = hal::framebuffer::WRITER.lock().as_mut() {
            w.clear();
        }
    }

    pub(crate) fn cmd_echo(&mut self, arg: Option<String>) {
        let s = arg.unwrap_or_default();
        let parsed = crate::shell::parse_args(&s);
        let output = parsed.positional.join(" ");
        if parsed.has_flag('n') {
            print!("{}", output);
        } else {
            println!("{}", output);
        }
    }

    pub(crate) fn cmd_history(&mut self, arg: Option<String>) {
        let available = self.hist_count.min(crate::shell::HIST_CAP);
        if available == 0 { return; }
        let limit = match arg {
            Some(s) => s.trim().parse::<usize>().unwrap_or(available).min(available),
            None    => available,
        };
        let skip = available - limit;
        let start_slot = (self.hist_count - available + skip) % crate::shell::HIST_CAP;
        let start_num  =  self.hist_count - available + skip + 1;
        for i in 0..limit {
            let slot = (start_slot + i) % crate::shell::HIST_CAP;
            let len  = self.hist_len[slot];
            let s: String = self.history[slot][..len].iter().collect();
            println!("{:4}  {}", start_num + i, s);
        }
    }

    pub(crate) fn cmd_help(&mut self, arg: Option<String>) {
        match arg {
            None => {
                println!("AmaterasuOS -- available commands\n");
                for cmd in crate::shell::COMMANDS {
                    println!("  {}", cmd.usage);
                }
                println!("\nType 'help <command>' for detailed usage.");
                println!("Type 'help tab' for tab completion tips.");
            }
            Some(cmd_name) => {
                let path = alloc::format!("/sys/help/{}", cmd_name);
                if !crate::shell::print_file(&path) {
                    if let Some(entry) = crate::shell::COMMANDS.iter().find(|e| e.name == cmd_name.as_str()) {
                        println!("{}", entry.usage);
                    } else {
                        println!("no help found for {}", cmd_name);
                    }
                }
            }
        }
    }

    pub(crate) fn cmd_export(&mut self, arg: Option<String>) {
        match arg {
            None => {
                let mut vars = crate::env::list();
                vars.sort_unstable_by(|a, b| a.0.cmp(&b.0));
                for (k, v) in vars {
                    println!("{}={}", k, v);
                }
            }
            Some(s) => {
                let s = String::from(s.trim());
                if let Some(eq) = s.find('=') {
                    let key = &s[..eq];
                    let val = &s[eq + 1..];
                    crate::env::set(key, val);
                } else if s.is_empty() {
                    // `export` with only whitespace — treat as no-arg
                    let mut vars = crate::env::list();
                    vars.sort_unstable_by(|a, b| a.0.cmp(&b.0));
                    for (k, v) in vars {
                        println!("{}={}", k, v);
                    }
                } else {
                    match crate::env::get(&s) {
                        Some(v) => println!("{}={}", s, v),
                        None    => println!("export: {}: not set", s),
                    }
                }
            }
        }
    }
}
