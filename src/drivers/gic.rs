// src/drivers/gic.rs
//! GICv2 & Machine Interrupt Peripheral (MIP) Driver
//!
//! This module manages the BCM2712 Interrupt Controller.
//!
//! # The Chain
//! RP1 MSI-X -> MIP Input 0 -> GIC SPI 160 -> CPU Core 0
//!
//! # Critical Details
//! 1. **VPU Masking:** The firmware enables VPU (VideoCore) interrupts by default.
//!    If not masked, the VPU will "steal" the interrupts, and the CPU will see nothing.
//! 2. **Trigger Type:** The RP1 sends an Edge signal. The MIP must be configured
//!    to expect this (CFG register = 0xFFFFFFFF).

use tock_registers::{
    interfaces::{Readable, Writeable}, 
    register_bitfields, register_structs,
    registers::{ReadWrite, ReadOnly, WriteOnly},
};
use crate::{println, drivers::uart};
use core::ptr::{read_volatile, write_volatile};

// --- Constants ---
const MIP_BASE:        u64 = 0x10_0013_0000;
const GICD_BASE:       u64 = 0x10_7FFF_9000;
const GICC_BASE:       u64 = 0x10_7FFF_A000;

// Mapping: MIP Input 0 corresponds to GIC SPI 128.
// GIC ID = SPI_Base (32) + 128 = 160.
const IRQ_MIP_INPUT0: u32 = 160; 

// --- MIP Definitions ---
register_structs! {
    #[allow(non_snake_case)]
    pub MipRegs {
        (0x00 => pub MIP_STATUS: ReadOnly<u32>),
        (0x04 => _reserved0),
        // Host (ARM CPU) Config
        (0x20 => pub INT_CFGL_HOST: ReadWrite<u32>), 
        (0x24 => _reserved1),
        (0x30 => pub INT_CFGH_HOST: ReadWrite<u32>),
        (0x34 => _reserved2),
        (0x40 => pub INT_MASKL_HOST: ReadWrite<u32>), 
        (0x44 => _reserved3),
        (0x50 => pub INT_MASKH_HOST: ReadWrite<u32>),
        
        // VPU (VideoCore) Config
        (0x54 => _reserved4),
        (0x60 => pub INT_MASKL_VPU: ReadWrite<u32>),
        (0x64 => _reserved5),
        (0x70 => pub INT_MASKH_VPU: ReadWrite<u32>),
        (0x74 => @END),
    }
}

// --- GIC Definitions ---
register_bitfields! {
    u32,
    GICC_CTLR [
        ENABLE_GRP0 OFFSET(0) NUMBITS(1) [], // Secure
        ENABLE_GRP1 OFFSET(1) NUMBITS(1) []  // Non-Secure
    ],
    GICC_PMR [ PRIORITY OFFSET(0) NUMBITS(8) [] ],
    GICC_IAR [ INTERRUPT_ID OFFSET(0) NUMBITS(10) [] ],
    GICD_CTLR [ ENABLE OFFSET(0) NUMBITS(1) [] ]
}

register_structs! {
    #[allow(non_snake_case)]
    pub GicCpuInterface {
        (0x00 => pub CTLR: ReadWrite<u32, GICC_CTLR::Register>),
        (0x04 => pub PMR:  ReadWrite<u32, GICC_PMR::Register>),
        (0x08 => _reserved0),
        (0x0C => pub IAR:  ReadOnly<u32, GICC_IAR::Register>),
        (0x10 => pub EOIR: WriteOnly<u32>),
        (0x14 => @END),
    }
}

register_structs! {
    #[allow(non_snake_case)]
    pub GicDistributor {
        (0x000 => pub CTLR: ReadWrite<u32, GICD_CTLR::Register>),
        (0x004 => pub TYPER: ReadOnly<u32>),
        (0x008 => @END),
    }
}

// --- Implementation ---

pub unsafe fn init() {
    println!("[INFO] GICv2 & MIP Initialization...");
    init_mip();
    init_gic();
}

