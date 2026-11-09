#!/usr/bin/env bash
# Build the MiiOS (Linux edition) USB image.
#
#   sudo ./linux/build-image.sh            -> build/miios-linux.img (+ .img.xz)
#
# What it does, step by step:
#   1. Download a minimal Debian 13 ("trixie") into build/linux/rootfs   (debootstrap)
#   2. Install the Linux kernel, Wi-Fi/graphics firmware, NetworkManager,
#      Sway, Firefox, sound, and the installer tools                     (apt)
#   3. Add MiiOS: the home screen (miios-shell), its scripts and settings,
#      Mii Hub, and the classic bare-metal MiiOS
#   4. Create the user "mii" (auto-login) and switch on the services
#   5. Pack it all into a disk image: an EFI boot partition (systemd-boot)
#      + an ext4 partition with the system. No mounting of real disks.
#
# Needs: Debian/Ubuntu host, root, debootstrap, mtools, fdisk (sfdisk),
# e2fsprogs, xz-utils, rsync, cargo (Rust), git. Internet access to deb.debian.org.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"
B="${BUILD_DIR:-$REPO/build/linux}"
R="$B/rootfs"
OUT="${OUT:-$REPO/build/miios-linux.img}"
SUITE=trixie
MIRROR=https://deb.debian.org/debian
ESP_MB=512
MII_HUB_SRC="${MII_HUB_SRC:-https://github.com/sneekyedd/mii-hub}"   # or a local folder

[ "$(id -u)" = 0 ] || { echo "Please run with sudo."; exit 1; }
step() { echo; echo "==> $*"; }
mkdir -p "$B"

mounts_on() { for d in proc sys dev dev/pts; do mountpoint -q "$R/$d" || mount --bind "/$d" "$R/$d"; done; }
mounts_off() { for d in dev/pts dev sys proc; do mountpoint -q "$R/$d" && umount -l "$R/$d"; done; true; }
trap mounts_off EXIT
in_root() { chroot "$R" /usr/bin/env DEBIAN_FRONTEND=noninteractive LANG=C.UTF-8 /bin/bash -c "$*"; }

# ---------------------------------------------------------------- 1. Debian
if [ ! -x "$R/usr/bin/apt-get" ]; then
    step "Downloading a minimal Debian ($SUITE)"
    rm -rf "$R"
    debootstrap --variant=minbase --components=main,contrib,non-free,non-free-firmware "$SUITE" "$R" "$MIRROR"
fi
cat > "$R/etc/apt/sources.list" <<EOF
deb $MIRROR $SUITE main contrib non-free non-free-firmware
deb $MIRROR $SUITE-updates main contrib non-free non-free-firmware
deb https://deb.debian.org/debian-security $SUITE-security main contrib non-free non-free-firmware
EOF
cp /etc/resolv.conf "$R/etc/resolv.conf" 2>/dev/null || true
mounts_on

# ---------------------------------------------------------------- 2. Packages
step "Installing packages (kernel, firmware, Wi-Fi, Sway, Firefox, ...)"
PKGS="$(grep -v '^#' "$HERE/packages.txt" | tr '\n' ' ')"
in_root "apt-get update -qq && apt-get install -y -qq --no-install-recommends $PKGS"
in_root "apt-get clean"

# ---------------------------------------------------------------- 3. MiiOS
step "Building the MiiOS home screen and the classic kernel"
# Run cargo as the normal user who called sudo (that's where Rust is installed).
cargo_as_user() { if [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != root ]; then sudo -u "$SUDO_USER" -H env "PATH=$PATH" cargo "$@"; else cargo "$@"; fi; }
( cd "$HERE/shell" && cargo_as_user build --release )
( cd "$REPO" && cargo_as_user build --release )
make -C "$REPO" build/limine >/dev/null

step "Adding MiiOS files"
cp -a "$HERE/rootfs-overlay/." "$R/"
install -m 755 "$HERE/shell/target/x86_64-unknown-linux-gnu/release/miios-shell" "$R/usr/local/bin/miios-shell"
chmod 440 "$R/etc/sudoers.d/miios"

step "Adding Mii Hub (its newest release) and your Miis"
HUB="$R/opt/miios/apps/mii-hub"
rm -rf "$R/opt/miios/apps"; mkdir -p "$HUB"
if [ -d "$MII_HUB_SRC" ]; then               # a local folder (for testing)
    cp -a "$MII_HUB_SRC/." "$HUB/"; HUB_VER=local
else
    HUB_VER=$(curl -fsS --max-time 30 "https://api.github.com/repos/sneekyedd/mii-hub/releases/latest" | jq -r '.tag_name // empty' || true)
    if [ -n "$HUB_VER" ]; then
        curl -fsSL "https://codeload.github.com/sneekyedd/mii-hub/tar.gz/refs/tags/$HUB_VER" | tar -xz -C "$HUB" --strip-components=1
    else
        git clone -q --depth 1 "$MII_HUB_SRC" "$HUB"; HUB_VER=main
    fi
