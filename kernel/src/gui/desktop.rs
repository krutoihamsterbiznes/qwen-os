//! Рабочий стол: фон, ярлыки, панель задач с часами, окна, курсор мыши,
//! обработка ввода (клавиатура PS/2 + мышь PS/2) и цикл событий.

use crate::arch;
use crate::arch::isr;
use crate::drivers::mouse::{self, MouseState};
use crate::drivers::ps2;
use crate::drivers::vfs;
use crate::gfx::fb::{self, blend_rect, circle, fill_rect, line, put_pixel, rect, Color};
use crate::gfx::font;
use crate::gui::term;
use crate::gui::window::{self, App, Hit};

pub const TASKBAR_H: usize = 30;

/// Ярлыки на рабочем столе.
struct Icon {
    label: &'static str,
    app: App,
}

const ICONS: [Icon; 4] = [
    Icon { label: "Terminal", app: App::Terminal },
    Icon { label: "System Info", app: App::Info },
    Icon { label: "Files", app: App::Files },
    Icon { label: "About", app: App::Logo },
];

const ICON_X: usize = 24;
const ICON_Y0: usize = 28;
const ICON_STEP: usize = 74;

/// Идёт ли перетаскивание окна; смещение курсора относительно левого верха окна.
static mut DRAG_IDX: usize = usize::MAX;
static mut DRAG_DX: i32 = 0;
static mut DRAG_DY: i32 = 0;

static mut PREV_LEFT: bool = false;

fn screen_w() -> usize { fb::fb_width() }
fn screen_h() -> usize { fb::fb_height() }

/// Открыть приложение по имени строки ("info" | "files" | "logo").
fn open_named(name: &str) {
    let w = screen_w();
    match name {
        "info" => {
            window::open(App::Info, w / 2 - 180, 90, 380, 260, "System Info");
        }
        "files" => {
            window::open(App::Files, 120, 120, 420, 300, "RAM-FS Files");
        }
        "logo" | "about" => {
            window::open(App::Logo, w / 2 - 150, 120, 320, 200, "About QwenOS");
        }
        _ => {}
    }
}

/// Стартовая раскладка окон.
fn spawn_default_windows() {
    let w = screen_w();
    let h = screen_h();
    // Терминал — главное окно.
    let tw = 560.min(w - 80);
    let th = (h - TASKBAR_H - 90).min(420);
    if let Some(idx) = window::open(App::Terminal, 60, 50, tw, th, "QwenOS Terminal") {
        window::focus(idx);
    }
    term::welcome();
    open_named("info");
}

// ---------- отрисовка ----------

fn draw_icons() {
    for (i, ic) in ICONS.iter().enumerate() {
        let y = ICON_Y0 + i * ICON_STEP;
        // пиктограмма
        match ic.app {
            App::Terminal => {
                fill_rect(ICON_X, y, 40, 30, Color::TERMINAL_BG);
                rect(ICON_X, y, 40, 30, Color::WHITE);
                font::draw_text(ICON_X + 5, y + 6, ">_", 2, Color::GREEN);
            }
            App::Info => {
                fill_rect(ICON_X, y, 40, 30, Color::rgb(230, 240, 255));
                rect(ICON_X, y, 40, 30, Color::ACCENT);
                font::draw_text(ICON_X + 16, y + 8, "i", 2, Color::ACCENT);
            }
            App::Files => {
                fill_rect(ICON_X, y + 6, 40, 24, Color::YELLOW);
                fill_rect(ICON_X, y, 20, 8, Color::YELLOW);
                rect(ICON_X, y, 40, 30, Color::GRAY);
            }
            App::Logo => {
                circle((ICON_X + 20) as i64, (y + 15) as i64, 14, Color::WHITE);
                circle((ICON_X + 20) as i64, (y + 15) as i64, 9, Color::ACCENT);
            }
        }
        font::draw_text(ICON_X - 2, y + 36, ic.label, 1, Color::WHITE);
    }
}

