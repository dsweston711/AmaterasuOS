use crate::pic::{inl, outl};

const CONFIG_ADDR: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

unsafe fn cfg_read32(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr = (1u32 << 31)
        | ((bus  as u32) << 16)
        | ((dev  as u32) << 11)
        | ((func as u32) <<  8)
        | ((offset & 0xFC) as u32);
    outl(CONFIG_ADDR, addr);
    inl(CONFIG_DATA)
}

/// Scan PCI buses for the first XHCI controller (class 0x0C / sub 0x03 / progif 0x30).
/// Returns the virtual address of BAR0 on success.
pub fn find_xhci_bar0(phys_off: usize) -> Option<usize> {
    for bus in 0u8..=255 {
        for dev in 0u8..32 {
            let vendor_dev = unsafe { cfg_read32(bus, dev, 0, 0x00) };
            if vendor_dev == 0xFFFF_FFFF { continue; }

            let class_rev = unsafe { cfg_read32(bus, dev, 0, 0x08) };
            if (class_rev >> 8) != 0x0C_03_30 { continue; }

            // 64-bit prefetchable MMIO BAR at offset 0x10 (lo) / 0x14 (hi).
            let bar_lo = unsafe { cfg_read32(bus, dev, 0, 0x10) };
            let bar_hi = unsafe { cfg_read32(bus, dev, 0, 0x14) };
            let phys   = ((bar_hi as u64) << 32) | ((bar_lo as u64) & !0xF);

            crate::serial_println!(
                "[PCI] XHCI {:02x}:{:02x}.0  vendor={:#06x} BAR0={:#010x}",
                bus, dev, vendor_dev & 0xFFFF, phys
            );
            return Some(phys as usize + phys_off);
        }
    }
    crate::serial_println!("[PCI] no XHCI controller found");
    None
}
