//! Turning the computer off and restarting it, via ACPI.
//!
//! ACPI is a set of tables the firmware leaves in memory describing the
//! machine. To power off ("sleep state S5") we need two things:
//!  * from the FADT table: the I/O port of the "PM1 control" register;
//!  * from the DSDT table: the magic number for S5, stored in a little
//!    package named `_S5_` (written in ACPI's bytecode language, AML).
//! Then we write (S5 number << 10) | "sleep enable" bit to that port.
//! A real AML interpreter is thousands of lines; scanning for `_S5_` is the
//! classic hobby-OS shortcut and works on most machines, including ThinkPads.

use super::cpu::{self, inb, outb, outw};
use crate::{boot, log, mem::paging};

struct Acpi { pm1a: u16, pm1b: u16, slp_a: u16, slp_b: u16, smi_cmd: u16, acpi_enable: u8, reset_port: u16, reset_val: u8 }

/// Read `len` bytes of a physical address (ACPI tables aren't in Limine's direct map).
fn phys_slice(addr: u64, len: usize) -> Option<&'static [u8]> {
    let p = paging::map_mmio(addr, len as u64).ok()?;
    Some(unsafe { core::slice::from_raw_parts(p, len) })
}

fn table(addr: u64) -> Option<&'static [u8]> {
    let hdr = phys_slice(addr, 36)?;
    let len = u32::from_le_bytes(hdr[4..8].try_into().ok()?) as usize;
    if !(36..=4 << 20).contains(&len) { return None; }
    phys_slice(addr, len)
}

fn find_acpi() -> Option<Acpi> {
    let r = boot::RSDP.get()?;
    let hhdm = boot::hhdm_offset();
    let rsdp_phys = if r.address >= hhdm { r.address - hhdm } else { r.address };
    let rsdp = phys_slice(rsdp_phys, 36)?;
    if &rsdp[0..8] != b"RSD PTR " { return None; }
    let (root, wide) = if rsdp[15] >= 2 {
        (u64::from_le_bytes(rsdp[24..32].try_into().ok()?), true)
    } else {
        (u32::from_le_bytes(rsdp[16..20].try_into().ok()?) as u64, false)
    };
    let rt = table(root)?;
    let n = (rt.len() - 36) / if wide { 8 } else { 4 };
    let mut fadt = None;
    for i in 0..n {
        let a = if wide { u64::from_le_bytes(rt[36 + i * 8..44 + i * 8].try_into().ok()?) }
                else { u32::from_le_bytes(rt[36 + i * 4..40 + i * 4].try_into().ok()?) as u64 };
        let t = phys_slice(a, 4)?;
        if t == b"FACP" { fadt = table(a); break; }
    }
    let f = fadt?;
    let u32at = |o: usize| u32::from_le_bytes(f[o..o + 4].try_into().unwrap());
    let mut dsdt_addr = u32at(40) as u64;
    if f.len() >= 148 { let x = u64::from_le_bytes(f[140..148].try_into().unwrap()); if x != 0 { dsdt_addr = x; } }
    let dsdt = table(dsdt_addr)?;
    // Find the _S5_ package and read its first two numbers.
    let pos = dsdt.windows(4).position(|w| w == b"_S5_")?;
    let mut i = pos + 4;
    if dsdt.get(i)? != &0x12 { return None; } // PackageOp
    i += 1;
    i += ((dsdt[i] >> 6) as usize) + 1;        // skip PkgLength
    i += 1;                                     // skip NumElements
    let read_num = |i: &mut usize| -> u16 {
        match dsdt[*i] { 0x0A => { *i += 2; dsdt[*i - 1] as u16 } 0x01 => { *i += 1; 1 } _ => { *i += 1; 0 } }
    };
    let slp_a = read_num(&mut i);
    let slp_b = read_num(&mut i);
    let flags = u32at(112);
    let (reset_port, reset_val) = if flags & (1 << 10) != 0 && f.len() >= 129 && f[116] == 1 {
        (u64::from_le_bytes(f[120..128].try_into().unwrap()) as u16, f[128])
    } else { (0, 0) };
    Some(Acpi {
        pm1a: u32at(64) as u16, pm1b: u32at(68) as u16, slp_a, slp_b,
        smi_cmd: u32at(48) as u16, acpi_enable: f[52], reset_port, reset_val,
    })
}

pub fn power_off() -> ! {
    cpu::cli();
    if let Some(a) = find_acpi() {
        log!("power: ACPI S5 = {}/{}, PM1a port {:#x}", a.slp_a, a.slp_b, a.pm1a);
        // Make sure the firmware has handed power management to the OS ("ACPI mode").
        if super::cpu::inw(a.pm1a) & 1 == 0 && a.smi_cmd != 0 && a.acpi_enable != 0 {
            outb(a.smi_cmd, a.acpi_enable);
            crate::time::spin_us(100_000);
        }
        outw(a.pm1a, (a.slp_a << 10) | (1 << 13));
        if a.pm1b != 0 { outw(a.pm1b, (a.slp_b << 10) | (1 << 13)); }
        crate::time::spin_us(500_000);
    }
    outw(0x604, 0x2000); // QEMU's shortcut
    log!("power: could not turn off");
    cpu::halt_forever();
}

pub fn restart() -> ! {
    cpu::cli();
    if let Some(a) = find_acpi() { if a.reset_port != 0 { outb(a.reset_port, a.reset_val); crate::time::spin_us(100_000); } }
    outb(0xCF9, 0x02); outb(0xCF9, 0x06);              // Intel chipset reset register
    crate::time::spin_us(100_000);
    for _ in 0..10_000 { if inb(0x64) & 2 == 0 { break; } }
    outb(0x64, 0xFE);                                   // keyboard controller reset pulse
    crate::time::spin_us(100_000);
    // Last resort: load an empty interrupt table and cause an interrupt (triple fault).
    unsafe {
        let null: [u8; 10] = [0; 10];
        core::arch::asm!("lidt [{}]", "int3", in(reg) &null);
    }
    cpu::halt_forever();
}
