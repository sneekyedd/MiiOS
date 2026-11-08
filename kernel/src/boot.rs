//! Talking to the Limine bootloader.
//!
//! How it works: we place small "request" structs in a special section of the
//! kernel file (`.limine_requests`, see linker.ld). Before jumping to our code,
//! Limine scans that section, recognises each request by its magic ID numbers,
//! and fills in the `response` pointer. We then just read the answers.
//!
//! The official `limine` crate needs nightly Rust, so we write the handful of
//! structs we need by hand (copied from limine.h) and stay on stable Rust.

use core::cell::UnsafeCell;
use core::ptr;

const MAGIC_A: u64 = 0xc7b1dd30df4c8b88;
const MAGIC_B: u64 = 0x0a82e883a194f07b;

/// A request whose response Limine writes before we start.
#[repr(C)]
pub struct Request<T> {
    id: [u64; 4],
    revision: u64,
    response: UnsafeCell<*const T>,
}
// Safety: Limine writes these once before the kernel runs; afterwards read-only.
unsafe impl<T> Sync for Request<T> {}

impl<T> Request<T> {
    const fn new(c: u64, d: u64) -> Self {
        Self { id: [MAGIC_A, MAGIC_B, c, d], revision: 0, response: UnsafeCell::new(ptr::null()) }
    }
    /// The bootloader's answer, if it gave one.
    pub fn get(&self) -> Option<&'static T> {
        // volatile: the compiler must not assume this is still null.
        unsafe { ptr::read_volatile(self.response.get()).as_ref() }
    }
}

// ---- Response layouts (must match limine.h exactly) ----

#[repr(C)]
pub struct HhdmResponse { pub revision: u64, pub offset: u64 }

#[repr(C)]
pub struct Framebuffer {
    pub address: *mut u8,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub memory_model: u8,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
}

#[repr(C)]
pub struct FramebufferResponse { pub revision: u64, count: u64, list: *const *const Framebuffer }
impl FramebufferResponse {
    pub fn first(&self) -> Option<&'static Framebuffer> {
        if self.count == 0 { None } else { unsafe { (*self.list).as_ref() } }
    }
}

pub const MEM_USABLE: u64 = 0;
pub const MEM_BOOTLOADER_RECLAIMABLE: u64 = 5;
pub const MEM_KERNEL: u64 = 6;
pub const MEM_FRAMEBUFFER: u64 = 7;

#[repr(C)]
pub struct MemEntry { pub base: u64, pub length: u64, pub kind: u64 }

#[repr(C)]
pub struct MemmapResponse { pub revision: u64, count: u64, list: *const *const MemEntry }
impl MemmapResponse {
    pub fn entries(&self) -> impl Iterator<Item = &'static MemEntry> {
        let (n, l) = (self.count as usize, self.list);
        (0..n).map(move |i| unsafe { &**l.add(i) })
    }
}

#[repr(C)]
pub struct RsdpResponse { pub revision: u64, pub address: u64 }

#[repr(C)]
pub struct BootTimeResponse { pub revision: u64, pub unix_seconds: i64 }

#[repr(C)]
pub struct KernelAddressResponse { pub revision: u64, pub physical_base: u64, pub virtual_base: u64 }

#[repr(C)]
pub struct StackSizeResponse { pub revision: u64 }

// ---- The requests themselves, placed in the special section ----

#[used]
#[unsafe(link_section = ".limine_requests_start")]
static START_MARKER: [u64; 4] =
    [0xf6b8f4b39de7d1ae, 0xfab91a6940fcb9cf, 0x785c6ed015d3e316, 0x181e920a7852b9d9];

/// "Base revision 3": the protocol version we speak. Limine sets the last
/// number to 0 if it understood us.
#[used]
#[unsafe(link_section = ".limine_requests")]
static BASE_REVISION: BaseRevision = BaseRevision(UnsafeCell::new([0xf9562b2d5c95a6c8, 0x6a7b384944536bdc, 3]));
#[repr(C)]
struct BaseRevision(UnsafeCell<[u64; 3]>);
unsafe impl Sync for BaseRevision {}

pub fn base_revision_supported() -> bool {
    unsafe { ptr::read_volatile(&(*BASE_REVISION.0.get())[2]) == 0 }
}

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static HHDM: Request<HhdmResponse> = Request::new(0x48dcf1cb8ad2b852, 0x63984e959a98244b);

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static FRAMEBUFFER: Request<FramebufferResponse> = Request::new(0x9d5827dcd881dd75, 0xa3148604f6fab11b);

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static MEMMAP: Request<MemmapResponse> = Request::new(0x67cf3d9d378a806f, 0xe304acdfc50c3c62);

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static RSDP: Request<RsdpResponse> = Request::new(0xc5e77b6b397e7b43, 0x27637845accdcf3c);

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static BOOT_TIME: Request<BootTimeResponse> = Request::new(0x502746e184c088aa, 0xfbc5ec83e6327893);

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static KERNEL_ADDRESS: Request<KernelAddressResponse> = Request::new(0x71ba76863cc55f63, 0xb2644a48c516a487);

/// Ask for a 256 KiB boot stack (the default is only 64 KiB).
#[used]
#[unsafe(link_section = ".limine_requests")]
static STACK_SIZE: StackSizeRequest = StackSizeRequest {
    id: [MAGIC_A, MAGIC_B, 0x224ef0460a8e8926, 0xe1cb0fc25f46ea3d],
    revision: 0,
    response: UnsafeCell::new(ptr::null()),
    size: 256 * 1024,
};
#[repr(C)]
struct StackSizeRequest { id: [u64; 4], revision: u64, response: UnsafeCell<*const StackSizeResponse>, size: u64 }
unsafe impl Sync for StackSizeRequest {}

#[used]
#[unsafe(link_section = ".limine_requests_end")]
static END_MARKER: [u64; 2] = [0xadc0e0531bb10d03, 0x9572709f31764c62];

/// Offset where Limine mapped all of physical memory ("higher half direct map").
/// physical address P can be read at virtual address P + hhdm_offset().
pub fn hhdm_offset() -> u64 {
    HHDM.get().map(|h| h.offset).unwrap_or(0)
}
