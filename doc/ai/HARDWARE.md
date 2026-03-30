# Hardware Boot Guide

## Supported Hardware

### StarFive VisionFive 2
SoC: JH7110 (SiFive U74 RISC-V cores). Board has DDR starting at 0x40000000.

## Hardware Setup

### Boot Mode Jumpers (VisionFive 2)
Set both RGPIO_0 and RGPIO_1 jumpers to LOW (QSPI mode). They are located near the 40-pin GPIO header at the top of the board.

### UART Serial Console
Connect a 3.3V USB-to-UART adapter to the 40-pin GPIO header:
```
Pin 8  (GPIO5, UART TX) -> RX on adapter
Pin 10 (GPIO6, UART RX) -> TX on adapter
Pin 6  (GND)             -> GND on adapter
```
Serial config: 115200 baud, 8N1.

### Ethernet
Connect to eth0 (port closer to USB ports). Must be on the same network as the development machine.

## TFTP Server

The devshell includes `atftp`. Start the TFTP server outside the container (port 69 requires root — U-Boot hardcodes this port):

```bash
sudo atftpd --daemon --no-fork --verbose 7 /path/to/solaya/target/tftp
```

Build and deploy the binary:
```bash
just tftp-deploy     # Builds and copies to target/tftp/solaya.bin
```

Set `SOLAYA_TFTP_DIR` to change the deploy directory.

## U-Boot Commands

After connecting serial console, power on the board and press any key to stop autoboot:

```
# Set network (adjust for your setup)
setenv ipaddr 192.168.1.200
setenv serverip 192.168.1.100

# Load and boot
tftpboot 0x80200000 solaya.bin
booti 0x80200000 - ${fdtcontroladdr}
```

If `booti` rejects the binary (no Linux Image header), use: `go 0x80200000`

To save U-Boot env so you don't have to type IPs every time: `saveenv`

## Platform Generalization

Solaya discovers hardware configuration from the device tree at runtime:
- PLIC base address and size
- CLINT base address and size (optional mapping)
- UART interrupt IRQ number
- Memory size

The UART base address (0x10000000) is currently hardcoded but happens to be the same on both QEMU virt and VisionFive 2. The baud rate is not reprogrammed — firmware configures it.

PCI initialization is optional: platforms without PCI ECAM (like VisionFive 2) skip it gracefully.
