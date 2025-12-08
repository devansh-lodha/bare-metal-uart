SECTIONS
{
    . = 0x80000;

    .text : { 
        KEEP(*(.text.boot)) 
        *(.text .text.*) 
    }
    
    .rodata : { *(.rodata .rodata.*) }
    
    .data : { *(.data .data.*) }

    /* 
     * MMU REQUIREMENT: 
     * Align BSS to 64KB (0x10000). 
     * We will place our Translation Tables at the start of BSS.
     */
    . = ALIGN(0x10000); 
    
    .bss : {
        __bss_start = .;
        *(.bss .bss.*)
        . = ALIGN(16);
        __bss_end = .;
    }
}
