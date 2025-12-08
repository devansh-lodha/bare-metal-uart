use aarch64_cpu::asm;
use tock_registers::{
    interfaces::{Readable, Writeable, ReadWriteable},
    register_bitfields, register_structs,
    registers::{ReadWrite, ReadOnly},
};

// -----------------------------------------------------------------------------
// Register Definitions
// -----------------------------------------------------------------------------

// GPIO / PADS registers for RPi5 (RP1 Chip)
register_bitfields! {
    u32,
    PADS_CTRL [
        OD  OFFSET(7) NUMBITS(1) [], // Output Disable
        IE  OFFSET(6) NUMBITS(1) [], // Input Enable
        PUE OFFSET(3) NUMBITS(1) []  // Pull Up Enable
    ],
    GPIO_CTRL [
        FUNCSEL OFFSET(0) NUMBITS(5) [] // Function Select (UART0 = 4)
    ]
}

register_structs! {
    #[allow(non_snake_case)]
    pub Rp1PadBank {
        (0x00 => _reserved0),
        // PADS_GPIO14 is at offset 0x3C relative to PADS_BANK0
        (0x3C => pub PADS_GPIO14: ReadWrite<u32, PADS_CTRL::Register>),
        // PADS_GPIO15 is at offset 0x40. Since u32 is 4 bytes, 0x3C+4 = 0x40.
        // This is contiguous, so no padding needed here.
        (0x40 => pub PADS_GPIO15: ReadWrite<u32, PADS_CTRL::Register>),
        (0x44 => @END),
    },

    #[allow(non_snake_case)]
    pub Rp1GioBank {
        (0x00 => _reserved0),
        // GPIO14_CTRL is at offset 0x74 relative to IO_BANK0
        (0x74 => pub GPIO14_CTRL: ReadWrite<u32, GPIO_CTRL::Register>),
        
        // GPIO14 register ends at 0x78. GPIO15 starts at 0x7C.
        // We MUST reserve the 4-byte gap to satisfy tock-registers alignment checks.
        (0x78 => _reserved1), 

        (0x7C => pub GPIO15_CTRL: ReadWrite<u32, GPIO_CTRL::Register>),
        (0x80 => @END),
    }
}

// PL011 UART Registers (Standard ARM PrimeCell UART)
register_bitfields! {
    u32,
    FR [
        TXFF OFFSET(5) NUMBITS(1) [], // Transmit FIFO Full
        RXFE OFFSET(4) NUMBITS(1) []  // Receive FIFO Empty
    ],
    CR [
        RXE    OFFSET(9) NUMBITS(1) [], // Receive Enable
        TXE    OFFSET(8) NUMBITS(1) [], // Transmit Enable
        UARTEN OFFSET(0) NUMBITS(1) []  // UART Enable
    ],
    LCRH [
        WLEN OFFSET(5) NUMBITS(2) [
            EightBit = 0b11,
            SevenBit = 0b10
        ],
        FEN  OFFSET(4) NUMBITS(1) []  // FIFO Enable
    ]
}

register_structs! {
    #[allow(non_snake_case)]
    pub Pl011Uart {
        (0x00 => pub DR: ReadWrite<u32>),                 // Data Register
        (0x04 => _reserved0),
        (0x18 => pub FR: ReadOnly<u32, FR::Register>),    // Flag Register
        (0x1C => _reserved1),
        (0x24 => pub IBRD: ReadWrite<u32>),               // Integer Baud Rate Divisor
        (0x28 => pub FBRD: ReadWrite<u32>),               // Fractional Baud Rate Divisor
        (0x2C => pub LCRH: ReadWrite<u32, LCRH::Register>), // Line Control Register
        (0x30 => pub CR: ReadWrite<u32, CR::Register>),   // Control Register
        (0x34 => _reserved2),
        (0x44 => pub ICR: ReadWrite<u32>),                // Interrupt Clear Register
        (0x48 => @END),
    }
}

// -----------------------------------------------------------------------------
// Constants & Static Pointers
// -----------------------------------------------------------------------------

const RP1_PERIPH_BASE: u64 = 0x1f_0000_0000;
const IO_BANK0_OFFSET: u64 = 0xd0000;
const PADS_BANK0_OFFSET: u64 = 0xf0000;
const UART0_OFFSET: u64 = 0x03_0000;

