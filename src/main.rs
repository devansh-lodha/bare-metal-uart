#![no_std]
#![no_main]

use core::arch::global_asm;
use core::panic::PanicInfo;
use core::ptr::{read_volatile, write_volatile};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

global_asm!(
    ".section .text.boot",
    ".global _start",
    "_start:",
    // --- Step 1: Read CPU ID to check if we are the primary core (Core 0) ---
    "   mrs  x0, mpidr_el1",
    "   and  x0, x0, #3", 
    "   cbz  x0, .L_core0_main", 
    // --- If not Core 0, put this core into a holding pattern ---
    ".L_secondary_core_sleep:",
    "   wfe",
    "   b    .L_secondary_core_sleep",

    ".L_core0_main:",
    // --- Step 2: Set up a temporary stack right below the kernel load address ---
    "   ldr  x0, =_start",
    "   mov  sp, x0",

    // --- Step 3: Clean and Invalidate the Data Cache (D-Cache) ---
    "   mrs  x0, clidr_el1",      
    "   and  x0, x0, #0x7000000", 
    "   lsr  x0, x0, #23",        
    "   cbz  x0, .L_finished_cache_cleaning", 
    "   mov  x10, x0",            

    "   mov  x1, #0",             
    ".L_cache_level_loop:",
    "   add  x2, x1, x1, lsr #1", 
    "   lsl  x0, x1, #1",     // x0 = level * 2 (for CSSELR shift)
    "   msr  csselr_el1, x0",     
    "   isb",                    
    "   mrs  x0, ccsidr_el1",     
    "   and  x2, x0, #7",         
    "   add  x2, x2, #4",         
    "   ubfx x3, x0, #3, #10",    
    "   ubfx x4, x0, #13, #15",   
    
    ".L_cache_set_loop:",
    "   mov  x5, x3",
    ".L_cache_way_loop:",
    "   lsl  x6, x5, #30",
    "   lsl  x7, x1, #1",
    "   lsl  x9, x4, x2",
    "   orr  x6, x6, x7",
    "   orr  x6, x6, x9",
    "   dc   cisw, x6",
    "   subs x5, x5, #1",
    "   bge  .L_cache_way_loop",
    "   subs x4, x4, #1",
    "   bge  .L_cache_set_loop",

    "   add  x1, x1, #1",
    "   cmp  x1, x10",
    "   b.lt .L_cache_level_loop",

    ".L_finished_cache_cleaning:",
    "   dsb  ish",

    // --- Step 4: Disable MMU, Caches, and Invalidate TLBs ---
    "   ic   iallu",
    "   tlbi vmalle1is",
    "   dsb  ish",
    "   isb",

    "   mrs  x0, sctlr_el1",
    "   bic  x0, x0, #(1 << 0)",
    "   bic  x0, x0, #(1 << 2)",
    "   bic  x0, x0, #(1 << 12)",
    "   msr  sctlr_el1, x0",
    "   isb",

    // --- Step 5: Jump to Rust code after clearing BSS ---
    "   ldr  x0, =__bss_start",
    "   ldr  x1, =__bss_end",
    "   sub  x1, x1, x0",
    "   bl   memset_zero",
    "   bl   rust_main",
    
    // --- Loop forever if rust_main returns ---
    "loop:",
    "   wfe",
    "   b    loop",

    // Helper function to zero memory for the BSS section.
    "memset_zero:",
    "   cbz  x1, .L_memset_zero_end",
    "   str  xzr, [x0], #8",
    "   sub  x1, x1, #8",
    "   b    memset_zero",
    ".L_memset_zero_end:",
    "   ret",
);


// Physical addresses calculated from BAR + Offset
const UART0_BASE: u64 = 0x1F00030000;

// PL011 Register Offsets (from bcm2711-peripherals.pdf, Table 172)
const UART_DR_OFFSET:   u64 = 0x00;
const UART_FR_OFFSET:   u64 = 0x18;
const UART_IBRD_OFFSET: u64 = 0x24;
const UART_FBRD_OFFSET: u64 = 0x28;
const UART_LCRH_OFFSET: u64 = 0x2c;
const UART_CR_OFFSET:   u64 = 0x30;
const UART_ICR_OFFSET:  u64 = 0x44;

// Flag Register (FR) bits
const UART_FR_TXFF: u32 = 1 << 5; // Transmit FIFO Full

// Line Control Register (LCRH) bits
const UART_LCRH_WLEN8: u32 = 0b11 << 5; // 8-bit word length
const UART_LCRH_FEN:   u32 = 1 << 4;    // Enable FIFOs

// Control Register (CR) bits
const UART_CR_UARTEN: u32 = 1 << 0; // UART Enable
const UART_CR_TXE:    u32 = 1 << 8; // Transmit Enable
const UART_CR_RXE:    u32 = 1 << 9; // Receive Enable

