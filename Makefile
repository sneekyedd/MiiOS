# MiiOS build commands. Run `make help` to see them.
#
# You need: rustup (Rust), qemu-system-x86_64, OVMF (UEFI firmware for QEMU),
# mtools, sfdisk (util-linux) and git. See README.md for install commands.

KERNEL   := target/x86_64-unknown-none/release/kernel
LIMINE   := build/limine
IMAGE    := build/miios.img
MEM      ?= 512M

# Find OVMF (UEFI firmware for QEMU). Different Linux distros keep it in different places.
OVMF_CODE ?= $(firstword $(wildcard \
	/usr/share/OVMF/OVMF_CODE_4M.fd \
	/usr/share/OVMF/OVMF_CODE.fd \
	/usr/share/edk2/x64/OVMF_CODE.4m.fd \
	/usr/share/edk2/ovmf/OVMF_CODE.fd \
	/usr/share/edk2-ovmf/x64/OVMF_CODE.fd \
	/usr/share/qemu/ovmf-x86_64-code.bin))
OVMF_VARS_SRC ?= $(firstword $(wildcard \
	/usr/share/OVMF/OVMF_VARS_4M.fd \
	/usr/share/OVMF/OVMF_VARS.fd \
	/usr/share/edk2/x64/OVMF_VARS.4m.fd \
	/usr/share/edk2/ovmf/OVMF_VARS.fd \
	/usr/share/edk2-ovmf/x64/OVMF_VARS.fd \
	/usr/share/qemu/ovmf-x86_64-vars.bin))

# Use hardware virtualisation (KVM) if this machine has it: much faster.
KVM := $(shell [ -w /dev/kvm ] && echo "-enable-kvm -cpu host" || echo "-cpu max")

QEMU_FLAGS := -machine q35 $(KVM) -m $(MEM) \
	-drive if=pflash,format=raw,readonly=on,file=$(OVMF_CODE) \
	-drive if=pflash,format=raw,file=build/ovmf_vars.fd \
	-drive format=raw,file=$(IMAGE),if=none,id=disk0 \
	-device nvme,drive=disk0,serial=MIIOS0 \
	-device VGA,xres=1920,yres=1080,edid=on \
	-serial stdio -no-reboot

.PHONY: all kernel image run run-headless size clean help

all: image

help:
	@echo "make run      - build everything and boot MiiOS in a QEMU window"
	@echo "make image    - build build/miios.img (the file you flash to a USB stick)"
	@echo "make size     - show how big the kernel is"
	@echo "make clean    - delete build output"

kernel:
	cargo build --release

$(LIMINE):
	mkdir -p build
	git clone --depth 1 --branch v8.x-binary https://github.com/limine-bootloader/limine.git $(LIMINE)

image: kernel $(LIMINE)
	./tools/mkimage.sh $(KERNEL) $(LIMINE) $(IMAGE)

build/ovmf_vars.fd:
	@if [ -z "$(OVMF_CODE)" ]; then echo "OVMF not found: install the 'ovmf' (Debian/Ubuntu) or 'edk2-ovmf' (Fedora/Arch) package"; exit 1; fi
	mkdir -p build
	cp $(OVMF_VARS_SRC) $@

run: image build/ovmf_vars.fd
	qemu-system-x86_64 $(QEMU_FLAGS)

# Same, but without a window (used for automated tests; prints serial output).
run-headless: image build/ovmf_vars.fd
	qemu-system-x86_64 $(QEMU_FLAGS) -display none

size: kernel
	@ls -l $(KERNEL) | awk '{printf "kernel: %d bytes (%.1f KiB)\n", $$5, $$5/1024}'

clean:
	cargo clean
	rm -rf build
