//! VGA text-mode консоль (0xB8000) — работает на стадии ядра (после exit_boot_services).

use core::ptr::write_volatile;

pub const WIDTH: usize = 80;
pub const HEIGHT: usize = 25;
const VGA_MEM: usize = 0xb_8000;

// Цвета VGA: fg 4 бита, bg 3 бита (+bit 7 мигание).
pub const BLACK: u8 = 0;
pub const BLUE: u8 = 1;
pub const GREEN: u8 = 2;
pub const CYAN: u8 = 3;
pub const RED: u8 = 4;
pub const MAGENTA: u8 = 5;
pub const YELLOW: u8 = 6;
pub const WHITE: u8 = 7;
pub const LIGHT_GRAY: u8 = 8;

static mut COLUMN: usize = 0;
static mut ROW: usize = 0;
/// Текущий атрибут: (fg & 0xf) | (bg & 7) << 4. По умолчанию серый на чёрном.
static mut ATTR: u8 = 0x07;

fn cell_index(x: usize, y: usize) -> usize {
    y * WIDTH + x
}

fn vga_cell(index: usize) -> *mut u16 {
    (VGA_MEM as *mut u16).wrapping_add(index)
}

#[inline]
const fn cell(c: u8, attr: u8) -> u16 {
    ((attr as u16) << 8) | c as u16
}

/// Установить цвет: `fg` (0..15), `bg` (0..7).
pub fn set_color_fg_bg(fg: u8, bg: u8) {
    unsafe {
        ATTR = (fg & 0xf) | ((bg & 7) << 4);
    }
}

/// Установить готовый байт атрибута.
pub fn set_color(attr: u8) {
    unsafe {
        ATTR = attr;
    }
}

pub fn current_attr() -> u8 {
    unsafe { ATTR }
}

/// Поставить символ с текущим цветом в (x, y).
pub fn put_at(x: usize, y: usize, ch: u8, attr: u8) {
    if x < WIDTH && y < HEIGHT {
        unsafe {
            write_volatile(vga_cell(cell_index(x, y)), cell(ch, attr));
        }
    }
}

pub fn get_at(x: usize, y: usize) -> u16 {
    if x < WIDTH && y < HEIGHT {
        unsafe { core::ptr::read_volatile(vga_cell(cell_index(x, y))) }
    } else {
        0
    }
}

fn put_char(b: u8) {
    unsafe {
        match b {
            b'\n' => new_line(),
            0x08 => {
                if COLUMN > 0 {
                    COLUMN -= 1;
                    write_volatile(vga_cell(cell_index(COLUMN, ROW)), cell(b' ', ATTR));
                }
            }
            c => {
                write_volatile(vga_cell(cell_index(COLUMN, ROW)), cell(c, ATTR));
                COLUMN += 1;
                if COLUMN >= WIDTH {
                    new_line();
                }
            }
        }
    }
}

unsafe fn new_line() {
    COLUMN = 0;
    ROW += 1;
    if ROW >= HEIGHT {
        scroll_up();
    }
}

unsafe fn scroll_up() {
    for y in 1..HEIGHT {
        for x in 0..WIDTH {
            let ch = core::ptr::read_volatile(vga_cell(cell_index(x, y)));
            write_volatile(vga_cell(cell_index(x, y - 1)), ch);
        }
    }
    for x in 0..WIDTH {
        write_volatile(vga_cell(cell_index(x, HEIGHT - 1)), cell(b' ', ATTR));
    }
    ROW = HEIGHT - 1;
    COLUMN = 0;
}

pub fn clear() {
    unsafe {
        let blank = cell(b' ', ATTR);
        for i in 0..WIDTH * HEIGHT {
            write_volatile(vga_cell(i), blank);
        }
        ROW = 0;
        COLUMN = 0;
    }
}

/// Курсор в (row, col) для позиции вывода текста.
pub fn move_cursor(row: usize, col: usize) {
    unsafe {
        ROW = row.min(HEIGHT - 1);
        COLUMN = col.min(WIDTH - 1);
    }
}

pub fn cursor_pos() -> (usize, usize) {
    unsafe { (ROW, COLUMN) }
}

pub fn write_str(s: &str) {
    for b in s.as_bytes() {
        put_char(*b);
    }
}

pub fn write_bytes(bytes: &[u8]) {
    for b in bytes {
        put_char(*b);
    }
}

/// Простой fmt::Write, чтобы использовать write! без аллокатора.
pub struct VgaWriter;

impl core::fmt::Write for VgaWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        write_str(s);
        Ok(())
    }
}
