use alloc::string::String;
use crate::shell::Shell;

impl Shell {
    pub(crate) fn cmd_shutdown(&mut self, _: Option<String>) {
        unsafe { hal::pic::acpi_power_off(); }
    }

    pub(crate) fn cmd_reboot(&mut self, _: Option<String>) {
        unsafe {
            while hal::pic::inb(0x64) & 0x02 != 0 {}
            hal::pic::outb(0x64, 0xFE);
        }
    }
}
