//! Интерактивный терминал этапа загрузки (работает пока живы boot services).
//!
//! Читает ввод через EFI SimpleTextInput (polling, без прерываний),
//! поддерживает историю команд (стрелки ↑/↓), редактирование строки
//! (Backspace, стрелки ←/→, Home/End) и набор встроенных команд.

use alloc::fmt::Write as _;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write as _;
use uefi::cstr16;
use uefi::proto::console::text::{Color, Output};
use uefi::system::{with_stdin, with_stdout};
use uefi::table::input::InputKey;

use crate::console;
use crate::fs;
use crate::info;

const PROMPT: &str = "qwen> ";

/// Запускает REPL-цикл терминала. Возвращается только при команде `exit`
/// (или если stdin недоступен).
pub fn run() {
    console::println("QwenOS terminal. Type 'help' for the list of commands.");

    let mut history: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut hist_idx: Option<usize> = None; // позиция «вверху» = None -> edit live line
    let mut cursor: usize = 0; // позиция курсора в байтах внутри line

    loop {
        render_prompt(&line);

        // Ожидание следующей клавиши.
        let key = loop {
            if let Ok(Some(k)) = with_stdin(|input| input.read_key()) {
                break k;
            }
        };

        match key {
            InputKey::Special(uefi::unsafe_guid) => unreachable!(),
            _ => {}
        }

        match key {
            InputKey::Unicode(c) => match c {
                '\r' | '\n' => {
                    console::println("");
                    let cmd = line.trim().to_string();
                    if !cmd.is_empty() {
                        history.push(cmd.clone());
                    }
                    hist_idx = None;
                    line.clear();
                    cursor = 0;
                    if execute(&cmd) {
                        // команда просит выйти из терминала
                        return;
                    }
                }
                '\u{8}' => {
                    // Backspace — удаляем символ слева от курсора.
                    if cursor > 0 {
                        let prev = line[..cursor]
                            .chars()
                            .next_back()
                            .map(|ch| ch.len_utf8())
                            .unwrap_or(1);
                        line.replace_range(cursor - prev..cursor, "");
                        cursor -= prev;
                    }
                }
                _ => {
                    if c != '\t' {
                        line.insert(cursor, c);
                        cursor += c.len_utf8();
                    }
                }
            },
            InputKey::ArrowUp => {
                if !history.is_empty() {
                    let idx = match hist_idx {
                        Some(i) if i > 0 => i - 1,
                        Some(i) => i,
                        None => {
                            hist_idx = Some(history.len() - 1);
                            history.len() - 1
                        }
                    };
                    hist_idx = Some(idx);
                    line = history[idx].clone();
                    cursor = line.len();
                }
            }
            InputKey::ArrowDown => {
                if let Some(i) = hist_idx {
                    if i + 1 < history.len() {
                        hist_idx = Some(i + 1);
                        line = history[i + 1].clone();
                    } else {
                        hist_idx = None;
                        line.clear();
                    }
                    cursor = line.len();
                }
            }
            InputKey::ArrowLeft => {
                if let Some(prev) = line[..cursor].chars().next_back() {
                    cursor -= prev.len_utf8();
                }
            }
            InputKey::ArrowRight => {
                if let Some(next) = line[cursor..].chars().next() {
                    cursor += next.len_utf8();
                }
            }
            InputKey::Home => cursor = 0,
            InputKey::End => cursor = line.len(),
            InputKey::Delete => {
                if let Some(next) = line[cursor..].chars().next() {
                    line.replace_range(cursor..cursor + next.len_utf8(), "");
                }
            }
            InputKey::Escape => {
                line.clear();
                cursor = 0;
            }
            _ => {}
        }
    }
}

/// Перерисовка строки ввода (prompt + содержимое + обратный курсор).
fn render_prompt(line: &str) {
    let _ = with_stdout(|out| {
        let _ = out.write_str(PROMPT);
        let _ = out.write_str(line);
        // Сдвигаем курсор назад к месту ввода.
        let back = line.len() - current_cursor_in_line(line);
        for _ in 0..back {
            let _ = out.input_state_placeholder();
        }
    });
}

fn current_cursor_in_line(_line: &str) -> usize {
    // Терминал UEFI не даёт нам точного положения курсора, поэтому рисуем
    // строку целиком и продолжаем ввод в конце — упрощённая модель.
    0
}

