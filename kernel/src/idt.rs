use spin::Once;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::keyboard::keyboard_handler;
use hal::timer::TIMER_VECTOR;

pub const KBD_VECTOR:      u8 = 0x21; // keyboard routed here by I/O APIC
pub const SPURIOUS_VECTOR: u8 = 0xFF; // LAPIC spurious interrupt vector

static IDT: Once<InterruptDescriptorTable> = Once::new();

pub fn init() {
    let idt = IDT.call_once(|| {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.general_protection_fault.set_handler_fn(gpf_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt[TIMER_VECTOR].set_handler_fn(timer_handler);
        idt[KBD_VECTOR].set_handler_fn(keyboard_handler);
        idt[SPURIOUS_VECTOR].set_handler_fn(spurious_handler);
        idt
    });
    idt.load();
}

extern "x86-interrupt" fn timer_handler(_frame: InterruptStackFrame) {
    hal::timer::tick();
}

// Spurious LAPIC interrupts require no EOI — just return.
extern "x86-interrupt" fn spurious_handler(_frame: InterruptStackFrame) {}

extern "x86-interrupt" fn breakpoint_handler(frame: InterruptStackFrame) {
    panic!("EXCEPTION: BREAKPOINT\n{:#?}", frame);
}

extern "x86-interrupt" fn gpf_handler(frame: InterruptStackFrame, error_code: u64) {
    panic!("EXCEPTION: GENERAL PROTECTION FAULT (error: {:#x})\n{:#?}", error_code, frame);
}

extern "x86-interrupt" fn page_fault_handler(frame: InterruptStackFrame, error_code: PageFaultErrorCode) {
    use x86_64::registers::control::Cr2;
    panic!("EXCEPTION: PAGE FAULT (addr: {:?}, error: {:?})\n{:#?}", Cr2::read(), error_code, frame);
}

