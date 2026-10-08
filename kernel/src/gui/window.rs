//! Менеджер окон: стек Z-порядка, заголовки, кнопки закрыть/свернуть,
//! перетаскивание за окно, клик по содержимому.

use crate::gfx::fb::{fill_rect, rect, Color};
use crate::gfx::font;

pub const TITLE_H: usize = 24;
pub const MAX_WINDOWS: usize = 8;

/// Что умеет окно — задаётся дескриптором приложения.
#[derive(Clone, Copy, PartialEq)]
pub enum App {
    Terminal,
    Info,      // «О системе»
    Files,     // просмотр ramfs
    Logo,      // about/logo
}

#[derive(Clone, Copy)]
pub struct Window {
    pub app: App,
    pub used: bool,
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    pub title: [u8; 32],
    pub titlelen: usize,
}

const EMPTY: Window = Window {
    app: App::Logo,
    used: false,
    x: 0,
    y: 0,
    w: 0,
    h: 0,
    title: [0; 32],
    titlelen: 0,
};

static mut WINS: [Window; MAX_WINDOWS] = [EMPTY; MAX_WINDOWS];

fn set_title(w: &mut Window, t: &str) {
    let b = t.as_bytes();
    let l = b.len().min(32);
    w.title[..l].copy_from_slice(&b[..l]);
    w.titlelen = l;
}

/// Открыть окно; возвращает индекс или None если мест нет.
pub fn open(app: App, x: usize, y: usize, w: usize, h: usize, title: &str) -> Option<usize> {
    unsafe {
        for i in 0..MAX_WINDOWS {
            if !WINS[i].used {
                WINS[i] = EMPTY;
                WINS[i].used = true;
                WINS[i].app = app;
                WINS[i].x = x;
                WINS[i].y = y;
                WINS[i].w = w;
                WINS[i].h = h;
                set_title(&mut WINS[i], title);
                focus_top(i);
                return Some(i);
            }
        }
    }
    None
}

// Z-стек: массив индексов окон, последние — поверх.
static mut TOP_STACK: [usize; MAX_WINDOWS] = [0; MAX_WINDOWS];
static mut STACK_LEN: usize = 0;

fn focus_top(idx: usize) {
    focus(idx);
}

/// Навести фокус (поднять наверх).
pub fn focus(idx: usize) {
    unsafe {
        for i in 0..STACK_LEN {
            if TOP_STACK[i] == idx && WINS[idx].used {
                // remove at i
                for j in i..STACK_LEN - 1 {
                    TOP_STACK[j] = TOP_STACK[j + 1];
                }
                STACK_LEN -= 1;
                break;
            }
        }
        if WINS[idx].used {
            TOP_STACK[STACK_LEN] = idx;
            STACK_LEN += 1;
        }
    }
}

pub fn close(idx: usize) {
    unsafe {
        WINS[idx] = EMPTY;
        focus_remove(idx);
    }
}

fn focus_remove(idx: usize) {
    unsafe {
        let mut i = 0;
        while i < STACK_LEN {
            if TOP_STACK[i] == idx {
                for j in i..STACK_LEN - 1 {
                    TOP_STACK[j] = TOP_STACK[j + 1];
                }
                STACK_LEN -= 1;
            } else {
                i += 1;
            }
        }
    }
}

