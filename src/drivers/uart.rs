// src/drivers/uart.rs
//! PL011 UART Driver for RP1 (Raspberry Pi 5 Southbridge)
//!
//! # Hardware Details
//! - The UART is IP embedded inside the RP1 chip.
//! - It is accessed via the RP1 Peripheral Window at 0x1F_0003_0000.
//! - Interrupts must be cleared in *both* the UART (ICR) and the RP1 APB (ACK).

use tock_registers::{
    interfaces::{Readable, Writeable, ReadWriteable},
    register_bitfields, register_structs,
    registers::{ReadWrite, ReadOnly},
};
use core::ptr::write_volatile;

// --- Constants ---
const RP1_PERIPH_BASE: u64 = 0x1f_0000_0000;
const UART0_BASE:      u64 = RP1_PERIPH_BASE + 0x03_0000;
const PADS_BASE:       u64 = RP1_PERIPH_BASE + 0x0f_0000;
const GPIO_BASE:       u64 = RP1_PERIPH_BASE + 0x0d_0000;
const UART0_IRQ_VECTOR: usize = 25;

register_bitfields! {
    u32,
    PADS_CTRL [ OD OFFSET(7) NUMBITS(1) [], IE OFFSET(6) NUMBITS(1) [], PUE OFFSET(3) NUMBITS(1) [] ],
    GPIO_CTRL [ FUNCSEL OFFSET(0) NUMBITS(5) [] ],
    FR [ TXFF OFFSET(5) NUMBITS(1) [], RXFE OFFSET(4) NUMBITS(1) [] ],
    CR [ RXE OFFSET(9) NUMBITS(1) [], TXE OFFSET(8) NUMBITS(1) [], UARTEN OFFSET(0) NUMBITS(1) [] ],
    LCRH [ WLEN OFFSET(5) NUMBITS(2) [ EightBit = 0b11 ], FEN OFFSET(4) NUMBITS(1) [] ],
    IMSC [ RXIM OFFSET(4) NUMBITS(1) [], RTIM OFFSET(6) NUMBITS(1) [] ],
    MIS [ RXMIS OFFSET(4) NUMBITS(1) [], RTMIS OFFSET(6) NUMBITS(1) [] ],
    ICR [ RXIC OFFSET(4) NUMBITS(1) [], RTIC OFFSET(6) NUMBITS(1) [] ]
}

register_structs! {
    #[allow(non_snake_case)]
    pub Rp1PadBank {
        (0x00 => _reserved0),
        (0x3C => pub GPIO14: ReadWrite<u32, PADS_CTRL::Register>), 
        (0x40 => pub GPIO15: ReadWrite<u32, PADS_CTRL::Register>), 
        (0x44 => @END),
    },
    #[allow(non_snake_case)]
    pub Rp1GioBank {
        (0x00 => _reserved0),
        (0x74 => pub GPIO14_CTRL: ReadWrite<u32, GPIO_CTRL::Register>),
        (0x78 => _reserved1),
        (0x7C => pub GPIO15_CTRL: ReadWrite<u32, GPIO_CTRL::Register>),
        (0x80 => @END),
    },
    #[allow(non_snake_case)]
    pub Pl011Uart {
        (0x00 => pub DR: ReadWrite<u32>),
        (0x04 => _reserved0),
        (0x18 => pub FR: ReadOnly<u32, FR::Register>),
        (0x1C => _reserved1),
        (0x24 => pub IBRD: ReadWrite<u32>),
        (0x28 => pub FBRD: ReadWrite<u32>),
        (0x2C => pub LCRH: ReadWrite<u32, LCRH::Register>),
        (0x30 => pub CR: ReadWrite<u32, CR::Register>),
        (0x34 => _reserved2),
        (0x38 => pub IMSC: ReadWrite<u32, IMSC::Register>),
        (0x3C => _reserved3),
        (0x40 => pub MIS: ReadOnly<u32, MIS::Register>),
        (0x44 => pub ICR: ReadWrite<u32, ICR::Register>),
        (0x48 => @END),
    }
}

// --- Implementation ---

pub unsafe fn init() {
    let uart = &*(UART0_BASE as *const Pl011Uart);
    let pads = &*(PADS_BASE as *const Rp1PadBank);
    let gpio = &*(GPIO_BASE as *const Rp1GioBank);

    uart.CR.set(0);
    uart.ICR.set(0x7FF);

    // Configure GPIO 14/15 for UART (Alt Func 4)
    pads.GPIO14.modify(PADS_CTRL::OD::CLEAR); 
    gpio.GPIO14_CTRL.write(GPIO_CTRL::FUNCSEL.val(4));
    pads.GPIO15.modify(PADS_CTRL::IE::SET + PADS_CTRL::PUE::SET); 
    gpio.GPIO15_CTRL.write(GPIO_CTRL::FUNCSEL.val(4));

    // Baud Rate Calculation:
    // With RP1 Clock at 50MHz (approx), IBRD=26, FBRD=3 yields ~115200 baud.
    // 50,000,000 / (16 * 115200) = 27.12 -> IBRD=27
    // Adjust slightly for actual crystal variances -> 26/3 is a standard value.
    uart.IBRD.set(26);
    uart.FBRD.set(3);
    
    // 8-bit, Enable FIFO
    uart.LCRH.write(LCRH::WLEN::EightBit + LCRH::FEN::SET);
    
    // Enable UART, TX, RX
    uart.CR.write(CR::UARTEN::SET + CR::TXE::SET + CR::RXE::SET);
}

pub unsafe fn init_panic() {
    let uart = &*(UART0_BASE as *const Pl011Uart);
    uart.CR.write(CR::UARTEN::SET + CR::TXE::SET);
}

pub unsafe fn enable_rx_interrupt() {
    let uart = &*(UART0_BASE as *const Pl011Uart);
    uart.IMSC.write(IMSC::RXIM::SET + IMSC::RTIM::SET);
}

pub fn putc(c: char) {
    let uart = unsafe { &*(UART0_BASE as *const Pl011Uart) };
    while uart.FR.is_set(FR::TXFF) { core::hint::spin_loop(); }
    uart.DR.set(c as u32);
}

pub fn write_str(s: &str) {
    for c in s.chars() {
        if c == '\n' { putc('\r'); }
        putc(c);
    }
}

pub unsafe fn handle_irq() {
    let uart = &*(UART0_BASE as *const Pl011Uart);
    let pending = uart.MIS.extract();
    
    if pending.is_set(MIS::RXMIS) || pending.is_set(MIS::RTMIS) {
        // Drain FIFO and Echo Back
        while !uart.FR.is_set(FR::RXFE) {
            let c = (uart.DR.get() & 0xFF) as u8 as char;
            
            // Handle Return Key for natural typing
            if c == '\r' {
                putc('\r');
                putc('\n');
            } else {
                putc(c);
            }
        }

        // 1. Clear Interrupt at UART
        uart.ICR.write(ICR::RXIC::SET + ICR::RTIC::SET);

        // 2. Acknowledge at RP1 Southbridge (Specific to this chip)
        // We write 0xD to the APB Config register for Vector 25.
        // This re-arms the MSI-X logic.
        let rp1_apb_base = 0x1f_0010_8000 as *mut u32;
        let ctrl_reg = rp1_apb_base.add(0x008/4).add(UART0_IRQ_VECTOR);
        write_volatile(ctrl_reg, 0xD); 
    }
}