fi
sh "$HERE/rootfs-overlay/usr/local/bin/miios-prepare-hub" "$HUB"
echo "$HUB_VER" > "$HUB/.miios-version"
# Miis that come with the image (yours!). Copied into the user's ~/MiiOS/miis.
mkdir -p "$R/etc/skel/MiiOS/miis"
cp "$HERE"/miis/*.glb "$R/etc/skel/MiiOS/miis/" 2>/dev/null || true

# ---------------------------------------------------------------- 4. System setup
step "Setting up the system"
echo miios > "$R/etc/hostname"
printf '127.0.0.1 localhost\n127.0.1.1 miios\n::1 localhost\n' > "$R/etc/hosts"
echo 'LANG=C.UTF-8' > "$R/etc/default/locale"
mkdir -p "$R/etc/miios" "$R/boot/efi"
touch "$R/etc/miios/live"   # "running from the USB stick" (the installer removes this)
in_root "id mii >/dev/null 2>&1 || useradd -m -s /bin/bash -G sudo,video,render,input,audio,netdev mii"
in_root "echo mii:miios | chpasswd"
in_root "cp -rn /etc/skel/. /home/mii/ && chown -R mii:mii /home/mii"
in_root "systemctl enable NetworkManager miios-grow.service >/dev/null 2>&1; systemctl disable systemd-networkd >/dev/null 2>&1 || true"
in_root "systemctl set-default multi-user.target >/dev/null"
: > "$R/etc/machine-id"     # each laptop makes its own ID on first boot
rm -rf "$R/var/lib/apt/lists/"* "$R/var/cache/apt/"*.bin "$R/tmp/"*

mounts_off
trap - EXIT

# ---------------------------------------------------------------- 5. Disk image
step "Building the disk image"
ROOT_UUID=$(cat /proc/sys/kernel/random/uuid)
ESP_SERIAL=$(printf '%08X' $(( RANDOM * 65536 + RANDOM )))
ESP_UUID="${ESP_SERIAL:0:4}-${ESP_SERIAL:4:4}"
cat > "$R/etc/fstab" <<EOF
UUID=$ROOT_UUID  /          ext4  defaults,noatime  0 1
UUID=$ESP_UUID   /boot/efi  vfat  umask=0077        0 1
EOF

# The EFI boot partition's contents.
E="$B/esp"
rm -rf "$E"; mkdir -p "$E/EFI/BOOT" "$E/EFI/limine" "$E/loader/entries" "$E/miios" "$E/boot/limine"
cp "$R/usr/lib/systemd/boot/efi/systemd-bootx64.efi" "$E/EFI/BOOT/BOOTX64.EFI"
cp -L "$R/vmlinuz" "$E/miios/vmlinuz"
cp -L "$R/initrd.img" "$E/miios/initrd.img"
cat > "$E/loader/loader.conf" <<EOF
# Boots MiiOS straight away. Hold Space while the laptop starts to see the menu.
default miios.conf
timeout 0
editor no
console-mode keep
EOF
cat > "$E/loader/entries/miios.conf" <<EOF
title   MiiOS
linux   /miios/vmlinuz
initrd  /miios/initrd.img
options root=UUID=$ROOT_UUID rw quiet splash loglevel=3 systemd.show_status=auto rd.udev.log_level=3 vt.global_cursor_default=0
EOF
# The original bare-metal MiiOS, as a second menu entry.
cp "$REPO/build/limine/BOOTX64.EFI" "$E/EFI/limine/BOOTX64.EFI"
cp "$REPO/limine.conf" "$E/boot/limine/limine.conf"
cp "$REPO/target/x86_64-unknown-none/release/kernel" "$E/boot/kernel"
cat > "$E/loader/entries/miios-classic.conf" <<EOF
title   MiiOS Classic (bare metal, no Linux)
efi     /EFI/limine/BOOTX64.EFI
EOF

# ext4 system partition, filled straight from the folder (no mounting needed).
USED_MB=$(du -sm "$R" | cut -f1)
ROOT_MB=$(( USED_MB * 115 / 100 + 400 ))
rm -f "$B/root.img"
mkfs.ext4 -q -L miios-root -U "$ROOT_UUID" -d "$R" "$B/root.img" "${ROOT_MB}M"

# Put both partitions into one GPT disk image.
TOTAL_MB=$(( 1 + ESP_MB + ROOT_MB + 1 ))
rm -f "$OUT"; truncate -s "${TOTAL_MB}M" "$OUT"
sfdisk -q "$OUT" <<EOF
label: gpt
start=2048, size=$(( ESP_MB * 2048 )), type=uefi, name="MIIOS-EFI"
start=$(( (1 + ESP_MB) * 2048 )), size=$(( ROOT_MB * 2048 )), type=linux, name="MIIOS-ROOT"
EOF
mformat -i "$OUT@@1M" -F -N "$ESP_SERIAL" -v MIIOS-EFI -T $(( ESP_MB * 2048 )) ::
mcopy -s -i "$OUT@@1M" "$E"/* ::/
dd if="$B/root.img" of="$OUT" bs=1M seek=$(( 1 + ESP_MB )) conv=notrunc,sparse status=none
rm -f "$B/root.img"

if [ "${NO_XZ:-0}" != 1 ]; then
    step "Compressing for download"
    xz -T0 -6 -k -f "$OUT"
    sha256sum "$OUT.xz" > "$OUT.xz.sha256"
fi
step "Done: $OUT ($(du -h --apparent-size "$OUT" | cut -f1))"
