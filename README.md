# Raspberry Pi 5 Bare Metal Kernel: Interrupt-Driven UART

## Overview

This project implements a bare-metal Operating System kernel in Rust for the Raspberry Pi 5 (BCM2712). Unlike previous Raspberry Pi generations where peripherals were integrated directly into the SoC's MMIO space, the Raspberry Pi 5 utilizes a disaggregated architecture. Peripherals are offloaded to the **RP1 Southbridge**, connected via a PCIe Gen2 link.

This kernel demonstrates the complete initialization of this complex pipeline, establishing an interrupt-driven console (UART) by configuring the PCIe Root Complex, the RP1 MSI-X capabilities, the BCM2712 Machine Interrupt Peripheral (MIP), and the GICv2 Interrupt Controller.

## System Architecture

The system is divided into two distinct domains connected by a 4-lane PCIe link:

1.  **Northbridge (BCM2712):**
    *   **CPU:** Quad-core ARM Cortex-A76 (ARMv8.2-A).
    *   **Root Complex (RC):** PCIe Gen2 Controller.
    *   **MIP:** Machine Interrupt Peripheral (Routes external signals to the CPU).
    *   **GICv2:** Generic Interrupt Controller (Distributes IRQs to cores).

2.  **Southbridge (RP1):**
    *   **Peripherals:** PL011 UART, GPIO, USB, Ethernet.
    *   **Bus Master:** Capable of issuing PCIe Memory Write transactions (MSI-X).

### The Interrupt Pipeline

A successful UART interrupt on the Pi 5 requires a coordinated configuration across five distinct hardware subsystems. The signal propagation flow is as follows:

1.  **Event Generation:** The PL011 UART (RP1) asserts a receive interrupt condition.
2.  **MSI-X Packetization:** The RP1 Interrupt Controller generates a PCIe Memory Write transaction.
    *   *Address:* `0xFFFF_F000` (Configured in RP1 MSI-X Table).
    *   *Data:* `0x0000_0000`.
3.  **PCIe Transport:** The packet traverses the PCIe bus to the BCM2712 Root Complex.
4.  **Address Translation:** The Root Complex matches the address against **Inbound Window 2 (BAR2)** and translates it to the System Physical Address.
    *   *Input:* `0xFFFF_F000` (PCIe Bus Address).
    *   *Output:* `0x10_0013_0000` (MIP Doorbell Address).
5.  **MIP Routing:** The MIP receives the write at Input 0. It asserts **SPI 160** to the GIC.
6.  **GIC Arbitration:** The GICv2 Distributor forwards the interrupt to CPU Core 0 based on priority and group configuration.
7.  **Exception Handling:** The CPU enters the Exception Vector Table (EL1), context switches, and executes the Rust ISR.

## Implementation Details

### 1. PCIe Root Complex Configuration
The BCM2712 Root Complex defaults to a secure state where inbound transactions (Southbridge $\to$ CPU) are blocked.
*   **System Core Bus (SCB) Access:** Bit 12 of `PCIE_MISC_MISC_CTRL` (`0x10_0012_4008`) must be set to enable inbound traffic.
*   **Inbound Window (BAR2):** The RP1 cannot natively address the high peripheral range (`0x10_XXXX_XXXX`). An inbound translation window is opened:
    *   `RC_BAR2_LO`: `0xFFFF_F000 | 0x1C` (64-bit, Prefetchable).
    *   `UBUS_BAR2_LO`: `0x0013_0000 | 1` (Mapped to MIP Physical Address, Enabled).

### 2. RP1 Southbridge & MSI-X
The RP1 is enumerated as a PCIe Endpoint (Vendor ID: `0x1DE4`).
*   **Bus Mastering:** The Command Register is configured to allow Memory Access and Bus Mastering.
*   **MSI-X Capability:** The capability structure is located at offset `0xB0` in the Configuration Space. Bit 31 of the Control Register enables MSI-X.
*   **Vector 25:** The hardware assigns Vector 25 to UART0. The MSI-X Table entry for this vector is programmed to write to the BAR2 address (`0xFFFF_F000`).

