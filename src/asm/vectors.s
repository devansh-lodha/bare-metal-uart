// src/asm/vectors.s

.section .text
.global vectors

// The Vector Table must be aligned to 2KB (2^11)
.align 11
vectors:
    // -------------------------------------------------------------------------
    // Current EL with SP0 (Not used)
    // -------------------------------------------------------------------------
    .align 7
    b   .
    .align 7
    b   .
    .align 7
    b   .
    .align 7
    b   .

    // -------------------------------------------------------------------------
    // Current EL with SPx (This is where we live: EL1h)
    // -------------------------------------------------------------------------
    .align 7
    b   .                   // Synchronous (Exceptions/Traps)
    .align 7
    b   irq_handler_asm     // IRQ (Interrupts) -> JUMP TO HANDLER
    .align 7
    b   .                   // FIQ (Fast Interrupts)
    .align 7
    b   .                   // SError

    // -------------------------------------------------------------------------
    // Lower EL (Not used yet)
    // -------------------------------------------------------------------------
    .align 7
    b   .
    .align 7
    b   .
    .align 7
    b   .
    .align 7
    b   .
    .align 7
    b   .
    .align 7
    b   .
    .align 7
    b   .
    .align 7
    b   .

// -----------------------------------------------------------------------------
// IRQ Context Saver
// -----------------------------------------------------------------------------
irq_handler_asm:
    // 1. Make space on stack (32 registers * 8 bytes = 256 bytes)
    sub     sp, sp, #256

    // 2. Save General Purpose Registers (x0-x29)
    stp     x0, x1, [sp, #16 * 0]
    stp     x2, x3, [sp, #16 * 1]
    stp     x4, x5, [sp, #16 * 2]
    stp     x6, x7, [sp, #16 * 3]
    stp     x8, x9, [sp, #16 * 4]
    stp     x10, x11, [sp, #16 * 5]
    stp     x12, x13, [sp, #16 * 6]
    stp     x14, x15, [sp, #16 * 7]
    stp     x16, x17, [sp, #16 * 8]
    stp     x18, x19, [sp, #16 * 9]
    stp     x20, x21, [sp, #16 * 10]
    stp     x22, x23, [sp, #16 * 11]
    stp     x24, x25, [sp, #16 * 12]
    stp     x26, x27, [sp, #16 * 13]
    stp     x28, x29, [sp, #16 * 14]
    
    // 3. Save Special Registers (Link Register, ELR, SPSR)
    // We use x21, x22, x23 as temps since they are already saved on stack
    mrs     x21, sp_el0
    mrs     x22, elr_el1
    mrs     x23, spsr_el1
    
    // Save Link Register (x30) and SP_EL0
    stp     x30, x21, [sp, #16 * 15]
    // Save ELR and SPSR (Overwrite empty slot at top for alignment or safety)
    // Actually, let's put them in the last slot logic.
    // We saved x0..x29 (30 regs). x30 and sp_el0 make 32. 
    // We need more space for ELR/SPSR if we want to be safe, but usually
    // struct is: [x0..x29, x30, elr, spsr]. 
    // Let's stick to the previous working simple layout you had:
    
    stp     x22, x23, [sp, #256 - 16] // Save ELR, SPSR at the very top

    // 4. Call Rust
    bl      handle_irq_router

    // 5. Restore Special Registers
    ldp     x22, x23, [sp, #256 - 16]
    msr     elr_el1, x22
    msr     spsr_el1, x23
    
    // 6. Restore Link Register and SP_EL0
    ldp     x30, x21, [sp, #16 * 15]
    msr     sp_el0, x21

    // 7. Restore General Purpose Registers
    ldp     x28, x29, [sp, #16 * 14]
    ldp     x26, x27, [sp, #16 * 13]
    ldp     x24, x25, [sp, #16 * 12]
    ldp     x22, x23, [sp, #16 * 11]
    ldp     x20, x21, [sp, #16 * 10]
    ldp     x18, x19, [sp, #16 * 9]
    ldp     x16, x17, [sp, #16 * 8]
    ldp     x14, x15, [sp, #16 * 7]
    ldp     x12, x13, [sp, #16 * 6]
    ldp     x10, x11, [sp, #16 * 5]
    ldp     x8, x9, [sp, #16 * 4]
    ldp     x6, x7, [sp, #16 * 3]
    ldp     x4, x5, [sp, #16 * 2]
    ldp     x2, x3, [sp, #16 * 1]
    ldp     x0, x1, [sp, #16 * 0]

    // 8. Clean stack and Return
    add     sp, sp, #256
    eret
