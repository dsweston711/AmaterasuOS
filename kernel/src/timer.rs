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
    unsafe {
        // ── Calibration: measure LAPIC ticks in 10 ms using RDTSC ───────
        crate::apic::lapic_write(LAPIC_TIMER_DIVIDE, DIVIDE_BY_16);
        // One-shot, masked — no interrupt fires during calibration.
        crate::apic::lapic_write(LAPIC_LVT_TIMER, LVT_MASKED | TIMER_VECTOR as u32);
        crate::apic::lapic_write(LAPIC_TIMER_INITIAL, 0xFFFF_FFFF);

        let t0 = crate::time::rdtsc();
        // cycles_to_ns is 1:1 at 1 GHz (QEMU default); wait 10 ms.
        while crate::time::cycles_to_ns(crate::time::rdtsc() - t0) < 10_000_000 {}

        let remaining   = crate::apic::lapic_read(LAPIC_TIMER_CURRENT);
        let elapsed     = 0xFFFF_FFFF_u32 - remaining;
        let ticks_per_ms = (elapsed / 10).max(1); // guard against zero
        TICKS_PER_MS.store(ticks_per_ms as u64, Ordering::Relaxed);

        crate::serial_println!(
            "[TIMER] calibrated: {} LAPIC ticks/ms (div 16)",
            ticks_per_ms
        );

        // ── Start periodic 1 ms timer ────────────────────────────────────
        crate::apic::lapic_write(LAPIC_LVT_TIMER, LVT_PERIODIC | TIMER_VECTOR as u32);
        crate::apic::lapic_write(LAPIC_TIMER_INITIAL, ticks_per_ms);
    }
}
