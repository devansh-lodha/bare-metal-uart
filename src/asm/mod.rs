// src/asm/mod.rs

use core::arch::global_asm;

// Include the assembly files here
global_asm!(include_str!("start.s"));
global_asm!(include_str!("vectors.s"));
