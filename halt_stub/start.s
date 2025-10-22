.section ".text.boot"
.global _start
_start:
    wfe // Wait for event (low-power idle)
    b _start