// Hardware Pointers
const UART0: *const Pl011Uart = (RP1_PERIPH_BASE + UART0_OFFSET) as *const Pl011Uart;
const PADS:  *const Rp1PadBank = (RP1_PERIPH_BASE + PADS_BANK0_OFFSET) as *const Rp1PadBank;
const GPIO:  *const Rp1GioBank = (RP1_PERIPH_BASE + IO_BANK0_OFFSET) as *const Rp1GioBank;

// -----------------------------------------------------------------------------
// Public API
// -----------------------------------------------------------------------------

/// Initialize UART0 on GPIO 14/15 (RPi 5 Standard Header)
///
/// # Safety
/// This function writes directly to physical memory addresses to configure hardware.
/// It should only be called once during kernel initialization.
pub unsafe fn init() {
    let uart = &*UART0;
    let pads = &*PADS;
    let gpio = &*GPIO;

    // 1. Disable UART before configuration
    uart.CR.set(0);

    // 2. Setup GPIO Pins (RP1 Specifics)
    // GPIO14 (TX): Function 4 (UART0), Output Disable Cleared
    pads.PADS_GPIO14.modify(PADS_CTRL::OD::CLEAR);
    gpio.GPIO14_CTRL.write(GPIO_CTRL::FUNCSEL.val(4));

    // GPIO15 (RX): Function 4 (UART0), Input Enable Set, Pull-Up Enabled
    pads.PADS_GPIO15.modify(PADS_CTRL::IE::SET + PADS_CTRL::PUE::SET);
    gpio.GPIO15_CTRL.write(GPIO_CTRL::FUNCSEL.val(4));

    // 3. Clear Pending Interrupts
    uart.ICR.set(0x7FF);

    // 4. Set Baud Rate
    // Assumption: Base Clock is 48MHz (default for RPi bootloaders).
    // Target: 115200 Baud.
    // Calculation: 48,000,000 / (16 * 115200) = 26.0416...
    // IBRD = 26
    // FBRD = 0.0416... * 64 = 2.66... -> 3
    uart.IBRD.set(26);
    uart.FBRD.set(3);

    // 5. Line Control: 8-bit word, FIFO enabled
    uart.LCRH.write(LCRH::WLEN::EightBit + LCRH::FEN::SET);

    // 6. Re-enable UART, Transmit, and Receive
    uart.CR.write(CR::UARTEN::SET + CR::TXE::SET + CR::RXE::SET);
}

/// Send a single character
pub fn putc(c: char) {
    let uart = unsafe { &*UART0 };
    // Wait until Transmit FIFO is NOT Full (TXFF)
    while uart.FR.is_set(FR::TXFF) {
        asm::nop();
    }
    uart.DR.set(c as u32);
}

/// Receive a single character
pub fn getc() -> char {
    let uart = unsafe { &*UART0 };
    // Wait until Receive FIFO is NOT Empty (RXFE)
    while uart.FR.is_set(FR::RXFE) {
        asm::nop();
    }
    (uart.DR.get() & 0xFF) as u8 as char
}

/// Print a string
pub fn console_print(s: &str) {
    for c in s.chars() {
        putc(c);
    }
}

/// Print a 64-bit integer as Hexadecimal (e.g., "1A")
pub fn print_hex(mut val: u64) {
    if val == 0 {
        console_print("0");
        return;
    }

    let mut buffer = [0u8; 16];
    let mut idx = 0;

    while val > 0 {
        let digit = (val % 16) as u8;
        if digit < 10 {
            buffer[idx] = digit + b'0';
        } else {
            buffer[idx] = digit - 10 + b'A';
        }
        val /= 16;
        idx += 1;
    }

    while idx > 0 {
        idx -= 1;
        putc(buffer[idx] as char);
    }
}

/// Print a 64-bit integer as Decimal (e.g., "42")
pub fn print_dec(mut val: u64) {
    if val == 0 {
        console_print("0");
        return;
    }

    let mut buffer = [0u8; 20]; // u64::MAX is approx 1.8e19, so 20 chars is safe
    let mut idx = 0;

    while val > 0 {
        let digit = (val % 10) as u8;
        buffer[idx] = digit + b'0';
        val /= 10;
        idx += 1;
    }

    while idx > 0 {
        idx -= 1;
        putc(buffer[idx] as char);
    }
}