### 3. Machine Interrupt Peripheral (MIP)
The MIP acts as the bridge between the system interconnect and the GIC.
*   **Address:** `0x10_0013_0000`.
*   **VPU Masking:** The firmware enables VideoCore (VPU) access to the MIP by default. To prevent the VPU from acknowledging and clearing interrupts before the ARM core sees them, `INT_MASKL_VPU` and `INT_MASKH_VPU` must be written with `0xFFFFFFFF`.
*   **Trigger Configuration:** `INT_CFGL_HOST` is set to `0xFFFFFFFF` to match the active-high/edge signaling of the RP1.

### 4. GICv2 Configuration
The BCM2712 implements GICv2 legacy memory-mapped interfaces.
*   **Distributor:** `0x10_7FFF_9000`.
*   **CPU Interface:** `0x10_7FFF_A000`.
*   **Mapping:** MIP Input 0 maps to Shared Peripheral Interrupt (SPI) **128**. In the GIC's linear ID space, this is ID **160** (32 PPIs + 128).
*   **Security Groups:** The interrupt is configured as Group 1 (Non-Secure) to ensure visibility in EL1.

## Hardware Memory Map

| Component | Physical Address | Description |
| :--- | :--- | :--- |
| **GIC Distributor** | `0x10_7FFF_9000` | Interrupt routing and enabling. |
| **GIC CPU Interface** | `0x10_7FFF_A000` | Interrupt acknowledgement and EOI. |
| **MIP Base** | `0x10_0013_0000` | Doorbell for MSI-X writes. |
| **PCIe2 RC Base** | `0x10_0012_0000` | Root Complex for RP1 connection. |
| **RP1 Config Window** | `0x1F_0010_9000` | PCIe Configuration Space for RP1. |
| **RP1 MSI-X Table** | `0x1F_0041_0000` | Target addresses for interrupts. |
| **RP1 UART0** | `0x1F_0003_0000` | PL011 Serial Controller. |

## Boot Log

The following output captures the kernel initialization sequence. It verifies the configuration of the bridge, the discovery of the Southbridge, and the successful routing of interrupts.

```text
tio /dev/cu.usbmodem1402
[20:01:27.497] tio 3.9
[20:01:27.498] Press ctrl-t q to quit
[20:01:27.502] Connected to /dev/cu.usbmodem1402

========================================
   RASPBERRY PI 5 - BARE METAL KERNEL   
========================================
[INFO] Foundation initialized.
[INFO] PCIe Bridge Initialization...
       |__ SCB Access: [ALREADY ACTIVE]
       |__ BAR2 Remap: CPU[0x10_0013_0000] <-> PCIe[0xff_fffff000]
[INFO] RP1 Southbridge Discovery...
       |__ ID Check: Vendor 0x1de4 Device 0xdead
       |__ MSI-X Capability: Found at Offset 0xb0
       |__ MSI-X Status: [ENABLED]
       |__ MSI-X Vector 25: Configured -> MIP Doorbell
[INFO] GICv2 & MIP Initialization...
       |__ MIP: Host Configured (CFG=0xFF..), VPU Masked
       |__ SPI 160: Grp1, Prio=0x80, Target=CPU0, Edge
       |__ GICD: Distributor Enabled
       |__ GICC: CPU Interface Enabled (Grp0 + Grp1)
[INFO] Unmasking CPU IRQ (DAIF)...
[SUCCESS] System Ready. Entering WFI Loop...
```

## Build and Usage

**Prerequisites:**
*   Rust Toolchain (`cargo`, `rustc`).
*   `aarch64-elf-binutils` (for `objcopy`, `ld`).
*   `aarch64-unknown-none` target.

**Compilation:**
```bash
make build
```

**Debugging (OpenOCD + GDB):**
Requires a Raspberry Pi Debug Probe connected to the UART/SWD port.
```bash
# Terminal 1
make openocd-pi5

# Terminal 2
make gdb-pi5
```