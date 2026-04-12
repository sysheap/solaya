#!/usr/bin/env python3
"""
Translate a Solaya .config into build-system-consumable artifacts.

Outputs (under --out-dir):
    cargo-features.txt    Per-crate cargo --features lists. Format:
                          "<crate>:<feat1>,<feat2>,..." one per line.
    rustc-cfg.txt         One --cfg argument per line, ready to be joined
                          into CARGO_ENCODED_RUSTFLAGS.
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


def _is_set(kconf, name):
    sym = kconf.syms.get(name)
    return sym is not None and sym.str_value == "y"


def emit_cargo_features(kconf, out):
    """Per-crate cargo feature lists derived from Kconfig selections.

    Cargo features are only used for options that must alter the dependency
    graph (e.g. arch selection when multiple arches exist).  Code-level
    toggles go through rustc-cfg.txt instead.

    Today the only arch is riscv64 and the solaya crate's default feature
    set already includes `arch/riscv64`, so no explicit --features arg is
    needed for boot/kernel.  When aarch64 / x86_64 land, this grows to
    emit the selected arch feature and the build system starts passing
    `--no-default-features` to bare-metal cargo invocations.
    """
    features = {"kernel": [], "userspace": []}

    with open(out, "w") as f:
        for crate, feats in features.items():
            f.write(f"{crate}:{','.join(feats)}\n")


def _cfg_name(kconf_name):
    return "solaya_" + kconf_name.lower()


def emit_rustc_cfg(kconf, out):
    """One --cfg argument per line."""
    lines = []
    for sym in kconf.unique_defined_syms:
        if not sym.name or sym.name == "BROKEN":
            continue
        val = sym.str_value
        cfg = _cfg_name(sym.name)
        if sym.type in (kconfiglib.BOOL, kconfiglib.TRISTATE):
            if val in ("y", "m"):
                lines.append(f"--cfg={cfg}")
        elif sym.type in (kconfiglib.INT, kconfiglib.HEX, kconfiglib.STRING):
            if val:
                lines.append(f'--cfg={cfg}="{val}"')

    with open(out, "w") as f:
        if lines:
            f.write("\n".join(lines) + "\n")


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

    emit_cargo_features(kconf, out / "cargo-features.txt")
    emit_rustc_cfg(kconf, out / "rustc-cfg.txt")
    emit_autoconf_h(kconf, out / "kconfig.h")


if __name__ == "__main__":
    main()
