use core::ptr;
use spin::Once;

#[derive(Clone, Copy, Debug)]
pub struct IrqOverride {
    pub isa_irq:         u8,
    pub gsi:             u32,
    pub active_low:      bool,
    pub level_triggered: bool,
}

pub struct ApicInfo {
    pub lapic_addr:      u64,
    pub ioapic_addr:     u64,
    pub ioapic_gsi_base: u32,
    pub overrides:       [Option<IrqOverride>; 16],
}

static APIC_INFO: Once<ApicInfo> = Once::new();

pub fn get() -> Option<&'static ApicInfo> {
    APIC_INFO.get()
}

// ── ACPI table structures ────────────────────────────────────────────────────

#[repr(C, packed)]
struct Rsdp {
    signature:          [u8; 8],
    checksum:           u8,
    oem_id:             [u8; 6],
    revision:           u8,
    rsdt_addr:          u32,
    // ACPI 2.0+ extension (valid when revision >= 2)
    length:             u32,
    xsdt_addr:          u64,
    extended_checksum:  u8,
    _reserved:          [u8; 3],
}

#[repr(C, packed)]
struct SdtHeader {
    signature:        [u8; 4],
    length:           u32,
    revision:         u8,
    checksum:         u8,
    oem_id:           [u8; 6],
    oem_table_id:     [u8; 8],
    oem_revision:     u32,
    creator_id:       u32,
    creator_revision: u32,
}

#[repr(C, packed)]
struct Madt {
    header:          SdtHeader,
    local_apic_addr: u32,
    _flags:          u32,
    // followed by variable-length IC structures
}

// ── Helpers ──────────────────────────────────────────────────────────────────

#[inline]
unsafe fn phys_to_virt(phys: u64, phys_off: usize) -> usize {
    phys as usize + phys_off
}

// Walk an RSDT (entry_size=4) or XSDT (entry_size=8) looking for the MADT.
unsafe fn find_madt(root_phys: u64, entry_size: usize, phys_off: usize) -> Option<*const Madt> {
    let root_virt = phys_to_virt(root_phys, phys_off);
    let hdr       = root_virt as *const SdtHeader;
    let table_len = ptr::read_unaligned(ptr::addr_of!((*hdr).length)) as usize;
    let header_sz = core::mem::size_of::<SdtHeader>();

    if table_len < header_sz {
        return None;
    }

    let entries_base = root_virt + header_sz;
    let n_entries    = (table_len - header_sz) / entry_size;

    for i in 0..n_entries {
        let entry_ptr = (entries_base + i * entry_size) as *const u8;
        let child_phys = if entry_size == 4 {
            ptr::read_unaligned(entry_ptr as *const u32) as u64
        } else {
            ptr::read_unaligned(entry_ptr as *const u64)
        };

        let child_virt = phys_to_virt(child_phys, phys_off);
        let child_hdr  = child_virt as *const SdtHeader;
        let sig        = ptr::read_unaligned(ptr::addr_of!((*child_hdr).signature));

        if &sig == b"APIC" {
            return Some(child_virt as *const Madt);
        }
    }
    None
}

// ── Public init ──────────────────────────────────────────────────────────────

pub fn init(rsdp_phys: u64, phys_off: usize) {
    let info = unsafe { parse(rsdp_phys, phys_off) };
    match info {
        Some(i) => { APIC_INFO.call_once(|| i); }
        None    => { crate::serial_println!("[ACPI] parse failed — APIC info unavailable"); }
    }
}

