//! Framebuffer: прямая запись в линейный буфер кадров (получен из UEFI GOP).
//!
//! После `exit_boot_services` указатель на кадровой буфер остаётся валидным
//! (страницы помечены как runtime-память), поэтому ядро рисует напрямую.

use core::ptr::write_volatile;

/// RGB-цвет (24 бита, упакованы в u32 0x00RRGGBB).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color(((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    pub const fn r(self) -> u8 {
        (self.0 >> 16) as u8
    }
    pub const fn g(self) -> u8 {
        (self.0 >> 8) as u8
    }
    pub const fn b(self) -> u8 {
        self.0 as u8
    }

    /// Смешать с другим цветом: `self*(1-a) + other*a`.
    pub const fn blend(self, other: Color, a: u32 /* 0..=256 */) -> Color {
        let inv = 256 - a;
        let r = (self.r() as u32 * inv + other.r() as u32 * a) >> 8;
        let g = (self.g() as u32 * inv + other.g() as u32 * a) >> 8;
        let b = (self.b() as u32 * inv + other.b() as u32 * a) >> 8;
        Color((r << 16) | (g << 8) | b)
    }

    // Палитра интерфейса
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const DESKTOP: Color = Color::rgb(30, 87, 140);
    pub const DESKTOP2: Color = Color::rgb(18, 55, 92);
    pub const ACCENT: Color = Color::rgb(60, 140, 220);
    pub const TITLEBAR: Color = Color::rgb(45, 62, 80);
    pub const TITLEBAR_ACTIVE: Color = Color::rgb(30, 110, 190);
    pub const WINDOW_BG: Color = Color::rgb(240, 242, 245);
    pub const TEXT: Color = Color::rgb(20, 20, 30);
    pub const GREEN: Color = Color::rgb(40, 180, 80);
    pub const RED: Color = Color::rgb(220, 60, 60);
    pub const YELLOW: Color = Color::rgb(240, 200, 60);
    pub const GRAY: Color = Color::rgb(120, 120, 130);
    pub const TERMINAL_BG: Color = Color::rgb(15, 18, 25);
    pub const TERMINAL_FG: Color = Color::rgb(210, 225, 210);
}

/// Статическое состояние framebuffer'а (заполняется один раз при старте ядра).
pub static mut FB: FbInfo = FbInfo::empty();

#[derive(Clone, Copy)]
pub struct FbInfo {
    pub base: *mut u8,
    pub width: usize,
    pub height: usize,
    /// Байт на строку (pixels_per_row * 4).
    pub pitch: usize,
    /// Формат пикселя: true => BGRx (типично для QEMU GOP), false => RGBx.
    pub is_bgr: bool,
}

unsafe impl Send for FbInfo {}

impl FbInfo {
    const fn empty() -> Self {
        FbInfo {
            base: core::ptr::null_mut(),
            width: 0,
            height: 0,
            pitch: 0,
            is_bgr: true,
        }
    }

    #[inline]
    fn pack(&self, c: Color) -> u32 {
        if self.is_bgr {
            (c.b() as u32) << 16 | (c.g() as u32) << 8 | (c.r() as u32)
        } else {
            (c.r() as u32) << 16 | (c.g() as u32) << 8 | (c.b() as u32)
        }
    }
}

/// Установить параметры framebuffer'а (вызывается из stage2 после GOP).
///
/// # Safety
/// `base` должен указывать на корректную область памяти размера height*pitch.
pub unsafe fn init(base: *mut u8, width: usize, height: usize, pitch: usize, is_bgr: bool) {
    FB = FbInfo { base, width, height, pitch, is_bgr };
}

pub fn fb_width() -> usize {
    unsafe { FB.width }
}

pub fn fb_height() -> usize {
    unsafe { FB.height }
}

#[inline]
fn write_px(x: usize, y: usize, packed: u32) {
    unsafe {
        let p = FB.base.add(y * FB.pitch + x * 4) as *mut u32;
        write_volatile(p, packed);
    }
}

/// Поставить пиксель (границы проверяются).
pub fn put_pixel(x: usize, y: usize, c: Color) {
    unsafe {
        if x >= FB.width || y >= FB.height {
            return;
        }
        write_px(x, y, FB.pack(c));
    }
}

