// src/drivers/pcie.rs
//! PCIe Root Complex & RP1 Southbridge Driver
//!
//! This module handles the initialization of the BCM2712 PCIe Root Complex (RC)
//! and the discovery of the RP1 Southbridge.
//!
//! # Architecture
//! The RP1 is connected via a 4-lane PCIe Gen2 link.
//! Interrupts are signaled via MSI-X (Memory Writes).
//!
//! # Address Translation (The Critical Part)
//! The RP1 (PCIe Endpoint) cannot write directly to the CPU's physical address space
//! at 0x10_xxxx_xxxx because it uses 32-bit addressing for MSI-X by default.
//!
//! We must configure an **Inbound Translation Window (BAR2)** in the Root Complex:
//! - **PCIe Bus Address:** 0xFFFF_F000 (Where RP1 writes)
//! - **CPU Phys Address:** 0x10_0013_0000 (Where the MIP Doorbell lives)

use tock_registers::{
    interfaces::{Readable, Writeable, ReadWriteable},
    register_bitfields, register_structs,
    registers::{ReadWrite, ReadOnly},
};
use crate::println;
use core::ptr::{write_volatile, read_volatile};
use aarch64_cpu::asm::barrier;

// --- Configuration Constants ---
const PCIE_RC_BASE:   u64 = 0x10_0012_0000; // PCIe2 RC Base
const RP1_CFG_BASE:   u64 = 0x1f_0010_9000; // RP1 Configuration Space Window
const RP1_APB_BASE:   u64 = 0x1f_0010_8000; // RP1 Internal Peripherals
const RP1_MSIX_TABLE: u64 = 0x1f_0041_0000; // MSI-X Table Window

const UART0_VECTOR: usize = 25; // Hardware-assigned vector for UART0

// The "Virtual" PCIe address we assign to the MIP Doorbell
const MIP_DOORBELL_PCI_LO: u32 = 0xFFFF_F000;
const MIP_DOORBELL_PCI_HI: u32 = 0x0000_00FF;

// --- Register Definitions ---

register_bitfields! {
    u32,
    MISC_CTRL [
        SCB_ACCESS_EN OFFSET(12) NUMBITS(1) [] // Bit 12: Enable System Core Bus Access
    ]
}

register_structs! {
    #[allow(non_snake_case)]
    pub PcieRootComplex {
        (0x0000 => _reserved_padding),
        (0x4008 => pub MISC_CTRL: ReadWrite<u32, MISC_CTRL::Register>),
        (0x400C => _reserved0),
        // Inbound Window 2 (BAR2) Configuration
        (0x4034 => pub BAR2_LO: ReadWrite<u32>),
        (0x4038 => pub BAR2_HI: ReadWrite<u32>),
        (0x403C => _reserved1),
        (0x40B4 => pub UBUS_BAR2_LO: ReadWrite<u32>),
        (0x40B8 => pub UBUS_BAR2_HI: ReadWrite<u32>),
        (0x40BC => @END),
    }
}

register_bitfields! {
    u32,
    CMD [
        BUS_MASTER OFFSET(2) NUMBITS(1) [], // Allow RP1 to issue Memory Writes
        MEM_ACCESS OFFSET(1) NUMBITS(1) []
    ]
}

register_structs! {
    #[allow(non_snake_case)]
    pub Rp1Config {
        (0x00 => pub VENDOR_ID: ReadOnly<u16>),
        (0x02 => pub DEVICE_ID: ReadOnly<u16>),
        (0x04 => pub COMMAND: ReadWrite<u32, CMD::Register>),
        (0x08 => _reserved0),
        (0x34 => pub CAP_PTR: ReadOnly<u32>), // Capabilities Pointer (Linked List)
        (0x38 => @END),
    }
}

// --- Implementation ---

