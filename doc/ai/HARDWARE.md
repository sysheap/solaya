# Hardware Boot Guide

## Supported Hardware

### StarFive VisionFive 2
SoC: JH7110 (SiFive U74 RISC-V cores). Board has DDR starting at 0x40000000.

## Hardware Setup

### UART Serial Console
Serial config: 115200 baud, 8N1. /dev/ttyUSB0

### Ethernet
Connect to eth0 (port closer to USB ports). Must be on the same network as the development machine.

## TFTP Server

The devshell includes `atftp`. Start the TFTP server outside the container (port 69 requires root — U-Boot hardcodes this port):

```bash
just tftp-server
```

Build and deploy the binary:
```bash
just tftp-deploy     # Builds and copies to target/tftp/solaya.bin
just reboot-hw       # Reboot the hw by sending a magic byte to the hw and it then resets itself (if the kernel is not stuck)
```

## Platform Generalization

Solaya discovers hardware configuration from the device tree at runtime:
- PLIC base address and size
- CLINT base address and size (optional mapping)
- UART interrupt IRQ number
- Memory size

The UART base address (0x10000000) is currently hardcoded but happens to be the same on both QEMU virt and VisionFive 2. The baud rate is not reprogrammed — firmware configures it.

PCI initialization is optional: platforms without PCI ECAM (like VisionFive 2) skip it gracefully.
