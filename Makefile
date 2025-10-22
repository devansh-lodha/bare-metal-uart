# NOTE: The indented lines below MUST start with a Tab character, not spaces.

PROJECT_NAME = bare_metal_led
TARGET_TRIPLE = aarch64-unknown-none
CROSS_COMPILE = aarch64-elf-

GDB = $(CROSS_COMPILE)gdb
OPENOCD = openocd
OBJCOPY = $(CROSS_COMPILE)objcopy

KERNEL_ELF_DEBUG = target/$(TARGET_TRIPLE)/debug/$(PROJECT_NAME)
HALT_STUB_IMG = halt_stub/halt_stub.img

OPENOCD_INTERFACE_CFG = debug-helper/cmsis-dap.cfg
OPENOCD_TARGET_CFG = debug-helper/raspberrypi5.cfg

.PHONY: all build openocd-pi5 gdb-pi5 clean halt_stub

all: build

build:
	@echo "--- Building Rust Kernel for Debug ---"
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
	@$(GDB) -x gdb-init.txt $(KERNEL_ELF_DEBUG)

clean:
	@cargo clean
	@rm -f $(HALT_STUB_IMG)