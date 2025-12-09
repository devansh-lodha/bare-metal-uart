// src/console.rs
use core::fmt;
use crate::drivers::uart;

struct Console;

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        uart::write_str(s);
        Ok(())
    }
}

pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    Console.write_fmt(args).unwrap();
}

/// Prints to the host console using the UART driver.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::console::_print(format_args!($($arg)*)));
}

/// Prints to the host console using the UART driver, with a newline.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
