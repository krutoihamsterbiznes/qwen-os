//! Консоль ядра после выхода из boot services: VGA text-mode (0xB8000).

use core::ptr::write_volatile;

const VGA_WIDTH: usize = 80;
const VGA_HEIGHT: usize = 50;
const VGA_MEM: usize = 0xb_8000;

/// Чёрный текст на светло-сером фоне.
const ATTR: u8 = 0x07;

static mut COLUMN: usize = 0;
static mut ROW: usize = 0;

fn vga_cell(index: usize) -> *mut u16 {
    (VGA_MEM as *mut u16).wrapping_add(index)
}

fn put_char(b: u8) {
    unsafe {
        match b {
            b'\n' => new_line(),
            0x08 => {
                if COLUMN > 0 {
                    COLUMN -= 1;
                    write_volatile(vga_cell(ROW * VGA_WIDTH + COLUMN), blank());
                }
            }
            c => {
                write_volatile(vga_cell(ROW * VGA_WIDTH + COLUMN), cell(c));
                COLUMN += 1;
                if COLUMN >= VGA_WIDTH {
                    new_line();
                }
            }
        }
    }
}

const fn cell(c: u8) -> u16 {
    (ATTR as u16) << 8 | c as u16
}

const fn blank() -> u16 {
    cell(b' ')
}

unsafe fn new_line() {
    COLUMN = 0;
    ROW += 1;
    if ROW >= VGA_HEIGHT {
        scroll_up();
    }
}

unsafe fn scroll_up() {
    for y in 1..VGA_HEIGHT {
        for x in 0..VGA_WIDTH {
            let ch = core::ptr::read_volatile(vga_cell(y * VGA_WIDTH + x));
            write_volatile(vga_cell((y - 1) * VGA_WIDTH + x), ch);
        }
    }
    for x in 0..VGA_WIDTH {
        write_volatile(vga_cell((VGA_HEIGHT - 1) * VGA_WIDTH + x), blank());
    }
    ROW = VGA_HEIGHT - 1;
    COLUMN = 0;
}

pub fn clear() {
    unsafe {
        for i in 0..VGA_WIDTH * VGA_HEIGHT {
            write_volatile(vga_cell(i), blank());
        }
        ROW = 0;
        COLUMN = 0;
    }
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
