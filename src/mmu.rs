use core::cell::UnsafeCell;
use aarch64_cpu::{asm::barrier, registers::*};
use tock_registers::{
    interfaces::{Writeable, ReadWriteable},
    register_bitfields,
    registers::InMemoryRegister,
};

// -----------------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------------
// RPi5 (ARM v8.2+) supports 64KB granules.
// In 64KB granule, Level 2 blocks are 512MB.
const GRANULE_64KB_BLOCK_SIZE: u64 = 512 * 1024 * 1024;

const MAIR_DEVICE: u64 = 0;
const MAIR_NORMAL: u64 = 1;

// -----------------------------------------------------------------------------
// Safe Table Wrapper
// -----------------------------------------------------------------------------
#[repr(align(65536))]
struct TranslationTable {
    entries: [u64; 8192], // Sufficient for L2 table
}

// We wrap it in UnsafeCell to be explicit about interior mutability,
// though in a single-core bootloader, Sync impl is "technically" just a promise.
struct PageTableWrapper(UnsafeCell<TranslationTable>);

// SAFETY: Single core access only in this bare metal example.
unsafe impl Sync for PageTableWrapper {}

static KERNEL_TABLE: PageTableWrapper = PageTableWrapper(UnsafeCell::new(TranslationTable {
    entries: [0; 8192],
}));

// -----------------------------------------------------------------------------
// Descriptor Definitions
// -----------------------------------------------------------------------------
register_bitfields! {
    u64,
    STAGE1_DESCRIPTOR [
        PXN      OFFSET(53) NUMBITS(1) [], // Privileged Execute Never
        OUTPUT   OFFSET(16) NUMBITS(32) [], // Physical Address
        AF       OFFSET(10) NUMBITS(1) [], // Access Flag
        SH       OFFSET(8) NUMBITS(2) [    // Shareability
            InnerShareable = 0b11
        ],
        AP       OFFSET(6) NUMBITS(2) [    // Access Permissions
            RW_EL1 = 0b00
        ],
        AttrIndx OFFSET(2) NUMBITS(3) [],  // MAIR Index
        TYPE     OFFSET(0) NUMBITS(2) [    // Descriptor Type
            Block = 0b01
        ]
    ]
}

fn create_block(phys: u64, mair_idx: u64) -> u64 {
    let desc = InMemoryRegister::<u64, STAGE1_DESCRIPTOR::Register>::new(0);
    desc.write(
        STAGE1_DESCRIPTOR::OUTPUT.val(phys >> 16) + // 64KB Granule shifts
        STAGE1_DESCRIPTOR::AF.val(1) +
        STAGE1_DESCRIPTOR::SH::InnerShareable +
        STAGE1_DESCRIPTOR::AP::RW_EL1 +
        STAGE1_DESCRIPTOR::AttrIndx.val(mair_idx) +
        STAGE1_DESCRIPTOR::TYPE::Block
    );
    desc.get()
}

// -----------------------------------------------------------------------------
// Init
// -----------------------------------------------------------------------------
pub unsafe fn init() {
    // 1. Define Memory Attributes (MAIR)
    MAIR_EL1.write(
        MAIR_EL1::Attr0_Device::nonGathering_nonReordering_EarlyWriteAck +
        MAIR_EL1::Attr1_Normal_Outer::WriteBack_NonTransient_ReadWriteAlloc +
        MAIR_EL1::Attr1_Normal_Inner::WriteBack_NonTransient_ReadWriteAlloc
    );

    // 2. Configure TCR (Translation Control Register)
    // TG0=64KB, T0SZ=22 (42-bit VA space), IPS=48 bits
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

    // 3. Populate Translation Table
    // Get raw pointer from the safe wrapper
    let table = &mut *KERNEL_TABLE.0.get();

    // Map 0x0 -> 512MB as Normal Memory (Kernel lives here)
    table.entries[0] = create_block(0x0000_0000, MAIR_NORMAL);

    // Map RP1 IO region (Starts around 0x1f_0000_0000)
    // 0x1f_0000_0000 / 512MB = Index 124
    let io_base = 0x1f_0000_0000;
    let io_idx = (io_base / GRANULE_64KB_BLOCK_SIZE) as usize;
    
    // Map 1GB of IO space (2 blocks)
    table.entries[io_idx] = create_block(io_base, MAIR_DEVICE);
    table.entries[io_idx+1] = create_block(io_base + GRANULE_64KB_BLOCK_SIZE, MAIR_DEVICE);

    // 4. Set TTBR0 (Translation Table Base Register)
    TTBR0_EL1.set_baddr(table.entries.as_ptr() as u64);

    // 5. Enable MMU
    barrier::isb(barrier::SY);
    SCTLR_EL1.modify(SCTLR_EL1::M::Enable + SCTLR_EL1::C::Cacheable + SCTLR_EL1::I::Cacheable);
    barrier::isb(barrier::SY);
}
