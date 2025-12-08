.section .text.boot
.global _start

_start:
    // 1. Park Secondary Cores
    mrs     x0, mpidr_el1
    and     x0, x0, #3
    cbz     x0, .L_check_el
.L_park:
    wfe
    b       .L_park

.L_check_el:
    // 2. Check EL
    mrs     x0, CurrentEL
    and     x0, x0, #0b1100
    lsr     x0, x0, #2
    cmp     x0, #2
    beq     .L_el2_entry
    b       .L_setup_stack

.L_el2_entry:
    // A. Timer Access for EL1
    mov     x0, #0b11
    msr     cnthctl_el2, x0
    msr     cntvoff_el2, xzr

    // B. AArch64 for EL1
    mov     x0, #(1 << 31)
    msr     hcr_el2, x0

    // C. Setup SCTLR_EL1
    // Res1 bits for A76/A78 + Little Endian + No MMU yet
    // 0x30C50830 is the reset default for many v8 cores.
    ldr     x0, =0x30C50830
    msr     sctlr_el1, x0

    // D. Enable FP/SIMD
    mov     x0, #(3 << 20)
    msr     cpacr_el1, x0

    // E. Return to EL1h
    mov     x0, #0x3c5
    msr     spsr_el2, x0
    adr     x0, .L_setup_stack
    msr     elr_el2, x0
    eret

.L_setup_stack:
    ldr     x0, =_start
    mov     sp, x0

    // Zero BSS
    ldr     x0, =__bss_start
    ldr     x1, =__bss_end
    sub     x1, x1, x0
    cbz     x1, .L_main
.L_bss_loop:
    str     xzr, [x0], #8
    sub     x1, x1, #8
    cbnz    x1, .L_bss_loop

.L_main:
    bl      rust_main
    b       .L_park