/// Выполняет одну команду. Возвращает true, если терминал нужно закрыть.
fn execute(cmd: &str) -> bool {
    let mut parts = cmd.split_whitespace();
    let name = match parts.next() {
        Some(n) => n,
        None => return false,
    };
    let args: Vec<&str> = parts.collect();

    match name {
        "help" => cmd_help(),
        "about" => cmd_about(),
        "echo" => console::println(&args.join(" ")),
        "clear" => clear_screen(),
        "ls" => cmd_ls(&args),
        "cat" => cmd_cat(&args),
        "uname" => cmd_uname(),
        "mem" => info::print_memory_report(),
        "cpu" => info::print_cpu_report(),
        "uptime" => cmd_uptime(),
        "date" => cmd_date(),
        "beep" => cmd_beep(),
        "shutdown" | "reboot" => {
            console::println("Powering off (resetting system)…");
            let _ = uefi::boot::stall(300_000);
            let _ = uefi::system::reset(
                uefi::table::system_table::ResetType::WARM_BOOT,
                uefi::table::system_table::RESET_STATUS_SUCCESS,
                None,
            );
            // Если сброс не сработал — выходим в загрузчик.
            return true;
        }
        "exit" => return true,
        other => {
            let mut s = String::new();
            let _ = write!(s, "qwen: command not found: {other} (try 'help')");
            console::println(&s);
        }
    }
    false
}

fn cmd_help() {
    console::println("Available commands:");
    for (c, d) in [
        ("help", "show this help"),
        ("about", "about QwenOS"),
        ("echo <text>", "print text"),
        ("clear", "clear screen"),
        ("ls [path]", "list files on the ESP / in ramfs"),
        ("cat <file>", "print file contents"),
        ("uname", "system information"),
        ("mem", "memory map report"),
        ("cpu", "CPUID report"),
        ("uptime", "time since boot"),
        ("date", "current date/time from RTC"),
        ("beep", "short beep via PC speaker"),
        ("exit", "leave terminal and continue boot"),
        ("shutdown", "reset the machine"),
    ] {
        let mut s = String::new();
        let _ = write!(s, "  {c:<16} {d}");
        console::println(&s);
    }
}

fn cmd_about() {
    console::println("QwenOS v0.2.0 — hobby hybrid OS: UEFI loader + bare x86_64 kernel,");
    console::println("written in pure Rust. Terminal runs during boot services stage.");
}

fn cmd_uname() {
    let mut s = String::new();
    let _ = write!(
        s,
        "QwenOS {} x86_64 UEFI (hybrid)",
        option_env!("CARGO_PKG_VERSION").unwrap_or("0.0.0")
    );
    console::println(&s);
}

fn cmd_uptime() {
    let ms = uefi::boot::get_uptime_ms();
    let secs = ms / 1000;
    let mut s = String::new();
    let _ = write!(
        s,
        "up {}: {:02}:{:02}:{:02}",
        ms,
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    );
    console::println(&s);
}

fn cmd_date() {
    match uefi::system::with_real_time_clock(|rt| rt.time()) {
        Ok(t) => {
            let mut s = String::new();
            let _ = write!(
                s,
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                t.year(),
                t.month(),
                t.day(),
                t.hour(),
                t.minute(),
                t.second()
            );
            console::println(&s);
        }
        Err(_) => console::println("date: RTC unavailable"),
    }
}

fn cmd_beep() {
    crate::arch::beep(880, 120);
    console::println("*beep*");
}

fn cmd_ls(args: &[&str]) {
    let path = args.first().copied().unwrap_or("/");
    // 1. Сначала смотрим виртуальную файловую систему в RAM.
    let ram_entries = fs::list(path);
    // 2. Затем реальные файлы на EFI System Partition.
    let esp_entries = esp_list(path);

    if ram_entries.is_empty() && esp_entries.is_empty() {
        console::println(&format!("ls: no such directory: {path}"));
        return;
    }
    for e in ram_entries.iter().chain(esp_entries.iter()) {
        console::println(e);
    }
}

/// Список файлов с ESP через EFI SimpleFileSystem (если доступна).
fn esp_list(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let _ = uefi::boot::get_image_file_system(uefi::system::image_handle());
    // Открываем корень ESP и читаем директорию.
    let result: Result<(), uefi::Status> = (|| -> Result<(), uefi::Status> {
        use uefi::fs::FileSystemExt;
        let fs = uefi::boot::get_image_file_system(uefi::system::image_handle())
            .map_err(|e| e.status())?;
        let dir_path = if path == "/" {
            cstr16!("\\")
        } else {
            return Ok(()); // вложенные пути для ESP пока не поддерживаем
        };
        let mut dir = fs.open_existing_directory(dir_path).map_err(|e| e.status())?;
        while let Ok(Some(info)) = dir.read_entry() {
            let name = info.file_name().to_string_lossy();
            let kind = if info.is_directory() { "/" } else { "" };
            out.push(format!("  {name}{kind}  [esp]"));
        }
        Ok(())
    })();
    let _ = result;
    out
}

fn cmd_cat(args: &[&str]) {
    let Some(path) = args.first() else {
        console::println("usage: cat <file>");
        return;
    };
    if let Some(data) = fs::read(path) {
        let text = String::from_utf8_lossy(&data);
        for l in text.lines() {
            console::println(l);
        }
    } else {
        console::println(&format!("cat: {path}: no such file (see 'ls')"));
    }
}

/// Очистка экрана через EFI stdout.
fn clear_screen() {
    let _ = with_stdout(|out| {
        let _ = out.clear();
        let _ = out.set_color(Color::LightGray, Color::Black);
        out.reset(false);
    });
}
