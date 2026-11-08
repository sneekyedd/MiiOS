#!/usr/bin/env bash
# Write build/miios.img to a USB stick -- with safety checks so you can't
# accidentally overwrite your computer's own disk.
#
#   usage:  sudo ./tools/flash-usb.sh /dev/sdX
#
# Run it without arguments to list the USB disks it would accept.
set -euo pipefail

IMG="${IMG:-build/miios.img}"
DEV="${1:-}"

list_usb() {
    echo "USB disks currently connected:"
    lsblk -d -o NAME,SIZE,TRAN,MODEL | awk 'NR==1 || $3=="usb" { print "  " $0 }'
}

if [ -z "$DEV" ]; then
    echo "usage: sudo $0 /dev/sdX"
    echo
    list_usb
    exit 1
fi

[ -f "$IMG" ] || { echo "No image at $IMG - run 'make image' first."; exit 1; }
[ -b "$DEV" ] || { echo "$DEV is not a disk device."; list_usb; exit 1; }

NAME="$(basename "$DEV")"
# Must be a whole disk (sdb), not a partition (sdb1).
if [ ! -d "/sys/block/$NAME" ]; then
    echo "$DEV looks like a partition. Give the whole disk, e.g. /dev/sdb (not /dev/sdb1)."
    exit 1
fi
# Must be connected over USB.
TRAN="$(lsblk -dno TRAN "$DEV" | tr -d ' ')"
if [ "$TRAN" != "usb" ]; then
    echo "Refusing: $DEV is connected via '${TRAN:-unknown}', not USB. This protects your internal drives."
    list_usb
    exit 1
fi
# Must not be mounted (e.g. your file manager auto-opened it).
if lsblk -no MOUNTPOINT "$DEV" | grep -q .; then
    echo "$DEV (or a partition on it) is mounted. Unmount it first:"
    echo "    sudo umount ${DEV}?*"
    exit 1
fi

echo "About to ERASE this USB disk and write MiiOS to it:"
lsblk -o NAME,SIZE,MODEL,TRAN "$DEV" | sed 's/^/    /'
read -r -p "Type the device name ($NAME) to continue: " ANSWER
[ "$ANSWER" = "$NAME" ] || { echo "Cancelled."; exit 1; }

dd if="$IMG" of="$DEV" bs=4M conv=fsync status=progress
sync
echo
echo "Done! You can unplug the stick and boot your ThinkPad from it (press F12 at power-on)."