/// Залить прямоугольник сплошным цветом.
pub fn fill_rect(x: usize, y: usize, w: usize, h: usize, c: Color) {
    unsafe {
        if FB.base.is_null() {
            return;
        }
        let packed = FB.pack(c);
        let x1 = x.min(FB.width);
        let y1 = y.min(FB.height);
        let x2 = (x + w).min(FB.width);
        let y2 = (y + h).min(FB.height);
        let row = packed | (packed << 32); // два пикселя в u64 — чуть быстрее
        let _ = row;
        for yy in y1..y2 {
            let row_ptr = FB.base.add(yy * FB.pitch + x1 * 4) as *mut u32;
            for xx in x1..x2 {
                write_volatile(row_ptr.add(xx - x1), packed);
            }
        }
    }
}

/// Полупрозрачный прямоугольник (alpha 0..=255).
pub fn blend_rect(x: usize, y: usize, w: usize, h: usize, c: Color, alpha: u8) {
    if alpha == 0 {
        return;
    }
    unsafe {
        if FB.base.is_null() {
            return;
        }
        let a = alpha as u32 * 256 / 255;
        let x1 = x.min(FB.width);
        let y1 = y.min(FB.height);
        let x2 = (x + w).min(FB.width);
        let y2 = (y + h).min(FB.height);
        for yy in y1..y2 {
            for xx in x1..x2 {
                let off = yy * FB.pitch + xx * 4;
                let raw = core::ptr::read_volatile(FB.base.add(off) as *const u32);
                // распаковать из формата обратно в Color
                let cur = if FB.is_bgr {
                    Color::rgb((raw & 0xff) as u8, ((raw >> 8) & 0xff) as u8, ((raw >> 16) & 0xff) as u8)
                } else {
                    Color::rgb(((raw >> 16) & 0xff) as u8, ((raw >> 8) & 0xff) as u8, (raw & 0xff) as u8)
                };
                let mixed = cur.blend(c, a);
                write_volatile(FB.base.add(off) as *mut u32, FB.pack(mixed));
            }
        }
    }
}

/// Рамка прямоугольника толщиной 1px.
pub fn rect(x: usize, y: usize, w: usize, h: usize, c: Color) {
    if w == 0 || h == 0 {
        return;
    }
    hline(x, y, w, c);
    hline(x, y + h - 1, w, c);
    vline(x, y, h, c);
    vline(x + w - 1, y, h, c);
}

pub fn hline(x: usize, y: usize, w: usize, c: Color) {
    fill_rect(x, y, w, 1, c);
}

pub fn vline(x: usize, y: usize, h: usize, c: Color) {
    fill_rect(x, y, 1, h, c);
}

/// Линия по алгоритму Брезенхера.
pub fn line(mut x0: i64, mut y0: i64, x1: i64, y1: i64, c: Color) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if x0 >= 0 && y0 >= 0 {
            put_pixel(x0 as usize, y0 as usize, c);
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

/// Окружность (обводка).
pub fn circle(cx: i64, cy: i64, r: i64, c: Color) {
    let mut x = r;
    let mut y = 0i64;
    let mut err = 1 - r;
    while x >= y {
        let pts = [
            (cx + x, cy + y), (cx - x, cy + y), (cx + x, cy - y), (cx - x, cy - y),
            (cx + y, cy + x), (cx - y, cy + x), (cx + y, cy - x), (cx - y, cy - x),
        ];
        for (px, py) in pts {
            if px >= 0 && py >= 0 {
                put_pixel(px as usize, py as usize, c);
            }
        }
        y += 1;
        if err < 0 {
            err += 2 * y + 1;
        } else {
            x -= 1;
            err += 2 * (y - x) + 1;
        }
    }
}

/// Экран целиком залить цветом.
pub fn clear(c: Color) {
    unsafe {
        if FB.base.is_null() {
            return;
        }
        fill_rect(0, 0, FB.width, FB.height, c);
    }
}

/// Вертикальный градиент фона рабочего стола.
pub fn gradient_bg() {
    unsafe {
        if FB.base.is_null() {
            return;
        }
        let h = FB.height;
        for y in 0..h {
            let t = (y * 256 / h.max(1)) as u32;
            let c = Color::DESKTOP2.blend(Color::DESKTOP, t);
            let packed = FB.pack(c);
            let row_ptr = FB.base.add(y * FB.pitch) as *mut u32;
            for x in 0..FB.width {
                write_volatile(row_ptr.add(x), packed);
            }
        }
    }
}