fn draw_taskbar() {
    let w = screen_w();
    let h = screen_h();
    let ty = h - TASKBAR_H;
    fill_rect(0, ty, w, TASKBAR_H, Color::rgb(22, 30, 40));
    blend_rect(0, ty, w, 2, Color::ACCENT, 200);

    // кнопка «Пуск»
    fill_rect(4, ty + 4, 64, TASKBAR_H - 8, Color::TITLEBAR_ACTIVE);
    rect(4, ty + 4, 64, TASKBAR_H - 8, Color::WHITE);
    font::draw_text(12, ty + 9, "Start", 1, Color::WHITE);

    // кнопки открытых окон
    let mut bx = 76usize;
    for idx in window::z_order_rev() {
        if let Some(win) = window::get(idx) {
            let label = core::str::from_utf8(&win.title[..win.titlelen]).unwrap_or("win");
            let short = if label.len() > 14 { &label[..14] } else { label };
            let bw = (short.chars().count() * 6 + 16).min(140);
            let active = idx == window::topmost();
            fill_rect(bx, ty + 4, bw, TASKBAR_H - 8, if active { Color::ACCENT } else { Color::TITLEBAR });
            rect(bx, ty + 4, bw, TASKBAR_H - 8, Color::GRAY);
            font::draw_text(bx + 8, ty + 9, short, 1, Color::WHITE);
            bx += bw + 6;
            if bx > w - 120 {
                break;
            }
        }
    }

    // часы справа
    let ticks = unsafe { isr::TICKS };
    let total_s = ticks / 100;
    let hh = (total_s / 3600) % 24;
    let mm = (total_s / 60) % 60;
    let ss = total_s % 60;
    let mut sb = [0u8; 32];
    use core::fmt::Write;
    {
        let mut buf = FmtBuf(&mut sb, 0);
        let _ = write!(buf, "{:02}:{:02}:{:02}", hh, mm, ss);
    }
    let len = unsafe { *(core::ptr::addr_of!(sb).cast::<u8>()) } ; // not used
    let _ = len;
    let time_str = core::str::from_utf8(&sb[..8]).unwrap_or("00:00:00");
    font::draw_text(w - 90, ty + 9, time_str, 2, Color::WHITE);
}

struct FmtBuf<'a>(&'a mut [u8], usize);
impl core::fmt::Write for FmtBuf<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let l = s.as_bytes().len().min(self.0.len() - self.1);
        self.0[self.1..self.1 + l].copy_from_slice(&s.as_bytes()[..l]);
        self.1 += l;
        Ok(())
    }
}

fn draw_window_content(idx: usize) {
    let Some(win) = window::get(idx) else { return };
    let cx = win.x + 2;
    let cy = win.y + window::TITLE_H + 2;
    let cw = win.w - 4;
    let ch = win.h - window::TITLE_H - 4;
    match win.app {
        App::Terminal => term::render(cx, cy, cw, ch),
        App::Info => render_info(cx, cy, cw, ch),
        App::Files => render_files(cx, cy, cw, ch),
        App::Logo => render_logo(cx, cy, cw, ch),
    }
}

fn render_info(cx: usize, cy: usize, cw: usize, _ch: usize) {
    fill_rect(cx, cy, cw, _ch, Color::WINDOW_BG);
    let mut y = cy + 8;
    for line in crate::SYS_INFO_LINES {
        font::draw_text(cx + 8, y, line, 1, Color::TEXT);
        y += 14;
    }
    let ticks = unsafe { isr::TICKS };
    let mut b = [0u8; 32];
    {
        let mut buf = FmtBuf(&mut b, 0);
        use core::fmt::Write;
        let _ = write!(buf, "timer ticks: {}", ticks);
    }
    font::draw_text(cx + 8, y, core::str::from_utf8(&b).unwrap_or(""), 1, Color::TEXT);
}

fn render_files(cx: usize, cy: usize, cw: usize, ch: usize) {
    fill_rect(cx, cy, cw, ch, Color::WINDOW_BG);
    let mut names: [&str; 32] = [""; 32];
    let n = vfs::list(&mut names, 32);
    let mut y = cy + 6;
    font::draw_text(cx + 8, y, "Name                     Size", 1, Color::TITLEBAR);
    y += 12;
    line(cx + 6, y, cx + cw - 6, y, Color::GRAY);
    y += 4;
    for fname in names.iter().take(n) {
        if y + 12 > cy + ch {
            break;
        }
        let size = vfs::file_size(fname).unwrap_or(0);
        let mut b = [0u8; 64];
        {
            let mut buf = FmtBuf(&mut b, 0);
            use core::fmt::Write;
            let _ = write!(buf, "{:<24} {:>6} B", fname, size);
        }
        font::draw_text(cx + 8, y, core::str::from_utf8(&b).unwrap_or(*fname), 1, Color::TEXT);
        y += 12;
    }
    if n == 0 {
        font::draw_text(cx + 8, y, "(empty)", 1, Color::GRAY);
    }
}

fn render_logo(cx: usize, cy: usize, cw: usize, ch: usize) {
    fill_rect(cx, cy, cw, ch, Color::DESKTOP);
    let ccx = (cx + cw / 2) as i64;
    let ccy = (cy + ch / 2 - 14) as i64;
    circle(ccx, ccy, 34, Color::WHITE);
    circle(ccx, ccy, 26, Color::ACCENT);
    circle(ccx, ccy, 12, Color::WHITE);
    font::draw_text_centered(cx, cy + ch - 44, cw, "QwenOS 0.2", 2, Color::WHITE);
    font::draw_text_centered(cx, cy + ch - 22, cw, "Rust x86_64 UEFI kernel", 1, Color::rgb(200, 215, 235));
}

