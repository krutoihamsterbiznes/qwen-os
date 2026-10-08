//! Сбор информации о системе на этапе boot services.

use alloc::fmt::Write as _;
use alloc::string::String;
use uefi::boot;
use uefi::mem::memory_map::{MemoryMap as _, MemoryMapMut as _};
use uefi::system;

use crate::console;

/// Печатает сведения о firmware, памяти и CPU.
pub fn print_system_info() {
    console::println("=== QwenOS :: system info ===");

    let fw = system::firmware_revision();
    let rev = system::uefi_revision();
    let mut line = String::new();
    let _ = write!(
        line,
        "UEFI {}.{} (firmware rev {}.{})",
        rev.major(),
        rev.minor(),
        fw >> 16,
        fw & 0xffff
    );
    console::println(&line);

    let mut line = String::new();
    let _ = write!(line, "Firmware vendor: {}", system::firmware_vendor());
    console::println(&line);

    // Объём доступной памяти по карте памяти boot services.
    if let Ok(mmap) = boot::memory_map(uefi::boot::MemoryType::CONVENTIONAL) {
        let mut total = 0usize;
        for desc in mmap.entries() {
            total += desc.num_pages() as usize * uefi::boot::PAGE_SIZE;
        }
        let mut line = String::new();
        let _ = write!(line, "Usable RAM (conventional): {} MiB", total >> 20);
        console::println(&line);
    }

    // CPU через CPUID.
    let mut buf = [0u8; 13];
    crate::arch::cpuid_vendor(&mut buf);
    let vendor = core::str::from_utf8(&buf[..12]).unwrap_or("????????????");
    let mut line = String::new();
    let _ = write!(line, "CPU vendor string: {}", vendor);
    console::println(&line);

    console::println("===========================");
}
