use alloc::string::String;

fn cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    // rbx is reserved by LLVM; xchg with a temp reg to preserve it.
    let (eax, ebx, ecx, edx): (u32, u32, u32, u32);
    unsafe {
        core::arch::asm!(
            "xchg {tmp:e}, ebx",
            "cpuid",
            "xchg {tmp:e}, ebx",
            inout("eax") leaf => eax,
            tmp = inout(reg) 0u32 => ebx,
            out("ecx") ecx,
            out("edx") edx,
            options(nomem, nostack, preserves_flags),
        );
    }
    (eax, ebx, ecx, edx)
}

fn u32_to_bytes(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

pub fn vendor() -> String {
    let (_, ebx, ecx, edx) = cpuid(0x0);
    let mut s = String::with_capacity(12);
    for b in u32_to_bytes(ebx) { s.push(b as char); }
    for b in u32_to_bytes(edx) { s.push(b as char); }
    for b in u32_to_bytes(ecx) { s.push(b as char); }
    s
}

pub fn brand() -> Option<String> {
    let (max_ext, _, _, _) = cpuid(0x80000000);
    if max_ext < 0x80000004 {
        return None;
    }
    let mut bytes = [0u8; 48];
    for (i, leaf) in (0x80000002u32..=0x80000004).enumerate() {
        let (eax, ebx, ecx, edx) = cpuid(leaf);
        let base = i * 16;
        bytes[base..base + 4].copy_from_slice(&u32_to_bytes(eax));
        bytes[base + 4..base + 8].copy_from_slice(&u32_to_bytes(ebx));
        bytes[base + 8..base + 12].copy_from_slice(&u32_to_bytes(ecx));
        bytes[base + 12..base + 16].copy_from_slice(&u32_to_bytes(edx));
    }
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(48);
    let trimmed = core::str::from_utf8(&bytes[..end]).unwrap_or("").trim();
    if trimmed.is_empty() { None } else { Some(String::from(trimmed)) }
}
