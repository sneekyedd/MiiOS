# MiiOS

A tiny, fast hobby operating system written in Rust for a Lenovo ThinkPad T470
(i5-7200U, 8 GB RAM, 1920×1080, UEFI). It has a playful, pastel home screen,
your own cartoon avatars, and a text editor.

This README explains **every choice**, so you can learn OS development from it.
Each milestone has its own section below.

---

## Quick start (Linux)

1. Install the tools (Debian/Ubuntu; for Fedora use `dnf install qemu-system-x86 edk2-ovmf mtools util-linux git`):
   ```bash
   sudo apt install qemu-system-x86 ovmf mtools fdisk git build-essential
   ```
2. Install Rust (skip this step if you already have `rustup`):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
3. Build and run:
   ```bash
   make run
   ```
   The first build downloads the Limine bootloader and the Rust bare-metal target.
   A QEMU window opens, and the kernel's log messages appear in your terminal.
4. Close the QEMU window (or press Ctrl+C in the terminal) to stop.

Other commands: `make image` builds only the disk image (`build/miios.img`), `make size`
prints the kernel size, and `make clean` deletes all build output.

> **Safety:** QEMU only sees the file `build/miios.img`. Nothing in this project
> writes to your real disks. Only the USB flashing step (milestone 9), which you
> run yourself, touches a real device.

---

## Big-picture choices

| Choice | Why |
|---|---|
| **Rust, `no_std`** | Memory safety without a garbage collector. `no_std` means we don't use Rust's standard library, because it expects an OS underneath, and we *are* the OS. |
| **Stable Rust**, target `x86_64-unknown-none` | No nightly compiler needed. This target already has no OS and no SSE/AVX, so interrupts and task switches never need to save the big vector registers. |
| **Limine bootloader** (not the `bootloader` crate) | Very fast, small, well tested on real laptops, and hands us a ready framebuffer, memory map and page tables. We write the ~150 lines of Limine bindings ourselves (`kernel/src/boot.rs`) because the official `limine` crate needs nightly Rust. |
| **QEMU + OVMF** | QEMU emulates a whole PC; OVMF is UEFI firmware for it. Like your T470, it boots via UEFI from an NVMe disk. |
| **Release profile**: `opt-level=3`, `lto="fat"`, `codegen-units=1`, `panic="abort"`, `strip` | The fastest code the compiler can make. Link-time optimisation removes unused code across crates, and `panic=abort` drops the unwinding machinery. |
| **Higher-half kernel** at `0xffffffff80000000` | The standard layout. The kernel lives in the top 2 GiB of the address space, which leaves the bottom free. |

### Files

| Path | What it is |
|---|---|
| `kernel/src/main.rs` | `kmain`, the first Rust function Limine jumps to. |
| `kernel/src/boot.rs` | Requests to Limine: framebuffer, memory map, etc. |
| `kernel/src/arch/` | CPU-specific code: I/O ports, interrupts, GDT, timer chips. |
| `kernel/src/serial.rs` | Debug log over the serial port. `log!()` prints to your terminal under QEMU. |
| `kernel/src/time.rs` | Measures the CPU's timestamp counter so we can tell the time. |
| `kernel/src/gfx/` | Graphics: fonts, drawing, dirty rectangles. |
| `kernel/linker.ld` | Tells the linker where in memory each part of the kernel goes. |
| `kernel/build.rs` | Passes the linker script to the linker. |
| `.cargo/config.toml` | Always builds for bare metal, with kernel-friendly compiler flags. |
| `limine.conf` | Limine menu config. `timeout: 0` boots instantly. |
| `tools/mkimage.sh` | Builds the GPT disk image (FAT32 boot partition + MiiOS data partition). |
| `Makefile` | `make run`, `make image`, … |

---

## Milestone 1: boot and print text ✅

**What happens when you power on:**