fn draw_cursor(ms: MouseState) {
    let (x, y) = (ms.x as usize, ms.y as usize);
    // стрелка
    for dy in 0..12usize {
        for dx in 0..(12 - dy).max(1) {
            put_pixel(x + dx, y + dy, if dx + dy < 6 { Color::BLACK } else { Color::WHITE });
        }
    }
    // обводка
    line(x as i64, y as i64, (x + 8) as i64, (y + 8) as i64, Color::BLACK);
    line((x + 8) as i64, (y + 8) as i64, (x + 4) as i64, (y + 11) as i64, Color::BLACK);
}

/// Полный перерисовка кадра.
pub fn redraw() {
    fb::gradient_bg();
    draw_icons();
    let top = window::topmost();
    for idx in window::z_order() {
        window::draw_frame(idx, idx == top);
        draw_window_content(idx);
    }
    draw_taskbar();
    let ms = mouse::state();
    draw_cursor(ms);
}

/// Демо-графика для команды `paint` (рисуется поверх рабочего стола до ближайшего redraw).
pub fn paint_demo() {
    let w = screen_w();
    let h = screen_h();
    for i in 0..60 {
        let x = (i * 7) % w;
        let y = (i * 11) % (h - TASKBAR_H);
        let c = Color::rgb((i * 4) as u8, (255 - i * 4) as u8, 128);
        circle(x as i64, y as i64, (i % 20) as i64 + 4, c);
    }
}

// ---------- события ----------

fn launch_icon(app: App) {
    match app {
        App::Terminal => {
            // если терминал уже есть — фокус, иначе создать
            if let Some(idx) = find_term() {
                window::focus(idx);
            } else {
                let w = screen_w();
                let h = screen_h();
                window::open(App::Terminal, 60, 50, 560.min(w - 80), (h - 140).min(420), "QwenOS Terminal");
            }
        }
        App::Info => open_named("info"),
        App::Files => open_named("files"),
        App::Logo => open_named("logo"),
    }
}

fn find_term() -> Option<usize> {
    for idx in window::z_order() {
        if let Some(w) = window::get(idx) {
            if w.app == App::Terminal {
                return Some(idx);
            }
        }
    }
    None
}

fn icon_at(x: usize, y: usize) -> Option<App> {
    for (i, ic) in ICONS.iter().enumerate() {
        let iy = ICON_Y0 + i * ICON_STEP;
        if x >= ICON_X && x < ICON_X + 44 && y >= iy && y < iy + 52 {
            return Some(ic.app);
        }
    }
    None
}

fn taskbar_start_btn(x: usize, y: usize) -> bool {
    let ty = screen_h() - TASKBAR_H;
    x >= 4 && x < 68 && y >= ty + 4 && y < ty + TASKBAR_H - 4
}

fn taskbar_window_btn(x: usize, y: usize) -> Option<usize> {
    let ty = screen_h() - TASKBAR_H;
    if y < ty + 4 || y >= ty + TASKBAR_H - 4 {
        return None;
    }
    let mut bx = 76usize;
    for idx in window::z_order_rev() {
        if let Some(win) = window::get(idx) {
            let label = core::str::from_utf8(&win.title[..win.titlelen]).unwrap_or("win");
            let short = if label.len() > 14 { &label[..14] } else { label };
            let bw = (short.chars().count() * 6 + 16).min(140);
            if x >= bx && x < bx + bw {
                return Some(idx);
            }
            bx += bw + 6;
        }
    }
    None
}