pub unsafe fn init() {
    println!("[INFO] PCIe Bridge Initialization...");
    let rc = &*(PCIE_RC_BASE as *const PcieRootComplex);

    // 1. Enable System Bus Access
    // Without this, the RC blocks all inbound transactions from RP1.
    let misc = rc.MISC_CTRL.extract();
    if !misc.is_set(MISC_CTRL::SCB_ACCESS_EN) {
        rc.MISC_CTRL.modify(MISC_CTRL::SCB_ACCESS_EN::SET);
        println!("       |__ SCB Access: [ENABLED]");
    } else {
        println!("       |__ SCB Access: [ALREADY ACTIVE]");
    }

    // 2. Configure Inbound Window (BAR2)
    // Map PCIe Address -> CPU Address.
    // 0x1C flags = 64-bit | Prefetchable
    rc.BAR2_LO.set(MIP_DOORBELL_PCI_LO | 0x1C); 
    rc.BAR2_HI.set(MIP_DOORBELL_PCI_HI);
    
    // UBUS Remap: Target the MIP Physical Address (0x10_0013_0000)
    // Bit 0 = Enable Window
    rc.UBUS_BAR2_LO.set(0x0013_0000 | 1); 
    rc.UBUS_BAR2_HI.set(0x0000_0010);
    
    println!("       |__ BAR2 Remap: CPU[0x10_0013_0000] <-> PCIe[0x{:x}_{:x}]", 
             MIP_DOORBELL_PCI_HI, MIP_DOORBELL_PCI_LO);

    barrier::dmb(barrier::SY);

    init_rp1();
}

unsafe fn init_rp1() {
    println!("[INFO] RP1 Southbridge Discovery...");
    let rp1 = &*(RP1_CFG_BASE as *const Rp1Config);

    // 1. Identity Check
    let vid = rp1.VENDOR_ID.get();
    let did = rp1.DEVICE_ID.get();
    println!("       |__ ID Check: Vendor 0x{:x} Device 0x{:x}", vid, did);

    if vid != 0x1DE4 {
        println!("[WARN] Unexpected Vendor ID! (Expected 0x1DE4)");
    }

    // 2. Enable Bus Mastering (Crucial for MSI-X)
    rp1.COMMAND.modify(CMD::BUS_MASTER::SET + CMD::MEM_ACCESS::SET);

    // 3. Enable MSI-X Capability
    // Walk the PCI capability list to find ID 0x11 (MSI-X)
    let mut cap_offset = (rp1.CAP_PTR.get() & 0xFF) as usize;
    while cap_offset != 0 {
        let cap_header_addr = (RP1_CFG_BASE as *const u32).byte_add(cap_offset);
        let cap_header = read_volatile(cap_header_addr);
        
        if (cap_header & 0xFF) as u8 == 0x11 { // 0x11 = MSI-X
            println!("       |__ MSI-X Capability: Found at Offset 0x{:x}", cap_offset);
            
            // Bit 31 is MSI-X Enable
            if (cap_header & (1 << 31)) == 0 {
                write_volatile(cap_header_addr as *mut u32, cap_header | (1 << 31));
                println!("       |__ MSI-X Status: [ENABLED]");
            } else {
                println!("       |__ MSI-X Status: [ALREADY ENABLED]");
            }
            break;
        }
        cap_offset = ((cap_header >> 8) & 0xFF) as usize; // Next Capability
    }

    configure_msix_vector(UART0_VECTOR);
}

unsafe fn configure_msix_vector(vector: usize) {
    // 1. Program the MSI-X Table Entry
    // Address = Our BAR2 PCIe Address
    // Data = 0 (Signals MIP Input 0)
    let table_base = RP1_MSIX_TABLE as *mut u32;
    let entry_ptr  = table_base.add(vector * 4); 

    write_volatile(entry_ptr.add(0), MIP_DOORBELL_PCI_LO);
    write_volatile(entry_ptr.add(1), MIP_DOORBELL_PCI_HI); 
    write_volatile(entry_ptr.add(2), 0x0000_0000); 
    write_volatile(entry_ptr.add(3), 0x0000_0000); // Unmasked

    barrier::dmb(barrier::SY);

    // 2. Unmask at RP1 Internal Controller (APB)
    let ctrl_base = (RP1_APB_BASE + 0x008) as *mut u32;
    let ctrl_reg  = ctrl_base.add(vector);
    
    // Magic: 0x9 (Bit 3=Enable, Bit 0=Target)
    write_volatile(ctrl_reg, (1 << 3) | (1 << 0)); 
    
    println!("       |__ MSI-X Vector {}: Configured -> MIP Doorbell", vector);
}
