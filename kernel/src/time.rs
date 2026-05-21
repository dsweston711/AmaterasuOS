use core::sync::atomic::{AtomicU64, Ordering};

// TSC ticks per millisecond, set by calibrate(). Default 1000 = 1 GHz fallback.
static TSC_KHZ: AtomicU64 = AtomicU64::new(1_000);

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

pub fn cycles_to_ns(cycles: u64) -> u64 {
    let khz = TSC_KHZ.load(Ordering::Relaxed);
    // Use u128 to avoid overflow for large cycle counts.
    ((cycles as u128 * 1_000_000) / khz as u128) as u64
}

pub fn tsc_mhz() -> u64 {
    TSC_KHZ.load(Ordering::Relaxed) / 1_000
}

/// Calibrate TSC frequency using PIT channel 2 as a hardware time reference.
/// Call this once after serial is ready, before any cycles_to_ns measurements.
pub fn calibrate() {
    const PIT_COUNT: u16 = 11931; // 10 ms at 1.193182 MHz

    let tsc_start;
    let tsc_end;

    unsafe {
        // Hold gate low, program channel 2 for a 10 ms one-shot.
        let p61 = crate::pic::inb(0x61);
        crate::pic::outb(0x61, p61 & 0xFE);
        crate::pic::outb(0x43, 0xB0); // ch2, lo/hi, mode 0, binary
        crate::pic::outb(0x42, (PIT_COUNT & 0xFF) as u8);
        crate::pic::outb(0x42, (PIT_COUNT >> 8) as u8);

        tsc_start = rdtsc();

        // Start countdown (gate=1, speaker off).
        crate::pic::outb(0x61, (p61 & 0xFD) | 0x01);

        // Bit 5 of port 0x61 is the channel-2 output.
        while crate::pic::inb(0x61) & 0x20 != 0 {} // wait for LOW (counting)
        while crate::pic::inb(0x61) & 0x20 == 0 {} // wait for HIGH (done)

        tsc_end = rdtsc();
        crate::pic::outb(0x61, p61);
    }

    let tsc_khz = ((tsc_end - tsc_start) / 10).max(1);
    TSC_KHZ.store(tsc_khz, Ordering::Relaxed);
}
