#!/usr/bin/env bash
# Builds build/miios.img: a disk image that boots on UEFI PCs and in QEMU.
#
# Layout (GPT partition table, like any modern disk):
#   1 MiB gap | EFI System Partition (FAT32, 64 MiB) | MiiOS data partition (16 MiB)
#
# UEFI firmware looks for \EFI\BOOT\BOOTX64.EFI on the FAT partition: that's
# Limine. Limine reads boot/limine/limine.conf and loads boot/kernel.
#
# Nothing here needs root, and nothing touches a real disk: it only writes
# to the file build/miios.img.
set -euo pipefail

KERNEL="$1"
LIMINE_DIR="$2"
OUT="$3"

ESP_MB=64
DATA_MB=16
TOTAL_MB=$((1 + ESP_MB + DATA_MB + 1))   # +1 MiB at the end for the backup GPT

rm -f "$OUT"
truncate -s "${TOTAL_MB}M" "$OUT"

# Create the partition table. Sizes are in 512-byte sectors.
# The data partition uses our own type GUID so MiiOS can find it later.
sfdisk --quiet "$OUT" <<PARTS
label: gpt
start=2048, size=$((ESP_MB * 2048)), type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name="MIIOS-BOOT"
start=$(((1 + ESP_MB) * 2048)), size=$((DATA_MB * 2048)), type=4D69694F-5344-6174-6150-617274000001, name="MIIOS-DATA"
PARTS

# Format the first partition as FAT32 and copy the files in (mtools works on image files).
ESP="$OUT@@1M"
mformat -i "$ESP" -F -T $((ESP_MB * 2048)) -v MIIOS ::
mmd -i "$ESP" ::/EFI ::/EFI/BOOT ::/boot ::/boot/limine
mcopy -i "$ESP" "$LIMINE_DIR/BOOTX64.EFI" ::/EFI/BOOT/BOOTX64.EFI
mcopy -i "$ESP" limine.conf ::/boot/limine/limine.conf
mcopy -i "$ESP" "$KERNEL" ::/boot/kernel

echo "built $OUT ($(du -h --apparent-size "$OUT" | cut -f1))"
