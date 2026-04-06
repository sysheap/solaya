#!/usr/bin/env bash

set -e

cd "$(dirname "$0")"

rm -f .gdb-port

QEMU_CMD="qemu-system-riscv64 \
    -machine virt \
    -cpu rv64 \
    -m 512M \
    -serial mon:stdio \
    -device virtio-rng-pci"

NEED_DISPLAY=false
HEADLESS=false

# Process options
while [[ $# -gt 0 ]]; do
    case "$1" in
        --capture)
            QEMU_CMD+=" -object filter-dump,id=f1,netdev=netdev1,file=network.pcap "
            shift
            ;;
        --fb)
            QEMU_CMD+=" -device bochs-display -device virtio-keyboard-pci"
            NEED_DISPLAY=true
            shift
            ;;
        --headless)
            HEADLESS=true
            shift
            ;;
        --gdb)
            shift
            if [[ "$1" =~ ^[0-9]+$ ]]; then
                GDB_PORT="$1"
                shift
            else
                GDB_PORT=$(python3 -c "import socket; s=socket.socket(socket.AF_INET, socket.SOCK_STREAM); s.bind(('127.0.0.1', 0)); print(s.getsockname()[1]); s.close()")
            fi
            QEMU_CMD+=" -gdb tcp::${GDB_PORT}"
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS] <KERNEL_PATH>"
            echo ""
            echo "Options:"
            echo "  --block FILE   Attach a raw disk image as virtio-blk device"
            echo "  --fb           Attach bochs-display framebuffer device"
            echo "  --headless     Force -display none even with framebuffer"
            echo "  --qmp PATH     Enable QMP on a Unix socket"
            echo "  --gdb [PORT]   Enable GDB server (default: dynamic port)"
            echo "  --log          Log qemu events to /tmp/solaya.log"
            echo "  --capture      Capture network traffic into network.pcap"
            echo "  --net [PORT]   Enable network card with host port PORT (default: dynamic)"
            echo "  -h, --help     Show this help message"
            echo "  --wait         Wait cpu until gdb is attached"
            exit 0
            ;;
        --log)
            QEMU_CMD+=" -d guest_errors,cpu_reset,unimp,int -D /tmp/solaya.log"
            shift
            ;;
        --qmp)
            shift
            QMP_SOCKET="$1"
            QEMU_CMD+=" -qmp unix:${QMP_SOCKET},server,wait=off"
            shift
            ;;
        --block)
            shift
            BLOCK_FILE="$1"
            shift
            if [[ ! -f "$BLOCK_FILE" ]]; then
                dd if=/dev/zero of="$BLOCK_FILE" bs=1M count=1 2>/dev/null
            fi
            QEMU_CMD+=" -drive if=none,file=${BLOCK_FILE},format=raw,id=hd0 -device virtio-blk-pci,drive=hd0"
            ;;
        --net)
            shift
            if [[ "$1" =~ ^[0-9]+$ ]]; then
                NET_PORT="$1"
                shift
            else
                NET_PORT=$(python3 -c "import socket; s=socket.socket(socket.AF_INET, socket.SOCK_STREAM); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind(('127.0.0.1', 0)); print(s.getsockname()[1]); s.close()")
            fi
            QEMU_CMD+=" -netdev user,id=netdev1,hostfwd=udp::${NET_PORT}-:1234,hostfwd=tcp::${NET_PORT}-:1234 -device virtio-net-pci,netdev=netdev1"
            ;;
        --smp)
            QEMU_CMD+=" -smp $(nproc)"
            shift
            ;;
        --wait)
            QEMU_CMD+=" -S"
            shift
            ;;
        -*)
            echo "Unknown option: $1"
            exit 1
            ;;
        *)
            # Assume the last non-option argument is the kernel path
            KERNEL_PATH="$1"
            shift
            ;;
    esac
done

# Validate kernel path
if [[ -z "$KERNEL_PATH" ]]; then
    echo "Error: You must specify the kernel path."
    echo "Use $0 --help for more information."
    exit 1
fi

if [[ "$HEADLESS" == "true" ]]; then
    QEMU_CMD+=" -display none"
elif [[ "$NEED_DISPLAY" == "true" ]] && [[ -n "$DISPLAY" || -n "$WAYLAND_DISPLAY" ]]; then
    QEMU_CMD+=" -display gtk"
else
    QEMU_CMD+=" -display none"
fi

# Add the kernel option
QEMU_CMD+=" -kernel $KERNEL_PATH"

# Execute the QEMU command
echo "Executing: $QEMU_CMD"

if [[ -n "$NET_PORT" ]]; then
    echo "Network host port: $NET_PORT" >&2
fi

if [[ -n "$GDB_PORT" ]]; then
    echo "GDB port: $GDB_PORT" >&2
    echo "$GDB_PORT" > .gdb-port
fi

exec bash -c "$QEMU_CMD"