/// Initializes UART0 for 115200 baud communication.
fn uart_init() {
    unsafe {
        // --- Step 1: Disable the UART to configure it ---
        write_volatile((UART0_BASE + UART_CR_OFFSET) as *mut u32, 0);

        // --- Step 2: Set the Baud Rate ---
        // Formula: BAUDDIV = UARTCLK / (16 * Baud Rate)
        // We assume UARTCLK is 48MHz from the DTS.
        // BAUDDIV = 48,000,000 / (16 * 115200) = 26.0416...
        // IBRD = 26
        // FBRD = integer((0.0416 * 64) + 0.5) = 3
        write_volatile((UART0_BASE + UART_IBRD_OFFSET) as *mut u32, 26);
        write_volatile((UART0_BASE + UART_FBRD_OFFSET) as *mut u32, 3);

        // --- Step 3: Configure the Line Control ---
        // Enable FIFOs and set 8-bit word length, no parity, 1 stop bit.
        write_volatile((UART0_BASE + UART_LCRH_OFFSET) as *mut u32, UART_LCRH_FEN | UART_LCRH_WLEN8);

        // --- Step 4: Clear all interrupts ---
        write_volatile((UART0_BASE + UART_ICR_OFFSET) as *mut u32, 0x7FF);

        // --- Step 5: Enable the UART, TX, and RX ---
        write_volatile((UART0_BASE + UART_CR_OFFSET) as *mut u32, UART_CR_UARTEN | UART_CR_TXE | UART_CR_RXE);
    }
}

/// Transmits a single character over UART0.
fn uart_putc(c: char) {
    unsafe {
        // Loop until the transmit FIFO is no longer full.
        while read_volatile((UART0_BASE + UART_FR_OFFSET) as *mut u32) & UART_FR_TXFF != 0 {}
        // Write the character to the data register.
        write_volatile((UART0_BASE + UART_DR_OFFSET) as *mut u32, c as u32);
    }
}

/// Transmits a string slice over UART0.
fn uart_puts(s: &str) {
    for c in s.chars() {
        uart_putc(c);
    }
}


#[no_mangle]
pub extern "C" fn rust_main() -> ! {
    // --- GPIO Address and Bitmask Constants (from previous code) ---
    const BCM2712_PCIE_BASE: u64 = 0x1f00000000;
    const IO_BANK0_BASE: u64 = BCM2712_PCIE_BASE + 0xd0000;
    const PADS_BANK0_BASE: u64 = BCM2712_PCIE_BASE + 0xf0000;
    
    // GPIO 14 (TXD0)
    const GPIO14_CTRL_ADDR: u64 = IO_BANK0_BASE + 0x074;
    const PADS14_CTRL_ADDR: u64 = PADS_BANK0_BASE + 0x03c;
    // GPIO 15 (RXD0)
    const GPIO15_CTRL_ADDR: u64 = IO_BANK0_BASE + 0x07c;
    const PADS15_CTRL_ADDR: u64 = PADS_BANK0_BASE + 0x040;

    // --- Pad Control Bits ---
    const BIT_PADS_OD: u32 = 1 << 7;  // Output Disable
    const BIT_PADS_IE: u32 = 1 << 6;  // Input Enable
    const BIT_PADS_PUE: u32 = 1 << 3; // Pull-Up Enable

    // --- IO Control Fields ---
    const FUNCSEL_FIELD_LSB: u32 = 0;
    const FUNCSEL_FIELD_MASK: u32 = 0x1F << FUNCSEL_FIELD_LSB;
    const FUNC_UART0: u32 = 4;

    unsafe {
        // --- Step 1: Configure GPIO Pins (The Two Locks) ---
        
        // Configure GPIO 14 as UART0 TX
        let mut pads14_val = read_volatile(PADS14_CTRL_ADDR as *mut u32);
        pads14_val &= !BIT_PADS_OD; // LOCK 1: Enable output driver
        write_volatile(PADS14_CTRL_ADDR as *mut u32, pads14_val);

        let mut io14_val = read_volatile(GPIO14_CTRL_ADDR as *mut u32);
        io14_val &= !FUNCSEL_FIELD_MASK; // Clear the function field
        io14_val |= FUNC_UART0 << FUNCSEL_FIELD_LSB; // LOCK 2: Set function to UART0
        write_volatile(GPIO14_CTRL_ADDR as *mut u32, io14_val);

        // Configure GPIO 15 as UART0 RX
        let mut pads15_val = read_volatile(PADS15_CTRL_ADDR as *mut u32);
        pads15_val |= BIT_PADS_IE; // LOCK 1: Enable input buffer
        pads15_val |= BIT_PADS_PUE; // Enable pull-up as per DTS
        write_volatile(PADS15_CTRL_ADDR as *mut u32, pads15_val);

        let mut io15_val = read_volatile(GPIO15_CTRL_ADDR as *mut u32);
        io15_val &= !FUNCSEL_FIELD_MASK; // Clear the function field
        io15_val |= FUNC_UART0 << FUNCSEL_FIELD_LSB; // LOCK 2: Set function to UART0
        write_volatile(GPIO15_CTRL_ADDR as *mut u32, io15_val);
        
        // --- Step 2: Initialize and use the UART ---
        uart_init();
        uart_puts("Hello, world from bare-metal Rust on Raspberry Pi 5!\r\n");
    }

    loop {}
}