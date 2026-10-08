//! Драйвер мыши PS/2 (aux-порт 8042).
//!
//! Инициализация: включить aux-порт, включить IRQ12, перевести мышь в режим
//! с пакетом из 3 байт. Дальше `poll_packet()` читает пакеты протокола PS/2
//! и обновляет глобальное состояние курсора (x, y, кнопки).

use crate::arch::{inb, outb};

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;
const OUT_BUF_FULL: u8 = 0x01;
const IN_BUF_FULL: u8 = 0x02;

fn wait_read() -> Option<u8> {
    for _ in 0..100_000 {
        if inb(STATUS_PORT) & OUT_BUF_FULL != 0 {
            return Some(inb(DATA_PORT));
        }
    }
    None
}

fn wait_write_ready() -> bool {
    for _ in 0..100_000 {
        if inb(STATUS_PORT) & IN_BUF_FULL == 0 {
            return true;
        }
    }
    false
}

/// Отправить команду aux-устройству (через 0xD4).
fn aux_write(byte: u8) -> bool {
    if !wait_write_ready() {
        return false;
    }
    outb(STATUS_PORT, 0xD4);
    if !wait_write_ready() {
        return false;
    }
    outb(DATA_PORT, byte);
    // Ждём ACK (0xFA)
    wait_read() == Some(0xFA)
}

/// Отправить команду контроллеру 8042.
fn cmd_write(byte: u8) {
    if wait_write_ready() {
        outb(STATUS_PORT, byte);
    }
}

#[derive(Default, Clone, Copy)]
pub struct MouseState {
    pub x: i32,
    pub y: i32,
    pub left: bool,
    pub right: bool,
    pub middle: bool,
    pub wheel: i8,
}

static mut STATE: MouseState = MouseState {
    x: 0,
    y: 0,
    left: false,
    right: false,
    middle: false,
    wheel: 0,
};

static mut PACKET: [u8; 8] = [0; 8];
static mut PACKET_LEN: usize = 0;
static mut USES_WHEEL: bool = false;

/// Инициализировать мышь. Возвращает false, если мыши нет (таймауты).
pub fn init() -> bool {
    unsafe {
        // Включить aux-порт и прерывания на нём (команды контроллера).
        cmd_write(0xA8); // enable aux device
        cmd_write(0x20); // read command byte
        let status = match wait_read() {
            Some(s) => s,
            None => return false,
        };
        cmd_write(0x60); // write command byte
        if !wait_write_ready() {
            return false;
        }
        outb(DATA_PORT, status | 0x02 | 0x10); // IRQ12 + aux enabled

        // Конфигурация самой мыши.
        if !aux_write(0xF5) {
            // включение потока данных (некоторые эмуляторы отвечают иначе)
            aux_write(0xF4);
        }
        aux_write(0xF4); // enable reporting
        aux_write(0xF6); // defaults
        aux_write(0xE8); // set sample rate
        if wait_write_ready() {
            outb(DATA_PORT, 100);
            let _ = wait_read(); // ACK
        }
        // Попытка включить Intellimouse (4-байтные пакеты с колесом).
        if aux_write(0xE8) && wait_write_ready() {
            outb(DATA_PORT, 200);
            let _ = wait_read();
            if aux_write(0xE8) && wait_write_ready() {
                outb(DATA_PORT, 100);
                let _ = wait_read();
                if aux_write(0xE8) && wait_write_ready() {
                    outb(DATA_PORT, 80);
                    let _ = wait_read();
                    if aux_write(0xF3) && wait_write_ready() {
                        outb(DATA_PORT, 200); // sample rate для wheel-режима
                        let _ = wait_read();
                        USES_WHEEL = true;
                    }
                }
            }
        }
        aux_write(0xF4); // снова включить отчёт
        PACKET_LEN = 0;
        true
    }
}

/// Прочитать доступные байты с порта и разобрать пакеты.
pub fn poll() {
    while inb(STATUS_PORT) & OUT_BUF_FULL != 0 {
        let b = inb(DATA_PORT);
        unsafe {
            push_byte(b);
        }
    }
}

unsafe fn push_byte(b: u8) {
    // Первый байт пакета имеет бит 5 (0x08 — sign-x? нет: bit3=1, bits4-5 — X/Y sign).
    // Стандартный признак: (b & 0x08) != 0 и (b & 0xC0) == 0 для 3-байтного.
    let is_header = if USES_WHEEL {
        (b & 0xC8) == 0x08 || (b & 0xF8) == 0x08
    } else {
        (b & 0xC8) == 0x08
    };
    if is_header || PACKET_LEN == 0 {
        PACKET_LEN = 0;
    }
    if PACKET_LEN < PACKET.len() {
        PACKET[PACKET_LEN] = b;
        PACKET_LEN += 1;
    }
    let want = if USES_WHEEL { 4 } else { 3 };
    if PACKET_LEN >= want {
        apply_packet();
        PACKET_LEN = 0;
    }
}

unsafe fn apply_packet() {
    let want = if USES_WHEEL { 4 } else { 3 };
    if PACKET_LEN < want {
        return;
    }
    let flags = PACKET[0];
    if flags & 0x08 == 0 {
        return; // повреждённый пакет
    }
    let mut dx = PACKET[1] as i32;
    let mut dy = -(PACKET[2] as i32);
    if flags & 0x10 != 0 {
        dx -= 256;
    }
    if flags & 0x20 != 0 {
        dy += 256;
    }
    STATE.x = (STATE.x + dx).clamp(0, screen_width() as i32 - 1);
    STATE.y = (STATE.y + dy).clamp(0, screen_height() as i32 - 1);
    STATE.left = flags & 0x01 != 0;
    STATE.right = flags & 0x02 != 0;
    STATE.middle = flags & 0x04 != 0;
    if USES_WHEEL && PACKET_LEN >= 4 {
        STATE.wheel = PACKET[3] as i8;
    }
}

/// Текущее состояние мыши (снимок).
pub fn state() -> MouseState {
    poll();
    unsafe { STATE }
}

/// Нажатие левой кнопки «как событие»: возвращает true один раз после нажатия.
static mut LAST_LEFT: bool = false;
pub fn left_click_edge() -> bool {
    let s = state();
    let pressed = s.left && !unsafe { LAST_LEFT };
    unsafe { LAST_LEFT = s.left };
    pressed
}

fn screen_width() -> u32 {
    crate::gfx::framebuffer().width
}

fn screen_height() -> u32 {
    crate::gfx::framebuffer().height
}
