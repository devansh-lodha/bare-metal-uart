// src/panic.rs
use core::panic::PanicInfo;
use crate::{println, drivers::uart};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe { 
        core::arch::asm!("msr daifset, #2");
        uart::init_panic(); 
    }
    
    println!("\n\n!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
    println!("             KERNEL PANIC                ");
    println!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
    
    if let Some(loc) = info.location() {
        println!("Location: {}:{}", loc.file(), loc.line());
    }
    
    // Fix: Display the message directly
    println!("Message:  {}", info.message());
    
    loop {
        unsafe { core::arch::asm!("wfe") };
    }
}