/// Обработать движение/клики мыши.
fn handle_mouse() {
    let ms = mouse::poll_state();
    let x = ms.x.max(0) as usize;
    let y = ms.y.max(0) as usize;

    // перетаскивание окна
    unsafe {
        if DRAG_IDX != usize::MAX {
            if ms.left {
                let nx = (ms.x - DRAG_DX).max(0) as usize;
                let ny = (ms.y - DRAG_DY).max(0) as usize;
                window::move_to(DRAG_IDX, nx, ny.min(screen_h().saturating_sub(window::TITLE_H + 40)));
            } else {
                DRAG_IDX = usize::MAX;
            }
            return;
        }
    }

    let pressed_edge = ms.left && !unsafe { PREV_LEFT };
    unsafe { PREV_LEFT = ms.left }
    let right_edge = mouse::right_click_edge();

    if pressed_edge {
        // клик: сначала проверим панель задач и ярлыки (они под окнами по z, но
        // кликаются только если не попали в окно)
        if let Some((idx, hit)) = window::window_at(x, y) {
            window::focus(idx);
            match hit {
                Hit::CloseBtn => window::close(idx),
                Hit::MinBtn => {
                    // minimize = увести за левый край (упрощённо: закрыть нельзя — прячем)
                    window::minimize(idx);
                }
                Hit::Titlebar => {
                    if let Some(win) = window::get(idx) {
                        unsafe {
                            DRAG_IDX = idx;
                            DRAG_DX = ms.x - win.x as i32;
                            DRAG_DY = ms.y - win.y as i32;
                        }
                    }
                }
                Hit::Client => {
                    // двойной клик по клиенту терминала ничего не делает; focus уже выполнен
                }
            }
        } else if taskbar_start_btn(x, y) {
            // «Пуск»: открыть самое верхнее приложение или терминал
            launch_icon(App::Terminal);
        } else if let Some(idx) = taskbar_window_btn(x, y) {
            window::restore_focus(idx);
        } else if let Some(app) = icon_at(x, y) {
            launch_icon(app);
        }
    }

    if right_edge {
        // правый клик по рабочему столу — контекстное меню (упрощённо: beep + info)
        if window::window_at(x, y).is_none() {
            arch::beep(660, 40);
            if context_menu_open() {
                close_context_menu();
            } else {
                open_context_menu(x, y);
            }
        } else {
            close_context_menu();
        }
    }
}

// Контекстное меню рабочего стола: маленькое всплывающее окно-список.
static mut CTX_OPEN: bool = false;
static mut CTX_X: usize = 0;
static mut CTX_Y: usize = 0;

fn context_menu_open() -> bool { unsafe { CTX_OPEN } }
fn close_context_menu() { unsafe { CTX_OPEN = false } }
fn open_context_menu(x: usize, y: usize) {
    unsafe {
        CTX_OPEN = true;
        CTX_X = x.min(screen_w() - 150);
        CTX_Y = y.min(screen_h() - TASKBAR_H - 90);
    }
}

fn draw_context_menu() {
    if !context_menu_open() {
        return;
    }
    let (x, y) = unsafe { (CTX_X, CTX_Y) };
    fill_rect(x, y, 150, 84, Color::WINDOW_BG);
    rect(x, y, 150, 84, Color::GRAY);
    font::draw_text(x + 10, y + 6, "New terminal", 1, Color::TEXT);
    font::draw_text(x + 10, y + 26, "Open files", 1, Color::TEXT);
    font::draw_text(x + 10, y + 46, "Paint demo", 1, Color::TEXT);
    font::draw_text(x + 10, y + 66, "About", 1, Color::TEXT);
}

fn ctx_item_at(x: usize, y: usize) -> Option<usize> {
    if !context_menu_open() {
        return None;
    }
    let (mx, my) = unsafe { (CTX_X, CTX_Y) };
    if x >= mx && x < mx + 150 && y >= my && y < my + 84 {
        Some(((y - my) / 20).min(3))
    } else {
        None
    }
}

/// Обработать клавишу из буфера клавиатуры (доставляет в активный терминал).
fn handle_keys() {
    while let Some(k) = ps2::poll_key() {
        match k {
            '\r' | '\n' => {
                term::submit(&mut |name| open_named(name));
            }
            0x08 => term::input_backspace(),
            ps2::K_UP => term::input_history(-1),
            ps2::K_DOWN => term::input_history(1),
            c if (c as u32) >= 32 => term::input_char(c),
            _ => {}
        }
    }
}

/// Главный цикл GUI. Вызывается из stage2 после инициализации всего остального.
pub fn run() -> ! {
    spawn_default_windows();
    arch::pic_remap_enable_timer_kbd_mouse();
    arch::pit_start(100);
    arch::enable_interrupts();

    loop {
        handle_keys();
        handle_mouse_with_ctx();
        redraw();
        draw_context_menu();
        // ~30 FPS при 100 Гц таймере: ждём 3 тика
        let t0 = unsafe { isr::TICKS };
        while unsafe { isr::TICKS } < t0 + 3 {
            arch::halt();
        }
    }
}

fn handle_mouse_with_ctx() {
    // сначала special: клик по пунктам контекстного меню
    let ms = mouse::poll_state();
    let x = ms.x.max(0) as usize;
    let y = ms.y.max(0) as usize;
    let pressed_edge = ms.left && !unsafe { PREV_LEFT };
    if pressed_edge {
        if let Some(item) = ctx_item_at(x, y) {
            close_context_menu();
            match item {
                0 => launch_icon(App::Terminal),
                1 => launch_icon(App::Files),
                2 => paint_demo(),
                _ => launch_icon(App::Logo),
            }
            unsafe { PREV_LEFT = ms.left }
            return;
        } else if context_menu_open() {
            close_context_menu();
        }
    }
    handle_mouse();
}
