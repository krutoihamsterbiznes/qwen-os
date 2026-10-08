//! Точка входа QwenOS: UEFI-приложение, которое поднимает GUI-ядро.
//!
//! Конвейер: `efi_main` → инициализация uefi-сервисов → поиск GOP (framebuffer)
//! → сбор информации о памяти/CPU → `exit_boot_services` → голый x86_64-код ядра
//! (GDT/IDT/PIC/PIT, PS/2, VGA) → графический рабочий стол.
//!
//! Запуск: `cargo run` (см. `.cargo/config.toml`: target x86_64-unknown-uefi + runner QEMU).

#![no_main]
#![no_std]

use uefi::prelude::*;
use uefi::proto::console::gop::{GraphicsOutput, PixelFormat};

mod arch;
mod drivers;
mod gfx;
mod gui;
mod vga;

/// Версия и строки «о системе», используемые терминалом и окном Info.
pub const OS_VERSION: &str = "QwenOS 0.3 \"Aurora\"";

/// Статичные строки системной информации (заполняются при старте).
pub static mut SYS_INFO_LINES: [&str; 6] = [
    "QwenOS 0.3 Aurora",
    "CPU: ...",
    "Memory: ...",
    "Video: ...",
    "Input: PS/2 kbd+mouse",
    "Kernel: Rust, x86_64, UEFI->bare-metal",
];

/// Данные, которые ядро забирает с собой после ExitBootServices.
#[derive(Clone, Copy)]
pub struct KernelInfo {
    pub fb_base: usize,
    pub fb_width: usize,
    pub fb_height: usize,
    pub fb_pitch: usize,
    pub is_bgr: bool,
    pub total_mib: usize,
    pub conv_mib: usize,
}

static mut KERNEL_INFO: KernelInfo = KernelInfo {
    fb_base: 0,
    fb_width: 0,
    fb_height: 0,
    fb_pitch: 0,
    is_bgr: true,
    total_mib: 0,
    conv_mib: 0,
};

/// Ранний печатный лог в VGA text mode — виден сразу после загрузки ядра.
fn kprint(s: &str) {
    vga::set_color_fg_bg(vga::LIGHT_GRAY, vga::BLACK);
    vga::write_str(s);
}

fn khalt() -> ! {
    loop {
        arch::disable_interrupts();
        arch::halt();
    }
}

#[entry]
fn efi_main() -> Status {
    let mut st = uefi::init();

    // Ищем GOP — без него GUI невозможен.
    let mut fb = KernelInfo {
        fb_base: 0,
        fb_width: 0,
        fb_height: 0,
        fb_pitch: 0,
        is_bgr: true,
        total_mib: 0,
        conv_mib: 0,
    };
    {
        let handles = uefi::boot::locate_handle_buffer(uefi::table::boot::SearchType::AllHandles);
        match handles {
            Ok(handles) => {
                for h in handles.iter() {
                    if let Ok(gop) =
                        uefi::boot::open_protocol_exclusive::<GraphicsOutput>(*h)
                    {
                        let info = gop.current_mode_info();
                        let (w, hpx) = info.resolution();
                        fb.fb_width = w;
                        fb.fb_height = hpx;
                        fb.fb_pitch = info.stride() * 4;
                        fb.is_bgr = matches!(
                            info.pixel_format(),
                            PixelFormat::BGR | PixelFormat::BGRA
                        );
                        fb.fb_base = gop.frame_buffer().as_mut_ptr() as usize;
                        break;
                    }
                }
            }
            Err(_) => {}
        }
    }
    if fb.fb_base == 0 {
        uefi::boot::warn!("QwenOS: GraphicsOutput (GOP) not found — GUI requires video output.");
        return Status::SUPPORT;
    }

    // Память: суммируем conventional / всю карту ДО выхода из boot services.
    {
        let mut storage = [0u8; 64 * 1024];
        if let Ok((_, map)) = unsafe { uefi::boot::memory_map(&mut storage) } {
            let (total, conv) = sum_memory(&map);
            fb.total_mib = total / (1024 * 1024);
            fb.conv_mib = conv / (1024 * 1024);
        }
    }

    unsafe {
        KERNEL_INFO = fb;
    }

    // Баннер через EFI stdout (пока есть протоколы).
    uefi::boot::info!("{} — UEFI hybrid kernel (x86_64, Rust)", OS_VERSION);
    uefi::boot::info!(
        "GOP: {}x{} pitch={} fmt={}",
        fb.fb_width,
        fb.fb_height,
        fb.fb_pitch,
        if fb.is_bgr { "BGRx" } else { "RGBx" }
    );
    uefi::boot::info!("RAM: ~{} MiB total, {} MiB conventional", fb.total_mib, fb.conv_mib);

    // ---------- выходим из boot services ----------
    if uefi::boot::exit_boot_services().is_err() {
        // stdout уже недоступен — просто зависнем.
        khalt();
    }

    kernel_entry();
}

