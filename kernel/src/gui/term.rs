//! Графический терминал внутри окна: вывод текста, ввод команд с клавиатуры.

use core::fmt::Write as _;

use crate::drivers::vfs;
use crate::gfx::fb::{fill_rect, Color};
use crate::gfx::font;

pub const SCALE: usize = 2;
pub const CW: usize = 6 * SCALE; // клетка символа по X
pub const CH: usize = 9 * SCALE; // шаг строки по Y

const MAX_LINES: usize = 100;
const LINE_LEN: usize = 120;
const INPUT_MAX: usize = 100;

static mut LINES: [[u8; LINE_LEN]; MAX_LINES] = [[0; LINE_LEN]; MAX_LINES];
static mut LENS: [usize; MAX_LINES] = [0; MAX_LINES];
static mut NLINES: usize = 0;

static mut INPUT: [u8; INPUT_MAX] = [0; INPUT_MAX];
static mut INPUT_LEN: usize = 0;

/// История команд (стрелки вверх/вниз).
static mut HISTORY: [[u8; LINE_LEN]; 16] = [[0; LINE_LEN]; 16];
static mut HIST_LEN: usize = 0;
static mut HIST_POS: usize = 0; // == HIST_LEN — «новая строка»

const PROMPT: &str = "qwen> ";

// ---------- вывод ----------

pub fn print_line(s: &str) {
    for part in s.split('\n') {
        push_row(part.as_bytes());
    }
}

fn push_row(bytes: &[u8]) {
    unsafe {
        if NLINES >= MAX_LINES {
            for i in 1..MAX_LINES {
                LINES[i - 1] = LINES[i];
                LENS[i - 1] = LENS[i];
            }
            NLINES = MAX_LINES - 1;
        }
        let idx = NLINES;
        let l = bytes.len().min(LINE_LEN);
        LINES[idx] = [0; LINE_LEN];
        LINES[idx][..l].copy_from_slice(&bytes[..l]);
        LENS[idx] = l;
        NLINES += 1;
    }
}

pub fn clear_screen() {
    unsafe { NLINES = 0 }
}

// ---------- ввод ----------

pub fn input_char(c: char) {
    unsafe {
        if INPUT_LEN < INPUT_MAX - 1 && (c as u32) >= 32 && (c as u32) < 127 {
            INPUT[INPUT_LEN] = c as u8;
            INPUT_LEN += 1;
            HIST_POS = HIST_LEN;
        }
    }
}

pub fn input_backspace() {
    unsafe { INPUT_LEN = INPUT_LEN.saturating_sub(1) }
}

/// delta = -1 (вверх по истории), +1 (вниз).
pub fn input_history(delta: i32) {
    unsafe {
        if HIST_LEN == 0 {
            return;
        }
        if delta < 0 {
            if HIST_POS > 0 {
                HIST_POS -= 1;
            }
        } else if HIST_POS < HIST_LEN {
            HIST_POS += 1;
        }
        if HIST_POS == HIST_LEN {
            INPUT_LEN = 0;
        } else {
            let l = HISTORY[HIST_POS][0] as usize;
            INPUT[..l].copy_from_slice(&HISTORY[HIST_POS][1..1 + l]);
            INPUT_LEN = l;
        }
    }
}

/// Забрать строку ввода, очистить её, сохранить в историю.
/// Копия кладётся в ротационный статический слот — возвращаем 'static str.
fn take_input_and_echo() -> &'static str {
    static mut SLOT: [[u8; LINE_LEN]; 4] = [[0; LINE_LEN]; 4];
    static mut SLOT_I: usize = 0;
    unsafe {
        let l = INPUT_LEN.min(LINE_LEN - 1);
        SLOT_I = (SLOT_I + 1) % 4;
        SLOT[SLOT_I] = [0; LINE_LEN];
        SLOT[SLOT_I][..l].copy_from_slice(&INPUT[..l]);
        INPUT_LEN = 0;
        if l > 0 {
            let h = HIST_LEN % 16;
            HISTORY[h] = [0; LINE_LEN];
            HISTORY[h][0] = l as u8;
            HISTORY[h][1..1 + l].copy_from_slice(&SLOT[SLOT_I][..l]);
            HIST_LEN = (HIST_LEN + 1).min(16);
            HIST_POS = HIST_LEN;
        }
        core::str::from_utf8(&SLOT[SLOT_I][..l]).unwrap_or("")
    }
}

