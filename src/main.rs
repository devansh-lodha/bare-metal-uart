#![no_std]
#![no_main]

use core::arch::global_asm;
use core::panic::PanicInfo;
use aarch64_cpu::registers::*;
use tock_registers::interfaces::Readable;

mod uart;
mod mmu;

global_asm!(include_str!("start.s"));

// -----------------------------------------------------------------------------
// Educational Helpers
// -----------------------------------------------------------------------------

/// Uses the hardware Address Translation (AT) instruction to ask the MMU:
/// "What Physical Address does this Virtual Address map to?"
/// Returns: Some(PhysicalAddress) or None if it faults (unmapped).
fn hardware_resolve_addr(va: u64) -> Option<u64> {
    use aarch64_cpu::asm::barrier;
    
    // SAFETY: AT instruction is safe to execute at EL1. 
    // It does not change memory, only updates PAR_EL1.
    unsafe {
        core::arch::asm!("at s1e1r, {}", in(reg) va);
        barrier::isb(barrier::SY);
    }

    // Read result from Physical Address Register (PAR_EL1)
    let par = PAR_EL1.get();

    // Check F bit (Bit 0). If 1, translation failed.
    if (par & 1) == 1 {
        return None;
    }

    // Extract Output Address (Bits 47:12) + add offset from VA (Bits 11:0)
    // The PAR holds the page base. We must add the page offset.
    let page_offset = va & 0xFFF; // 4KB offset mask (generic safe bet)
    let pa_base = par & 0x0000_FFFF_FFFF_F000;
    
    Some(pa_base | page_offset)
}

fn print_system_status() {
    uart::console_print("\r\n--- SYSTEM STATUS INSPECTION ---\r\n");

    // 1. EL Check
    let el = CurrentEL.read(CurrentEL::EL);
    uart::console_print(" [1] Current Exception Level: EL");
    uart::print_dec(el);
    uart::console_print("\r\n");

    // 2. PA Range
    let pa_range = ID_AA64MMFR0_EL1.read(ID_AA64MMFR0_EL1::PARange);
    uart::console_print(" [2] Physical Address Width:  ");
    match pa_range {
        0 => uart::console_print("32 bits (4GB)\r\n"),
        1 => uart::console_print("36 bits (64GB)\r\n"),
        2 => uart::console_print("40 bits (1TB)\r\n"),
        3 => uart::console_print("42 bits (4TB)\r\n"),
        4 => uart::console_print("44 bits (16TB)\r\n"),
        5 => uart::console_print("48 bits (256TB)\r\n"),
        _ => uart::console_print("Unknown\r\n"),
    }

    // 3. Cache/MMU
    let mmu_on = SCTLR_EL1.is_set(SCTLR_EL1::M);
    uart::console_print(" [3] SCTLR_EL1: ");
    if mmu_on { uart::console_print("MMU:[ON]  "); } else { uart::console_print("MMU:[OFF] "); }
    if SCTLR_EL1.is_set(SCTLR_EL1::I) { uart::console_print("I-Cache:[ON]  "); }
    if SCTLR_EL1.is_set(SCTLR_EL1::C) { uart::console_print("D-Cache:[ON]"); }
    uart::console_print("\r\n");

    // 4. Granule & VA Size
    let tg0 = TCR_EL1.read(TCR_EL1::TG0);
    uart::console_print(" [4] TCR_EL1:   Granule=");
    match tg0 {
        1 => uart::console_print("64KB"),
        0 => uart::console_print("4KB"),
        2 => uart::console_print("16KB"),
        _ => uart::console_print("?"),
    }
    
    let t0sz = TCR_EL1.read(TCR_EL1::T0SZ);
    let va_bits = 64 - t0sz;
    uart::console_print("  VirtualBits=");
    uart::print_dec(va_bits);
    uart::console_print("\r\n");

    // 5. Evidence-Based Verification
    uart::console_print(" [5] MMU Verification (AT Instruction Probe):\r\n");
    
    // Probe Kernel Entry (should be 1:1 mapped)
    let kernel_va = 0x80000;
    uart::console_print("     - Probing Kernel VA (0x80000)...  Result: PA 0x");
    if let Some(pa) = hardware_resolve_addr(kernel_va) {
        uart::print_hex(pa);
        if pa == kernel_va {
            uart::console_print(" [MATCH - Identity Mapped]\r\n");
        } else {
            uart::console_print(" [RE-MAPPED]\r\n");
        }
    } else {
        uart::console_print(" [FAULT - Not Mapped!]\r\n");
    }

    // Probe UART Base (should be mapped)
    let uart_va = 0x1f_0003_0000;
    uart::console_print("     - Probing UART VA   (0x");
    uart::print_hex(uart_va);
    uart::console_print(")...  Result: PA 0x");
    if let Some(pa) = hardware_resolve_addr(uart_va) {
        uart::print_hex(pa);
        uart::console_print(" [OK]\r\n");
    } else {
        uart::console_print(" [FAULT - Not Mapped!]\r\n");
    }

    // Probe Random High Address (should fail)
    let bad_va = 0x0FFF_FFFF_FFFF_0000; 
    uart::console_print("     - Probing Bad VA    (0x");
    uart::print_hex(bad_va);
    uart::console_print(")...  Result: ");
    if let Some(_) = hardware_resolve_addr(bad_va) {
        uart::console_print("PA Found (Unexpected)\r\n");
    } else {
        uart::console_print("Translation Fault (Expected - Secure)\r\n");
    }

    uart::console_print("--------------------------------\r\n");
}

#[no_mangle]
pub extern "C" fn rust_main() -> ! {
    unsafe {
        uart::init();
        mmu::init();
    }

    uart::console_print("\r\n\r\n");
    uart::console_print("========================================\r\n");
    uart::console_print("   RASPBERRY PI 5 - BARE METAL RUST     \r\n");
    uart::console_print("========================================\r\n");

    print_system_status();

    uart::console_print("Kernel Initialized. Echo Loop Active.\r\n");

    loop {
        let c = uart::getc();
        if c == '\r' {
            uart::console_print("\r\n");
        } else {
            uart::putc(c);
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { uart::init(); }
    uart::console_print("\r\n[KERNEL PANIC]\r\n");
    loop {
        aarch64_cpu::asm::wfe();
    }
}