unsafe fn parse(rsdp_phys: u64, phys_off: usize) -> Option<ApicInfo> {
    let rsdp_virt = phys_to_virt(rsdp_phys, phys_off);
    let rsdp      = rsdp_virt as *const Rsdp;

    // Validate signature.
    let sig = ptr::read_unaligned(ptr::addr_of!((*rsdp).signature));
    if &sig != b"RSD PTR " {
        crate::serial_println!("[ACPI] bad RSDP signature");
        return None;
    }

    let revision = ptr::read_unaligned(ptr::addr_of!((*rsdp).revision));
    crate::serial_println!("[ACPI] RSDP at phys {:#010x}, revision {}", rsdp_phys, revision);

    // Choose XSDT (64-bit pointers) on ACPI 2.0+, RSDT otherwise.
    let madt = if revision >= 2 {
        let xsdt_phys = ptr::read_unaligned(ptr::addr_of!((*rsdp).xsdt_addr));
        crate::serial_println!("[ACPI] XSDT at phys {:#010x}", xsdt_phys);
        find_madt(xsdt_phys, 8, phys_off)?
    } else {
        let rsdt_phys = ptr::read_unaligned(ptr::addr_of!((*rsdp).rsdt_addr)) as u64;
        crate::serial_println!("[ACPI] RSDT at phys {:#010x}", rsdt_phys);
        find_madt(rsdt_phys, 4, phys_off)?
    };

    let lapic_addr = ptr::read_unaligned(ptr::addr_of!((*madt).local_apic_addr)) as u64;
    let madt_len   = ptr::read_unaligned(ptr::addr_of!((*madt).header.length)) as usize;
    crate::serial_println!("[ACPI] MADT at virt {:#010x}, Local APIC phys {:#010x}", madt as usize, lapic_addr);

    // Walk MADT Interrupt Controller structures.
    let madt_virt = madt as usize;
    let madt_end  = madt_virt + madt_len;
    let mut ic    = madt_virt + core::mem::size_of::<Madt>();

    let mut ioapic_addr:     Option<u64> = None;
    let mut ioapic_gsi_base: u32         = 0;
    let mut overrides = [None::<IrqOverride>; 16];

    while ic + 2 <= madt_end {
        let entry_type = *(ic as *const u8);
        let entry_len  = *((ic + 1) as *const u8) as usize;
        if entry_len < 2 { break; }

        match entry_type {
            1 if entry_len >= 12 && ioapic_addr.is_none() => {
                // Type 1: I/O APIC
                // Layout: type(1), len(1), ioapic_id(1), reserved(1), addr(4), gsi_base(4)
                let addr = ptr::read_unaligned((ic + 4) as *const u32) as u64;
                let gsi  = ptr::read_unaligned((ic + 8) as *const u32);
                crate::serial_println!("[ACPI] I/O APIC phys {:#010x}, GSI base {}", addr, gsi);
                ioapic_addr     = Some(addr);
                ioapic_gsi_base = gsi;
            }
            2 if entry_len >= 10 => {
                // Type 2: Interrupt Source Override
                // Layout: type(1), len(1), bus(1), source(1), gsi(4), flags(2)
                let bus    = *((ic + 2) as *const u8);
                let source = *((ic + 3) as *const u8);
                let gsi    = ptr::read_unaligned((ic + 4) as *const u32);
                let flags  = ptr::read_unaligned((ic + 8) as *const u16);

                if bus == 0 && (source as usize) < 16 {
                    let polarity = flags & 0b0011;
                    let trigger  = (flags >> 2) & 0b0011;
                    let active_low      = polarity == 0b11;
                    let level_triggered = trigger  == 0b11;

                    overrides[source as usize] = Some(IrqOverride {
                        isa_irq: source, gsi, active_low, level_triggered,
                    });

                    crate::serial_println!(
                        "[ACPI] IRQ override: ISA {} -> GSI {} pol={} trig={}",
                        source, gsi,
                        if active_low { "active-low" } else { "active-high" },
                        if level_triggered { "level" } else { "edge" }
                    );
                }
            }
            _ => {}
        }

        ic += entry_len;
    }

    let ioapic_addr = ioapic_addr?;
    crate::serial_println!(
        "[ACPI] ApicInfo: lapic={:#010x}  ioapic={:#010x}  gsi_base={}",
        lapic_addr, ioapic_addr, ioapic_gsi_base
    );

    Some(ApicInfo { lapic_addr, ioapic_addr, ioapic_gsi_base, overrides })
}