/// Обработать Enter: эхо команды и исполнение.
pub fn submit(on_open_app: &mut dyn FnMut(&str)) {
    let cmd = take_input_and_echo();
    let mut sb = Sb::<256>::new();
    let _ = write!(sb, "{}{}", PROMPT, cmd);
    print_line(sb.as_str());
    execute(cmd, on_open_app);
}

// ---------- форматирование в стековый буфер (без аллокатора) ----------

struct Sb<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> Sb<N> {
    fn new() -> Self {
        Sb { buf: [0; N], len: 0 }
    }
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl<const N: usize> Write for Sb<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let b = s.as_bytes();
        let room = N - self.len;
        let l = b.len().min(room);
        self.buf[self.len..self.len + l].copy_from_slice(&b[..l]);
        self.len += l;
        Ok(())
    }
}

// ---------- интерпретатор ----------

fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    for b in s.as_bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v * 10 + (b - b'0') as u64;
        if v > u32::MAX as u64 {
            return None;
        }
    }
    Some(v as u32)
}

fn split_args(cmd: &str) -> ([&str; 4], &str) {
    let mut args = [""; 4];
    let mut it = cmd.split_whitespace().skip(1);
    for i in 0..4 {
        if let Some(a) = it.next() {
            args[i] = a;
        }
    }
    let rest = cmd.splitn(2, ' ').nth(1).unwrap_or("");
    (args, rest)
}

