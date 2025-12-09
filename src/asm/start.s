// src/asm/start.s

.section .text.boot
.global _start

_start:
    // 1. Park Secondary Cores (We only use Core 0 for now)
    mrs     x0, mpidr_el1
    and     x0, x0, #3
    cbz     x0, .L_check_el
.L_park:
    wfe
    b       .L_park

.L_check_el:
    // 2. Check current Exception Level (EL)
    mrs     x0, CurrentEL
    and     x0, x0, #0b1100
    lsr     x0, x0, #2
    cmp     x0, #2
    beq     .L_el2_entry
    // If we are already in EL1 (unlikely on bare metal), just setup stack
    b       .L_setup_stack

.L_el2_entry:
    // --- Switch from EL2 to EL1 ---

    // A. Enable Timer Access for EL1
    mov     x0, #0b11
    msr     cnthctl_el2, x0
    msr     cntvoff_el2, xzr

    // B. Set AArch64 for EL1 (HCR_EL2 bit 31)
    mov     x0, #(1 << 31)
    msr     hcr_el2, x0

    // C. Setup SCTLR_EL1 (System Control Register)
    // Little Endian, MMU disabled (for now), I-Cache disabled
    ldr     x0, =0x30C50830
    msr     sctlr_el1, x0

    // D. Enable FP/SIMD (Floating Point) access for EL1
    mov     x0, #(3 << 20)
    msr     cpacr_el1, x0

    // E. Prepare transition to EL1h (SP_EL1)
    // SPSR_EL2: D=1, A=1, I=1, F=1 (All interrupts masked), M=0101 (EL1h)
    mov     x0, #0x3c5
    msr     spsr_el2, x0

    // F. Set Return Address to our stack setup label
    adr     x0, .L_setup_stack
    msr     elr_el2, x0
    
    // G. Perform the switch
    eret

.L_setup_stack:
    // 3. Configure Stack Pointer
    // Force use of SP_EL1
    msr     spsel, #1
    
    // Set stack pointer to _start (growing downwards from 0x80000)
    // This gives us space below the kernel.
    ldr     x0, =_start
    mov     sp, x0

    // 4. Set Vector Base Address Register (VBAR)
    ldr     x0, =vectors
    msr     vbar_el1, x0

    // 5. Zero BSS Section (Critical for Rust static variables)
    ldr     x0, =__bss_start
    ldr     x1, =__bss_end
    sub     x1, x1, x0
    cbz     x1, .L_main
.L_bss_loop:
    str     xzr, [x0], #8
    sub     x1, x1, #8
    cbnz    x1, .L_bss_loop

.L_main:
    // 6. Jump to Rust
    bl      rust_main
    
    // Should never return
    b       .L_park
