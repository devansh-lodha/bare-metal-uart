// src/main.rs
#![no_std]
#![no_main]

mod asm; 
mod console;
mod cpu;
mod drivers;
mod panic;

#[no_mangle]
pub extern "C" fn rust_main() -> ! {
    unsafe {
        cpu::mmu::init();
        drivers::uart::init();
    }
    
    println!("\n========================================");
    println!("   RASPBERRY PI 5 - BARE METAL KERNEL   ");
    println!("========================================");
    println!("[INFO] Foundation initialized.");
    
    unsafe {
        drivers::pcie::init();
        drivers::gic::init();
        
        println!("[INFO] Unmasking CPU IRQ (DAIF)...");
        core::arch::asm!("msr daifclr, #2"); 
        
        drivers::uart::enable_rx_interrupt();
    }

    println!("[SUCCESS] System Ready. Entering WFI Loop...");

    loop {
        unsafe { core::arch::asm!("wfi") }; 
    }
}
