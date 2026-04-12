#!/usr/bin/env bash
# Send the magic DEADBEEF reboot sequence over /dev/ttyUSB0.
set -euo pipefail
stty -F /dev/ttyUSB0 115200 raw
printf '\xDE\xAD\xBE\xEF' > /dev/ttyUSB0
