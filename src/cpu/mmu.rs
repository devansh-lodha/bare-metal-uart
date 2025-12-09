// src/cpu/mmu.rs
use core::cell::UnsafeCell;
use aarch64_cpu::{asm::barrier, registers::*};
use tock_registers::{
    interfaces::{Writeable, ReadWriteable},
    register_bitfields,
    registers::InMemoryRegister,
};

const GRANULE_64KB_BLOCK_SIZE: u64 = 512 * 1024 * 1024; // 512MB
const MAIR_DEVICE: u64 = 0;
const MAIR_NORMAL: u64 = 1;

#[repr(align(65536))]
struct TranslationTable {
    entries: [u64; 8192],
}

struct PageTableWrapper(UnsafeCell<TranslationTable>);
unsafe impl Sync for PageTableWrapper {}

static KERNEL_TABLE: PageTableWrapper = PageTableWrapper(UnsafeCell::new(TranslationTable {
    entries: [0; 8192],
}));

register_bitfields! {
    u64,
    STAGE1_DESCRIPTOR [
        PXN      OFFSET(53) NUMBITS(1) [],
        OUTPUT   OFFSET(16) NUMBITS(32) [],
        AF       OFFSET(10) NUMBITS(1) [],
        SH       OFFSET(8) NUMBITS(2) [ InnerShareable = 0b11 ],
        AP       OFFSET(6) NUMBITS(2) [ RW_EL1 = 0b00 ],
        AttrIndx OFFSET(2) NUMBITS(3) [],
        TYPE     OFFSET(0) NUMBITS(2) [ Block = 0b01 ]
    ]
}

fn create_block(phys: u64, mair_idx: u64) -> u64 {
    let desc = InMemoryRegister::<u64, STAGE1_DESCRIPTOR::Register>::new(0);
    desc.write(
        STAGE1_DESCRIPTOR::OUTPUT.val(phys >> 16) +
        STAGE1_DESCRIPTOR::AF.val(1) +
        STAGE1_DESCRIPTOR::SH::InnerShareable +
        STAGE1_DESCRIPTOR::AP::RW_EL1 +
        STAGE1_DESCRIPTOR::AttrIndx.val(mair_idx) +
        STAGE1_DESCRIPTOR::TYPE::Block
    );
    desc.get()
}

pub unsafe fn init() {
    // 1. Define Attributes
    MAIR_EL1.write(
        MAIR_EL1::Attr0_Device::nonGathering_nonReordering_EarlyWriteAck +
        MAIR_EL1::Attr1_Normal_Outer::WriteBack_NonTransient_ReadWriteAlloc +
        MAIR_EL1::Attr1_Normal_Inner::WriteBack_NonTransient_ReadWriteAlloc
    );

    // 2. Configure TCR (48-bit PA, 42-bit VA, 64KB Granule)
    TCR_EL1.write(
        TCR_EL1::TBI0::Used +
        TCR_EL1::IPS::Bits_48 +
        TCR_EL1::TG0::KiB_64 +
        TCR_EL1::SH0::Inner +
        TCR_EL1::ORGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable +
        TCR_EL1::IRGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable +
        TCR_EL1::EPD1::DisableTTBR1Walks +
        TCR_EL1::T0SZ.val(64 - 42) 
    );

    // 3. Populate Table
    let table = &mut *KERNEL_TABLE.0.get();

    // Map Kernel (0-512MB)
    table.entries[0] = create_block(0x0000_0000, MAIR_NORMAL);

    // Map BCM Peripherals (GIC/MIP) @ 0x10_0000_0000
    let bcm_idx = (0x10_0000_0000 / GRANULE_64KB_BLOCK_SIZE) as usize;
    for i in 0..4 {
        table.entries[bcm_idx + i] = create_block(0x10_0000_0000 + (i as u64 * GRANULE_64KB_BLOCK_SIZE), MAIR_DEVICE);
    }

    // Map RP1 Peripherals @ 0x1F_0000_0000
    let rp1_idx = (0x1f_0000_0000 / GRANULE_64KB_BLOCK_SIZE) as usize;
    table.entries[rp1_idx] = create_block(0x1f_0000_0000, MAIR_DEVICE);
    table.entries[rp1_idx+1] = create_block(0x1f_0000_0000 + GRANULE_64KB_BLOCK_SIZE, MAIR_DEVICE);

    // 4. Enable
    TTBR0_EL1.set_baddr(table.entries.as_ptr() as u64);
    barrier::isb(barrier::SY);
    SCTLR_EL1.modify(SCTLR_EL1::M::Enable + SCTLR_EL1::C::Cacheable + SCTLR_EL1::I::Cacheable);
    barrier::isb(barrier::SY);
}
