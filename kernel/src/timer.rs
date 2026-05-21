use core::sync::atomic::{AtomicU64, Ordering};

pub const TIMER_VECTOR: u8 = 0x20;

// LAPIC timer register offsets
const LAPIC_LVT_TIMER:     u32 = 0x320;
const LAPIC_TIMER_INITIAL: u32 = 0x380;
const LAPIC_TIMER_CURRENT: u32 = 0x390;
const LAPIC_TIMER_DIVIDE:  u32 = 0x3E0;

// Divide-by-16 encoding: bits [3,1,0] = 011
const DIVIDE_BY_16:   u32 = 0x3;
const LVT_MASKED:     u32 = 0x0001_0000;
const LVT_PERIODIC:   u32 = 0x0002_0000;

static TICK_COUNT:   AtomicU64 = AtomicU64::new(0);
static TICKS_PER_MS: AtomicU64 = AtomicU64::new(0);

// Called from the timer IRQ handler in idt.rs every ~1 ms.
pub fn tick() {
    TICK_COUNT.fetch_add(1, Ordering::Relaxed);
    crate::apic::end_of_interrupt();
}

pub fn uptime_ms() -> u64 {
    TICK_COUNT.load(Ordering::Relaxed)
}

/// Busy-spin for `ms` milliseconds (requires interrupts enabled).
pub fn sleep(ms: u64) {
    let target = uptime_ms().saturating_add(ms);
    while uptime_ms() < target {
        core::hint::spin_loop();
    }
}

pub fn init() {
    let ticks_per_ms = unsafe { calibrate_with_pit() };
    TICKS_PER_MS.store(ticks_per_ms as u64, Ordering::Relaxed);

    crate::serial_println!(
        "[TIMER] calibrated: {} LAPIC ticks/ms (div 16, PIT reference)",
        ticks_per_ms
    );

    unsafe {
        // Switch from one-shot (used during calibration) to periodic 1 ms.
        crate::apic::lapic_write(LAPIC_LVT_TIMER, LVT_PERIODIC | TIMER_VECTOR as u32);
        crate::apic::lapic_write(LAPIC_TIMER_INITIAL, ticks_per_ms);
    }
}

/// Calibrate the LAPIC timer using PIT channel 2 as a hardware time reference.
///
/// The PIT runs at a fixed 1.193182 MHz regardless of CPU/TSC frequency, so
/// this gives correct results on any host speed.
unsafe fn calibrate_with_pit() -> u32 {
    // Count for 10 ms: floor(1_193_182 * 10 / 1000) = 11931 ticks
    const PIT_COUNT: u16 = 11931;

    // Set up PIT channel 2: mode 0 (one-shot), lo/hi byte, binary.
    // Gate is held low so counting hasn't started yet.
    let p61 = crate::pic::inb(0x61);
    crate::pic::outb(0x61, p61 & 0xFE);         // gate=0 (pause any running count)
    crate::pic::outb(0x43, 0xB0);               // ch2, lo/hi, mode 0, binary
    crate::pic::outb(0x42, (PIT_COUNT & 0xFF) as u8);
    crate::pic::outb(0x42, (PIT_COUNT >> 8)   as u8);

    // Arm LAPIC one-shot timer (masked — no IRQ during calibration).
    crate::apic::lapic_write(LAPIC_TIMER_DIVIDE, DIVIDE_BY_16);
    crate::apic::lapic_write(LAPIC_LVT_TIMER,    LVT_MASKED | TIMER_VECTOR as u32);
    crate::apic::lapic_write(LAPIC_TIMER_INITIAL, 0xFFFF_FFFF);

    // Start PIT channel 2 (gate=1, speaker off).
    // Port 0x61 bit 5 is the channel-2 output: low while counting, high when done.
    crate::pic::outb(0x61, (p61 & 0xFD) | 0x01);

    // Wait for output to go LOW (counting has begun — happens within ~1 µs).
    while crate::pic::inb(0x61) & 0x20 != 0 {}
    // Wait for output to go HIGH (10 ms elapsed).
    while crate::pic::inb(0x61) & 0x20 == 0 {}

    // Restore port 0x61.
    crate::pic::outb(0x61, p61);

    let remaining = crate::apic::lapic_read(LAPIC_TIMER_CURRENT);
    let elapsed   = 0xFFFF_FFFF_u32.wrapping_sub(remaining);
    (elapsed / 10).max(1)
}