1. UEFI firmware (OVMF in QEMU, Lenovo's firmware on the T470) starts up and looks for
   `\EFI\BOOT\BOOTX64.EFI` on a FAT32 partition. That file is Limine.
2. Limine reads `boot/limine/limine.conf`, loads `boot/kernel` (our ELF file) into
   memory, sets up 64-bit mode and page tables, fills in our *requests*, and jumps to `kmain`.
3. `kmain` sets up the serial port, measures CPU speed, paints the screen and draws text
   using the embedded **Spleen** bitmap font (BSD licence, `kernel/assets/fonts/`).

**How Limine "requests" work:** the kernel contains small structs with magic numbers in a
section called `.limine_requests`. Limine finds them before starting us and writes a
pointer to its answer into each one. No function calls, no parsing.

**Measuring boot time:** the CPU's Time Stamp Counter (TSC) starts at zero at power-on
and counts at a fixed rate. We measure that rate against the 1.193 MHz PIT timer chip
(10 ms), then `TSC / rate` gives *time since power-on*.

**Results (QEMU without KVM, i.e. a pure software CPU, so slower than real hardware):**

| | |
|---|---|
| Power-on → kernel start (firmware + Limine) | 2.41 s (emulated firmware; on a T470 most of this is Lenovo's firmware) |
| Kernel start → text on screen | **15 ms** (10 ms of it is the TSC calibration) |
| Kernel size | **66 KiB** |
| Screen | 1920×1080, 32 bits per pixel |

What you should see: a soft white screen with "Hello from MiiOS!" in the top-left, and
log lines in the terminal.

---

## Milestone 2: framebuffer graphics ✅

**Files:** `kernel/src/gfx/` (`canvas.rs`, `screen.rs`, `math.rs`, `color.rs`, `rect.rs`, `font.rs`), `kernel/src/mem/early.rs`, `kernel/src/demo.rs`.

**Key ideas:**

1. **Back buffer.** We draw into an 8 MB copy of the screen in normal RAM, never into
   screen memory directly. Screen memory is slow to read, and drawing there directly
   would show half-finished frames (flicker).
2. **Dirty rectangles** (`screen.rs`). Every change says "this rectangle changed". Before
   copying to the screen we redraw *only* those rectangles. The trick: the whole scene's
   draw code runs with a *clip rectangle*, so anything outside it is skipped almost for free.
   Overlapping rectangles are merged, with at most 32 tracked.
3. **Fixed-point math** (`math.rs`). Numbers like 3.25 are stored as `3.25 × 256 = 832` in
   an ordinary integer. There's no floating point anywhere, so it's fast and needs no special
   CPU state. Even sine comes from a 256-entry table computed at compile time.
4. **Anti-aliasing** (`canvas.rs`). Smooth edges on circles, ellipses and rounded rectangles.
   Each pixel row is sampled at 4 heights. Pixels fully inside a shape are filled with one fast
   `rep stosd` instruction, and only edge pixels are blended. Thick lines and arcs (smiles!)
   use *signed distance functions*: "how far is this pixel from the shape?" turns directly into
   how opaque the pixel is.
5. **Alpha blending** with two multiplies per pixel, by processing red and blue together.
6. **Fonts.** Spleen in 8×16, 12×24 and 16×32, embedded at compile time with `include_bytes!`.
7. **Early allocator** (`mem/early.rs`). Before the real memory manager exists, the 8 MB back
   buffer is carved off the end of the largest free RAM region in Limine's memory map.

**Results (QEMU, software CPU emulation, so expect real hardware to be several times faster):**

| | |
|---|---|
| Full 1920×1080 scene draw + copy | ~45–60 ms (done once, at start) |
| One animation frame of the bouncing ball | **~0.2–0.3 ms**, ~10,000 pixels instead of 2,073,600 (≈200× less work) |
| Kernel start → first frame | ~55–75 ms |
| Kernel size | **113 KiB** (the three fonts are ~56 KiB of that) |

What you should see: six pastel tiles (the first with a blue selection ring), a smiling face,
lines of different thickness, and a blue ball bouncing smoothly. The terminal prints how long
each frame took.
