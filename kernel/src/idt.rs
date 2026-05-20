use spin::Once;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::keyboard::keyboard_handler;

pub const PIC1_OFFSET: u8 = 0x20; // IRQ0-7 mapped to 0x20-0x27

static IDT: Once<InterruptDescriptorTable> = Once::new();

pub fn init() {
    let idt = IDT.call_once(|| {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.general_protection_fault.set_handler_fn(gpf_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt[PIC1_OFFSET + 1].set_handler_fn(keyboard_handler);
        idt
    });
    idt.load();
}

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

