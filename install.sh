#!/bin/sh
# Build mchose-tui, install it for the current user, and install the udev rule.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$here"

cargo build --release

bindir="${HOME}/.local/bin"
mkdir -p "$bindir"
install -m755 target/release/mchose-tui "$bindir/mchose-tui"

echo "installed $bindir/mchose-tui"
echo "installing udev rule (needs sudo) ..."
sudo install -m644 70-mchose.rules /etc/udev/rules.d/70-mchose.rules
sudo udevadm control --reload-rules
sudo udevadm trigger

echo
echo "Done. Replug the mouse or its receiver, then run:"
echo "  mchose-tui"
echo
echo "Make sure $bindir is in PATH."
