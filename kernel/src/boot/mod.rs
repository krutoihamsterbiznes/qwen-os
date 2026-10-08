//! Пост-загрузочная часть ядра: работает уже ПОСЛЕ exit_boot_services().
//!
//! Это «гибридная» сторона ОС: никакого UEFI runtime, только прямые
//! обращения к железу x86_64 (VGA-текст-буфер, порты, CPUID).

pub mod console;

use uefi::mem::memory_map::{MemoryMap as _, MemoryMapOwned};

use crate::arch;

/// Точка входа ядра после выхода из boot services.
///
/// # Safety
/// Вызывается ровно один раз сразу после `exit_boot_services`.
pub unsafe fn kernel_main(mmap: &MemoryMapOwned) -> ! {
    // Минимальная инициализация платформы.
    arch::init();

    console::clear();
    console::write_str("QwenOS kernel: boot services exited, running on bare x86_64.\n");

    console::write_str("Memory map entries: ");
    let mut buf = [0u8; 8];
    let n = format_usize(mmap.len(), &mut buf);
    console::write_bytes(n);
    console::write_str("\n");

    console::write_str("CPU vendor: ");
    let mut vbuf = [0u8; 13];
    arch::cpuid_vendor(&mut vbuf);
    console::write_bytes(&vbuf[..12]);
    console::write_str("\n");
    console::write_str("Halt loop engaged.\n");

    loop {
        arch::halt();
    }
}

/// Простейший формат числа в десятичную строку (без аллокатора).
fn format_usize(mut value: usize, buf: &mut [u8; 8]) -> &[u8] {
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    &buf[i..]
}