fn sum_memory(map: &uefi::mem::memory_map::MemoryMap) -> (usize, usize) {
    let mut total = 0usize;
    let mut conv = 0usize;
    for d in map.iter() {
        let sz = d.page_count as usize * 4096;
        total += sz;
        if d.ty == uefi::table::boot::MemoryType::CONVENTIONAL {
            conv += sz;
        }
    }
    (total, conv)
}

// ==================== ЯДРО ====================

/// Точка входа голого ядра после ExitBootServices.
fn kernel_entry() -> ! {
    // 1. VGA text console — ранние сообщения.
    vga::clear();
    kprint("QwenOS kernel: stage 1 - GDT/IDT/PIC/PIT\n");
    unsafe {
        arch::load_gdt();
        arch::install_idt();
    }
    arch::pic_remap();
    arch::pit_start(100);
    arch::enable_interrupts();

    // 2. Информация о системе → заполняем SYS_INFO_LINES.
    let mut vendor = [0u8; 13];
    arch::cpuid_vendor(&mut vendor);
    let vi = core::str::from_utf8(&vendor[..12]).unwrap_or("???");
    let ki = unsafe { KERNEL_INFO };

    static mut LINES_BUF: [[u8; 64]; 6] = [[0; 64]; 6];
    unsafe {
        use core::fmt::Write;
        let mut lines = [""; 6];

        LINES_BUF[0] = [0; 64];
        let _ = write!(&mut LINES_BUF[0][..], "{}\0", OS_VERSION);
        LINES_BUF[1] = [0; 64];
        let _ = write!(&mut LINES_BUF[1][..], "CPU: {}\0", vi);
        LINES_BUF[2] = [0; 64];
        let _ = write!(&mut LINES_BUF[2][..], "RAM: {} MiB total\0", ki.total_mib);
        LINES_BUF[3] = [0; 64];
        let _ = write!(
            &mut LINES_BUF[3][..],
            "Video: {}x{} GOP\0",
            ki.fb_width, ki.fb_height
        );
        LINES_BUF[4] = [0; 64];
        let _ = write!(&mut LINES_BUF[4][..], "Input: PS/2 kbd + aux mouse\0");
        LINES_BUF[5] = [0; 64];
        let _ = write!(&mut LINES_BUF[5][..], "Kernel: Rust x86_64 UEFI->ELF-ish\0");
        for i in 0..6 {
            let l = LINES_BUF[i].iter().position(|&c| c == 0).unwrap_or(63);
            lines[i] = core::str::from_utf8_unchecked(&LINES_BUF[i][..l]);
        }
        SYS_INFO_LINES = lines;
    }

    kprint("CPU vendor: ");
    kprint(vi);
    kprint("\n");
    kprint("FB: ");
    let mut b = [0u8; 24];
    kprint(arch::isr::itoa(ki.fb_width as u64, &mut b));
    kprint("x");
    kprint(arch::isr::itoa(ki.fb_height as u64, &mut b));
    kprint(" RAM: ");
    kprint(arch::isr::itoa(ki.total_mib as u64, &mut b));
    kprint(" MiB\n");

    // 3. Драйверы ввода.
    let mouse_ok = drivers::mouse::init();
    kprint(if mouse_ok {
        "PS/2 mouse: OK\n"
    } else {
        "PS/2 mouse: absent\n"
    });
    kprint("PS/2 keyboard: IRQ1 armed\n");

    // 4. Инициализация ramfs.
    drivers::vfs::init();
    kprint("RAM-FS mounted (readme.txt, hello.txt, about.txt, config.ini)\n");

    // 5. Графика: поднимаем framebuffer и рисуем рабочий стол.
    unsafe {
        gfx::fb::init(
            ki.fb_base as *mut u8,
            ki.fb_width,
            ki.fb_height,
            ki.fb_pitch,
            ki.is_bgr,
        );
    }
    kprint("Framebuffer ready. Launching desktop...\n");
    arch::stall_ms(150);

    // 6. Главный цикл GUI.
    gui::desktop::run();
}

// Паника: используем panic_handler crate `uefi` (фича "panic_handler") —
// он печатает сообщение в EFI stdout. После exit_boot_services stdout нет,
// поэтому на стадии ядра панику лучше не допускать; при исключении ЦП
// срабатывает собственный экран паники в arch::isr::exception_panic (VGA).