unsafe fn init_mip() {
    let mip = &*(MIP_BASE as *const MipRegs);
    
    // 1. Mask VPU (Crucial!)
    mip.INT_MASKL_VPU.set(0xFFFFFFFF);
    mip.INT_MASKH_VPU.set(0xFFFFFFFF);

    // 2. Configure Host Trigger (Active High Level / Edge)
    // 0xFFFFFFFF ensures latching logic matches RP1 signaling.
    mip.INT_CFGL_HOST.set(0xFFFFFFFF);
    mip.INT_CFGH_HOST.set(0xFFFFFFFF);

    // 3. Unmask Host
    mip.INT_MASKL_HOST.set(0x0000_0000);
    mip.INT_MASKH_HOST.set(0x0000_0000);
    
    println!("       |__ MIP: Host Configured (CFG=0xFF..), VPU Masked");
}

unsafe fn init_gic() {
    let gicd = &*(GICD_BASE as *const GicDistributor);
    let gicc = &*(GICC_BASE as *const GicCpuInterface);

    // 1. Disable Distributor
    gicd.CTLR.write(GICD_CTLR::ENABLE::CLEAR);

    // 2. Configure the specific Interrupt Line
    configure_spi(IRQ_MIP_INPUT0);

    // 3. Enable Distributor
    gicd.CTLR.write(GICD_CTLR::ENABLE::SET);
    println!("       |__ GICD: Distributor Enabled");

    // 4. Configure CPU Interface (Allow all priorities, Enable Groups)
    gicc.PMR.write(GICC_PMR::PRIORITY.val(0xF0)); 
    gicc.CTLR.set(0x3);
    println!("       |__ GICC: CPU Interface Enabled (Grp0 + Grp1)");
}

unsafe fn configure_spi(id: u32) {
    let gicd_base = GICD_BASE as *mut u32;

    // Helper macro to calculate register address based on ID
    // Note: This manual calculation is used because GIC registers are array-like.

    // A. Group 1 (Non-Secure)
    let group_reg = gicd_base.byte_add(0x080 + ((id as usize / 32) * 4));
    let current_grp = read_volatile(group_reg);
    write_volatile(group_reg, current_grp | (1 << (id % 32)));

    // B. Enable Interrupt
    let enable_reg = gicd_base.byte_add(0x100 + ((id as usize / 32) * 4));
    let current_en = read_volatile(enable_reg);
    write_volatile(enable_reg, current_en | (1 << (id % 32)));

    // C. Target CPU0
    let target_reg = gicd_base.byte_add(0x800 + ((id as usize / 4) * 4));
    let shift = (id % 4) * 8;
    let mut current_target = read_volatile(target_reg);
    current_target &= !(0xFF << shift); 
    current_target |= 0x01 << shift;
    write_volatile(target_reg, current_target);

    // D. Priority 0x80 (Mid-range)
    let prio_reg = gicd_base.byte_add(0x400 + ((id as usize / 4) * 4));
    let prio_shift = (id % 4) * 8;
    let mut current_prio = read_volatile(prio_reg);
    current_prio &= !(0xFF << prio_shift);
    current_prio |= 0x80 << prio_shift;
    write_volatile(prio_reg, current_prio);

    // E. Edge Trigger Configuration
    let cfg_reg = gicd_base.byte_add(0xC00 + ((id as usize / 16) * 4));
    let cfg_shift = (id % 16) * 2;
    let mut current_cfg = read_volatile(cfg_reg);
    current_cfg &= !(0x3 << cfg_shift);
    current_cfg |= 0x2 << cfg_shift; // 0b10 = Edge
    write_volatile(cfg_reg, current_cfg);

    println!("       |__ SPI {}: Grp1, Prio=0x80, Target=CPU0, Edge", id);
}

// --- Interrupt Service Routine ---

/// The Main Interrupt Handler.
/// Called from `vectors.s` when an IRQ occurs.
#[no_mangle]
pub unsafe extern "C" fn handle_irq_router() {
    let gicc = &*(GICC_BASE as *const GicCpuInterface);
    
    // 1. Acknowledge Interrupt
    let iar = gicc.IAR.get();
    let id = iar & 0x3FF;

    match id {
        IRQ_MIP_INPUT0 => {
            // Dispatch to UART driver
            uart::handle_irq();
        },
        1023 => { /* Spurious Interrupt */ },
        _ => {
            println!("\n[WARN] Unexpected IRQ ID: {}", id);
        }
    }

    // 2. End of Interrupt
    gicc.EOIR.set(iar);
}