fn execute(cmd: &str, on_open_app: &mut dyn FnMut(&str)) {
    let name = cmd.split_whitespace().next().unwrap_or("");
    let (args, rest) = split_args(cmd);

    match name {
        "" => {}
        "help" => {
            print_line("Commands:");
            print_line("  help               this list");
            print_line("  echo <text>        print text back");
            print_line("  ver                OS version");
            print_line("  info               CPU / memory / video");
            print_line("  date | uptime      clock since boot");
            print_line("  ls                 list ramfs files");
            print_line("  cat <file>         show file contents");
            print_line("  write <f> <text>   create/overwrite file");
            print_line("  rm <file>          delete file");
            print_line("  open <app>         info|files|logo");
            print_line("  cls | clear        clear screen");
            print_line("  beep [hz] [ms]     PC speaker tone");
            print_line("  paint              framebuffer demo art");
            print_line("  shutdown           power off QEMU");
            print_line("  reboot             reset machine");
        }
        "echo" => print_line(rest),
        "ver" => print_line("QwenOS 0.2.0-dev (UEFI hybrid kernel, x86_64, Rust)"),
        "info" => {
            print_line(crate::SYS_INFO_LINE1);
            print_line(crate::SYS_INFO_LINE2);
            print_line(crate::SYS_INFO_LINE3);
        }
        "date" | "uptime" => {
            let ticks = unsafe { crate::arch::isr::TICKS };
            let secs = ticks / 100;
            let mut sb = Sb::<128>::new();
            let _ = write!(sb, "up {}s ({} ticks @ 100Hz)", secs, ticks);
            print_line(sb.as_str());
        }
        "ls" => {
            let mut names: [&str; 32] = [""; 32];
            let n = vfs::list(&mut names, 32);
            if n == 0 {
                print_line("(empty fs)");
            }
            for fname in &names[..n] {
                let size = vfs::file_size(fname).unwrap_or(0);
                let mut sb = Sb::<160>::new();
                let _ = write!(sb, "{:<24} {:>6} B", fname, size);
                print_line(sb.as_str());
            }
        }
        "cat" => {
            if args[0].is_empty() {
                print_line("usage: cat <file>");
            } else {
                let mut buf = [0u8; 2048];
                match vfs::read_file(args[0], &mut buf) {
                    Some(data) => match core::str::from_utf8(data) {
                        Ok(s) => print_line(s.trim_end()),
                        Err(_) => print_line("<binary data>"),
                    },
                    None => {
                        let mut sb = Sb::<128>::new();
                        let _ = write!(sb, "cat: {}: no such file", args[0]);
                        print_line(sb.as_str());
                    }
                }
            }
        }
        "write" => {
            // write <file> <text...>
            let mut it = rest.splitn(2, ' ');
            let fname = it.next().unwrap_or("");
            let body = it.next().unwrap_or("");
            if fname.is_empty() {
                print_line("usage: write <file> <text>");
            } else if vfs::write_file(fname, body.as_bytes()) {
                let mut sb = Sb::<128>::new();
                let _ = write!(sb, "written {} bytes to {}", body.len(), fname);
                print_line(sb.as_str());
            } else {
                print_line("write: failed (bad name or fs full)");
            }
        }
        "rm" => {
            if !args[0].is_empty() && vfs::delete_file(args[0]) {
                let mut sb = Sb::<128>::new();
                let _ = write!(sb, "deleted {}", args[0]);
                print_line(sb.as_str());
            } else {
                print_line("rm: not found");
            }
        }
        "open" => match args[0] {
            "info" | "files" | "logo" | "about" => on_open_app(args[0]),
            "terminal" => print_line("terminal is already here"),
            "" => print_line("usage: open info|files|logo"),
            other => {
                let mut sb = Sb::<128>::new();
                let _ = write!(sb, "open: unknown app '{}'", other);
                print_line(sb.as_str());
            }
        },
        "cls" | "clear" => clear_screen(),
        "beep" => {
            let hz = parse_u32(args[0]).unwrap_or(880).clamp(50, 8000);
            let ms = parse_u32(args[1]).unwrap_or(150).min(1000);
            crate::arch::beep(hz, ms);
            let mut sb = Sb::<128>::new();
            let _ = write!(sb, "beep {}Hz {}ms", hz, ms);
            print_line(sb.as_str());
        }
        "paint" => {
            print_line("painting demo pattern...");
            crate::gui::desktop::paint_demo();
        }
        "shutdown" => {
            print_line("Powering down...");
            crate::arch::acpi_shutdown();
        }
        "reboot" => {
            print_line("Rebooting...");
            crate::arch::reset();
        }
        other => {
            let mut sb = Sb::<160>::new();
            let _ = write!(sb, "{}: command not found (try 'help')", other);
            print_line(sb.as_str());
            crate::arch::beep(220, 80);
        }
    }
}

// ---------- отрисовка ----------

/// Нарисовать терминал в клиентской области (cx, cy, w, h).
pub fn render(cx: usize, cy: usize, cw_px: usize, ch_px: usize) {
    fill_rect(cx, cy, cw_px, ch_px, Color::TERMINAL_BG);
    let rows = ch_px / CH;
    if rows == 0 {
        return;
    }
    unsafe {
        let start = NLINES.saturating_sub(rows - 1);
        for i in start..NLINES {
            let y = cy + (i - start) * CH;
            if y + CH > cy + ch_px {
                break;
            }
            if let Ok(s) = core::str::from_utf8(&LINES[i][..LENS[i]]) {
                font::draw_text(cx + 4, y + 2, s, SCALE, Color::TERMINAL_FG);
            }
        }
        // строка ввода — всегда последней видимой строкой
        let row = (NLINES - start).min(rows - 1);
        let y = cy + row * CH;
        let px = font::draw_text(cx + 4, y + 2, PROMPT, SCALE, Color::GREEN);
        if let Ok(s) = core::str::from_utf8(&INPUT[..INPUT_LEN]) {
            let endx = font::draw_text(px, y + 2, s, SCALE, Color::WHITE);
            let t = crate::arch::isr::TICKS / 25;
            if t % 2 == 0 {
                fill_rect(endx, y + 2, CW - 2, CH - 6, Color::TERMINAL_FG);
            }
        } else {
            let _ = px;
        }
    }
}

pub fn welcome() {
    print_line("QwenOS Terminal v0.2 - type 'help' for commands");
    print_line("");
}
