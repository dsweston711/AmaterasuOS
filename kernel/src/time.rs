#[inline(always)]
pub fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags)
        );
    }
    ((hi as u64) << 32) | lo as u64
}

// Assumes 1 GHz TSC (QEMU default with -cpu qemu64).
// Replace with PIT-calibrated frequency before running on real hardware.
pub fn cycles_to_ns(cycles: u64) -> u64 {
    cycles
}