/// Индекс окна под точкой (x,y), проверяя сверху вниз. Кнопка заголовка:
/// возвращает вместе с областью клика.
pub fn window_at(x: usize, y: usize) -> Option<(usize, Hit)> {
    unsafe {
        let mut i = STACK_LEN;
        while i > 0 {
            i -= 1;
            let idx = TOP_STACK[i];
            let w = &WINS[idx];
            if !w.used {
                continue;
            }
            if x >= w.x && x < w.x + w.w && y >= w.y && y < w.y + w.h {
                let hit = if y < w.y + TITLE_H {
                    // кнопки справа: minimize (—), close (x)
                    if x + 22 >= w.x + w.w && x < w.x + w.w {
                        Hit::CloseBtn
                    } else if x + 44 >= w.x + w.w && x + 22 < w.x + w.w {
                        Hit::MinBtn
                    } else {
                        Hit::Titlebar
                    }
                } else {
                    Hit::Client
                };
                return Some((idx, hit));
            }
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq)]
pub enum Hit {
    Titlebar,
    Client,
    CloseBtn,
    MinBtn,
}

pub fn get(idx: usize) -> Option<Window> {
    unsafe {
        if idx < MAX_WINDOWS && WINS[idx].used {
            Some(WINS[idx])
        } else {
            None
        }
    }
}

pub fn move_to(idx: usize, x: usize, y: usize) {
    unsafe {
        if WINS[idx].used {
            WINS[idx].x = x;
            WINS[idx].y = y;
        }
    }
}

/// Нарисовать рамку и заголовок окна (содержимое рисует приложение).
pub fn draw_frame(idx: usize, active: bool) {
    unsafe {
        let w = &WINS[idx];
        if !w.used {
            return;
        }
        // тень
        fill_rect(w.x + 3, w.y + 3, w.w, w.h, Color::rgb(0, 0, 0));
        // тело
        fill_rect(w.x, w.y, w.w, w.h, Color::WINDOW_BG);
        // заголовок
        let bar = if active { Color::TITLEBAR_ACTIVE } else { Color::TITLEBAR };
        fill_rect(w.x, w.y, w.w, TITLE_H, bar);
        rect(w.x, w.y, w.w, w.h, Color::GRAY);
        // текст заголовка
        if let Ok(t) = core::str::from_utf8(&w.title[..w.titlelen]) {
            font::draw_text(w.x + 8, w.y + 6, t, 2, Color::WHITE);
        }
        // кнопки
        let bx = w.x + w.w;
        fill_rect(bx - 22, w.y + 4, 18, 16, Color::RED);
        font::draw_text(bx - 19, w.y + 7, "x", 2, Color::WHITE);
        fill_rect(bx - 44, w.y + 4, 18, 16, Color::rgb(70, 130, 180));
        font::draw_text(bx - 41, w.y + 10, "-", 2, Color::WHITE);
    }
}

/// Все используемые индексы сверху вниз (для обхода при отрисовке).
pub fn z_order() -> ZIter {
    ZIter { left: unsafe { STACK_LEN } }
}

pub struct ZIter {
    left: usize,
}

impl Iterator for ZIter {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        let idx = unsafe { TOP_STACK[self.left] };
        if unsafe { WINS[idx].used } {
            Some(idx)
        } else {
            self.next()
        }
    }
}

/// Сколько окон открыто.
pub fn count() -> usize {
    z_order().count()
}

/// Самое верхнее (активное) окно или usize::MAX.
pub fn topmost() -> usize {
    unsafe {
        if STACK_LEN == 0 {
            usize::MAX
        } else {
            TOP_STACK[STACK_LEN - 1]
        }
    }
}

/// Обход z-стека сверху вниз (для кнопок панели задач).
pub fn z_order_rev() -> core::iter::Rev<ZIter> {
    z_order().rev()
}

/// «Свёрнутое» окно прячется за правым краем экрана (за пределами hit-test).
const OFFSCREEN_X: usize = usize::MAX / 4;

/// Свернуть окно: увести за правый край экрана (упрощённая минимизация).
pub fn minimize(idx: usize) {
    move_to(idx, OFFSCREEN_X, 50);
}

/// Вернуть фокус окну из панели задач (раскрытие «минимизированного»).
pub fn restore_focus(idx: usize) {
    unsafe {
        if let Some(w) = WINS.get_mut(idx) {
            if w.used && w.x >= OFFSCREEN_X {
                w.x = 80 + (idx * 24) % 200;
                w.y = 60 + (idx * 24) % 120;
            }
        }
    }
    focus(idx);
}
