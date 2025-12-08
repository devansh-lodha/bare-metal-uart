PROJECT_NAME = bare_metal_uart
TARGET_TRIPLE = aarch64-unknown-none
CROSS_COMPILE = aarch64-elf-

GDB = $(CROSS_COMPILE)gdb
OPENOCD = openocd

KERNEL_ELF_DEBUG = target/$(TARGET_TRIPLE)/debug/$(PROJECT_NAME)
HALT_STUB_IMG = halt_stub/halt_stub.img

OPENOCD_INTERFACE_CFG = debug/cmsis-dap.cfg
OPENOCD_TARGET_CFG = debug/raspberrypi5.cfg
GDB_INIT_FILE = debug/gdb-init.txt

.PHONY: all build openocd-pi5 gdb-pi5 clean halt_stub

all: build

build:
	@echo "--- Building Rust Kernel ---"
	@cargo build

halt_stub: $(HALT_STUB_IMG)

$(HALT_STUB_IMG): halt_stub/start.s halt_stub/linker.ld
	@echo "--- Building Halt Stub ---"
	@$(CROSS_COMPILE)as -o halt_stub/start.o halt_stub/start.s
	@$(CROSS_COMPILE)ld -T halt_stub/linker.ld -o halt_stub/halt_stub.elf halt_stub/start.o
	@$(CROSS_COMPILE)objcopy -O binary halt_stub/halt_stub.elf $(HALT_STUB_IMG)
	@rm -f halt_stub/start.o halt_stub/halt_stub.elf

openocd-pi5:
	@$(OPENOCD) -f $(OPENOCD_INTERFACE_CFG) -f $(OPENOCD_TARGET_CFG)

gdb-pi5: build
	@echo "--- Launching GDB ---"
	@$(GDB) -x $(GDB_INIT_FILE) $(KERNEL_ELF_DEBUG)

clean:
	@cargo clean
	@rm -f $(HALT_STUB_IMG)
