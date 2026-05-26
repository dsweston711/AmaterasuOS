use alloc::string::String;
use alloc::vec::Vec;
use crate::shell::Shell;

impl Shell {
    pub(crate) fn cmd_heap(&mut self, _: Option<String>) {
        let s = crate::allocator::stats();
        println!("heap start : {:#012x}", s.heap_start);
        println!("heap size  : {} MB",    s.heap_size / (1024 * 1024));
        println!("bump used  : {} / {} KB", s.bump_used / 1024, s.bump_capacity / 1024);
        println!("slabs:");
        for i in 0..6 {
            let used = s.slab_total[i] - s.slab_free[i];
            println!("  {:>3}B  {}/{}", s.slab_sizes[i], used, s.slab_total[i]);
        }
        let mut probe: Vec<u32> = Vec::new();
        probe.push(0xDEAD_BEEF);
        probe.push(0xCAFE_BABE);
        println!("alloc probe: Vec({}) OK", probe.len());
    }

    pub(crate) fn cmd_uptime(&mut self, _: Option<String>) {
        let ms = hal::timer::uptime_ms();
        println!("Uptime: {}s {}ms", ms / 1000, ms % 1000);
    }

    pub(crate) fn cmd_hostname(&mut self, _: Option<String>) {
        match crate::vfs::lookup("/etc/hostname") {
            Some(node) if node.kind() == crate::vfs::NodeKind::File => {
                let size = node.size();
                let mut buf = alloc::vec![0u8; size];
                let n = node.read(&mut buf, 0);
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    println!("{}", s.trim());
                }
            }
            _ => println!("amaterasu"),
        }
    }

    pub(crate) fn cmd_uname(&mut self, arg: Option<String>) {
        const SYSNAME: &str = "AmaterasuOS";
        const RELEASE: &str = env!("CARGO_PKG_VERSION");
        const MACHINE: &str = "x86_64";

        let s = arg.unwrap_or_default();
        let parsed = crate::shell::parse_args(&s);

        if parsed.flags.is_empty() && parsed.flag_vals.is_empty() || parsed.has_flag('a') {
            println!("{} {} {}", SYSNAME, RELEASE, MACHINE);
            return;
        }
        let mut parts: Vec<&str> = Vec::new();
        if parsed.has_flag('s') { parts.push(SYSNAME); }
        if parsed.has_flag('r') { parts.push(RELEASE); }
        if parsed.has_flag('m') { parts.push(MACHINE); }
        if parts.is_empty() { parts.push(SYSNAME); }
        println!("{}", parts.join(" "));
    }

    pub(crate) fn cmd_cpu(&mut self, _: Option<String>) {
        println!("vendor:  {}", hal::cpu::vendor());
        match hal::cpu::brand() {
            Some(b) => println!("brand:   {}", b),
            None    => println!("brand:   (not available)"),
        }
    }
}
