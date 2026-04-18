#!/bin/sh
# Buildroot post-build hook.  Runs against the target/ staging directory
# after packages are installed and the overlay is applied, before
# rootfs.cpio is assembled.
#
# buildroot invocation: "$(pwd)/$BR2_ROOTFS_POST_BUILD_SCRIPT" "$TARGET_DIR".
set -eu

TARGET_DIR="$1"

# Force /bin/sh to point at dash.  Buildroot's default skeleton creates
# /bin/sh -> busybox, and busybox's ash applet takes it over — which
# conflicts with our "dash is the POSIX shell" decision.
rm -f "${TARGET_DIR}/bin/sh"
ln -sf dash "${TARGET_DIR}/bin/sh"

# Rewrite /sbin/init as an absolute symlink.  Buildroot emits it as
# ../bin/busybox (relative), and Solaya's VFS walker currently doesn't
# resolve `..` in paths — tracked as a separate kernel bug.  Absolute
# symlinks sidestep the issue and cost nothing.
rm -f "${TARGET_DIR}/sbin/init"
ln -sf /bin/busybox "${TARGET_DIR}/sbin/init"

# Ensure /etc/init.d/rcS is executable in case git permissions were
# mangled through the overlay copy.
if [ -f "${TARGET_DIR}/etc/init.d/rcS" ]; then
    chmod +x "${TARGET_DIR}/etc/init.d/rcS"
fi
