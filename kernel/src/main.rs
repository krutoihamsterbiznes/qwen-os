//! QwenOS — UEFI-загрузчик / точка входа ядра.
//!
//! Гибридная модель: на этапе загрузки код работает как EFI-приложение
//! (сервисы UEFI: аллокатор, логгер, text-mode вывод), а после выхода из
//! boot services управление передаётся платформенному коду x86_64 без
//! использования runtime-сервисов UEFI.

#![no_main]
#![no_std]

extern crate alloc;

mod arch;
mod boot;
mod console;
mod info;

use uefi::prelude::*;

/// Точка входа EFI-приложения. Аргументы (image handle и SystemTable)
/// подставляет макрос `#[entry]` и регистрирует системную таблицу глобально.
#[entry]
fn main() -> Status {
    uefi::init();
    log::info!("QwenOS: старт (UEFI, x86_64, hybrid)");

    if let Err(status) = main_body() {
        log::error!("Ошибка загрузки: {:?}", status);
        return status;
    }

    // Выход из boot services. Полученная карта памяти описывает всё
    // доступное ядру физическое пространство.
    // Примечание: кастомный тип памяти не задаём, чтобы не трогать
    // код ядра, который может лежать в BOOTSERVICES_CODE/DATA.
    let mmap = unsafe { uefi::boot::exit_boot_services(None) };

    // Передаём управление «ядру». Возврат из kernel_main не ожидается.
    unsafe { boot::kernel_main(&mmap) }
}

fn main_body() -> Result<()> {
    // 1. Информация о системе (firmware, память, CPU).
    info::print_system_info();

    // 2. Ждём нажатие клавиши, чтобы экран можно было прочитать.
    wait_for_key()?;

    Ok(())
}

/// Блокирующее ожидание нажатия любой клавиши (SimpleTextInput, polling).
fn wait_for_key() -> Result<()> {
    log::info!("Нажмите любую клавишу для продолжения загрузки…");
    uefi::system::with_stdin(|input| {
        loop {
            if input.read_key().ok().flatten().is_some() {
                break;
            }
        }
        Ok(())
    })
}
