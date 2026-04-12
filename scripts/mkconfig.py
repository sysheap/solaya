#!/usr/bin/env python3
"""
Translate a Solaya .config into build-system-consumable artifacts.

Outputs (under --out-dir):
    kconfig.h             Linux-style autoconf.h, for future C consumers.

Invoked by cmake/kconfig.cmake during CMake configure.
"""

import argparse
import os
import sys
from pathlib import Path

_HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE.parent / "tools" / "kconfiglib"))
import kconfiglib  # noqa: E402


def emit_autoconf_h(kconf, out):
    """Linux-style autoconf.h."""
    kconf.write_autoconf(out)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--kconfig", required=True, help="Path to root Kconfig")
    ap.add_argument("--config", required=True, help="Path to .config")
    ap.add_argument("--out-dir", required=True, help="Output directory")
    ap.add_argument(
        "--source-dir",
        default=None,
        help="Source tree root (default: cwd). Kconfig `source` directives "
        "resolve relative to this.",
    )
    args = ap.parse_args()

    source_dir = args.source_dir or os.getcwd()
    out = Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)

    os.chdir(source_dir)
    kconf = kconfiglib.Kconfig(args.kconfig, warn=True, warn_to_stderr=True)
    kconf.load_config(args.config)

    emit_autoconf_h(kconf, out / "kconfig.h")


if __name__ == "__main__":
    main()
