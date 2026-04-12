#!/usr/bin/env bash
# Open a picocom session on /dev/ttyUSB0.
set -euo pipefail
exec picocom --omap crlf /dev/ttyUSB0 -b 115200
