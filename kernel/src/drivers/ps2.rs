//! Драйвер клавиатуры PS/2 (контроллер 8042) для стадии ядра.
//!
//! Прерывание IRQ1 вызывает `irq_handler`, который читает сканкод и
//! кладёт декодированный символ в кольцевой буфер. Читающая сторона —
//! `pop_key()` (неблокирующий) или `wait_key()` (hlt-цикл).

use crate::arch::{halt, inb};

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;
const OUT_BUF_FULL: u8 = 0x01;

/// Спецсимволы (управляющие коды), которые pop_key возвращает для нетекстовых клавиш.
pub const K_UP: char = '\u{F001}';
pub const K_DOWN: char = '\u{F002}';
pub const K_RIGHT: char = '\u{F003}';
pub const K_LEFT: char = '\u{F004}';
pub const K_HOME: char = '\u{F005}';
pub const K_END: char = '\u{F006}';
pub const K_DELETE: char = '\u{F007}';
pub const K_PGUP: char = '\u{F008}';
pub const K_PGDN: char = '\u{F009}';
pub const K_ESC: char = '\u{001B}';
pub const K_TAB: char = '\t';
pub const K_ENTER: char = '\n';
pub const K_BACKSPACE: char = '\u{0008}';
pub const K_CTRLALTDEL: char = '\u{F0FF}';
/// F1..F12
pub const fn k_fn(n: u8) -> char {
    match n {
        1 => '\u{F101}',
        2 => '\u{F102}',
        3 => '\u{F103}',
        4 => '\u{F104}',
        5 => '\u{F105}',
        6 => '\u{F106}',
        7 => '\u{F107}',
        8 => '\u{F108}',
        9 => '\u{F109}',
        10 => '\u{F10A}',
        11 => '\u{F10B}',
        _ => '\u{F10C}',
    }
}

#[derive(Default, Clone, Copy)]
struct Modifiers {
    shift: bool,
    ctrl: bool,
    alt: bool,
    caps: bool,
}

static mut MODS: Modifiers = Modifiers {
    shift: false,
    ctrl: false,
    alt: false,
    caps: false,
};

/// Кольцевой буфер символов.
const BUF_SIZE: usize = 128;
static mut KEYBUF: [char; BUF_SIZE] = ['\0'; BUF_SIZE];
static mut KB_HEAD: usize = 0;
static mut KB_TAIL: usize = 0;

/// Предыдущий байт для разбора extended-последовательностей (0xE0 ...).
static mut LAST_RAW: u8 = 0;

type Keysym = (char, char); // (обычный, с Shift)

const NONE: Keysym = ('\0', '\0');

/// Англо-американская раскладка, сканкоды set-1 make-кодов 0x00..=0x58.
/// Точные позиции: 0x1C = Enter, 0x1D = Left Ctrl, 0x2A = Left Shift,
/// 0x36 = Right Shift, 0x38 = Left Alt, 0x3A = CapsLock,
/// 0x3B..0x44 = F1..F10, 0x57 = F11, 0x58 = F12.
#[rustfmt::skip]
static KEYMAP: [Keysym; 0x59] = [
    /* 00 */ NONE,
    /* 01 */ (K_ESC, K_ESC),
    /* 02 */ ('1', '!'),
    /* 03 */ ('2', '@'),
    /* 04 */ ('3', '#'),
    /* 05 */ ('4', '$'),
    /* 06 */ ('5', '%'),
    /* 07 */ ('6', '^'),
    /* 08 */ ('7', '&'),
    /* 09 */ ('8', '*'),
    /* 0A */ ('9', '('),
    /* 0B */ ('0', ')'),
    /* 0C */ ('-', '_'),
    /* 0D */ ('=', '+'),
    /* 0E */ (K_BACKSPACE, K_BACKSPACE),
    /* 0F */ (K_TAB, K_TAB),
    /* 10 */ ('q', 'Q'),
    /* 11 */ ('w', 'W'),
    /* 12 */ ('e', 'E'),
    /* 13 */ ('r', 'R'),
    /* 14 */ ('t', 'T'),
    /* 15 */ ('y', 'Y'),
    /* 16 */ ('u', 'U'),
    /* 17 */ ('i', 'I'),
    /* 18 */ ('o', 'O'),
    /* 19 */ ('p', 'P'),
    /* 1A */ ('[', '{'),
    /* 1B */ (']', '}'),
    /* 1C */ (K_ENTER, K_ENTER),
    /* 1D */ NONE, // Left Ctrl — модификатор
    /* 1E */ ('a', 'A'),
    /* 1F */ ('s', 'S'),
    /* 20 */ ('d', 'D'),
    /* 21 */ ('f', 'F'),
    /* 22 */ ('g', 'G'),
    /* 23 */ ('h', 'H'),
    /* 24 */ ('j', 'J'),
    /* 25 */ ('k', 'K'),
    /* 26 */ ('l', 'L'),
    /* 27 */ (';', ':'),
    /* 28 */ ('\'', '"'),
    /* 29 */ ('`', '~'),
    /* 2A */ NONE, // Left Shift — модификатор
    /* 2B */ ('\\', '|'),
    /* 2C */ ('z', 'Z'),
    /* 2D */ ('x', 'X'),
    /* 2E */ ('c', 'C'),
    /* 2F */ ('v', 'V'),
    /* 30 */ ('b', 'B'),
    /* 31 */ ('n', 'N'),
    /* 32 */ ('m', 'M'),
    /* 33 */ (',', '<'),
    /* 34 */ ('.', '>'),
    /* 35 */ ('/', '?'),
    /* 36 */ NONE, // Right Shift — модификатор
    /* 37 */ NONE, // Keypad *
    /* 38 */ NONE, // Left Alt — модификатор
    /* 39 */ (' ', ' '),
    /* 3A */ NONE, // CapsLock — обрабатывается отдельно
    /* 3B */ (k_fn(1), k_fn(1)),
    /* 3C */ (k_fn(2), k_fn(2)),
    /* 3D */ (k_fn(3), k_fn(3)),
    /* 3E */ (k_fn(4), k_fn(4)),
    /* 3F */ (k_fn(5), k_fn(5)),
    /* 40 */ (k_fn(6), k_fn(6)),
    /* 41 */ (k_fn(7), k_fn(7)),
    /* 42 */ (k_fn(8), k_fn(8)),
    /* 43 */ (k_fn(9), k_fn(9)),
    /* 44 */ (k_fn(10), k_fn(10)),
    /* 45 */ NONE, // NumLock
    /* 46 */ NONE, // ScrollLock
    /* 47 */ NONE, /* 48 */ NONE, /* 49 */ NONE, /* 4A */ NONE,
    /* 4B */ NONE, /* 4C */ NONE, /* 4D */ NONE, /* 4E */ NONE,
    /* 4F */ NONE, /* 50 */ NONE, /* 51 */ NONE, /* 52 */ NONE,
    /* 53 */ NONE, /* 54 */ NONE, /* 55 */ NONE, /* 56 */ NONE,
    /* 57 */ (k_fn(11), k_fn(11)),
    /* 58 */ (k_fn(12), k_fn(12)),
];

// Стрелки и навигация приходят как extended (0xE0 prefix).
const EXT_CODES: [(u8, char); 9] = [
    (0x48, K_UP),
    (0x50, K_DOWN),
    (0x4D, K_RIGHT),
    (0x4B, K_LEFT),
    (0x47, K_HOME),
    (0x4F, K_END),
    (0x49, K_PGUP),
    (0x51, K_PGDN),
    (0x53, K_DELETE),
];

fn push_key(c: char) {
    unsafe {
        let next = (KB_HEAD + 1) % BUF_SIZE;
        if next != KB_TAIL {
            KEYBUF[KB_HEAD] = c;
            KB_HEAD = next;
        }
    }
}

/// Обработать один raw-байт со скан-кодами (общая логика для прерывания и polling).
fn handle_raw(raw: u8) {
    unsafe {
        // Extended-набор: после 0xE0 следующий байт — special.
        if raw == 0xE0 {
            LAST_RAW = 0xE0;
            return;
        }
        if LAST_RAW == 0xE0 {
            LAST_RAW = 0;
            let code = raw & 0x7f;
            if raw & 0x80 == 0 {
                for &(c, k) in EXT_CODES.iter() {
                    if c == code {
                        push_key(k);
                        return;
                    }
                }
                // Right Ctrl (0x1D ext), Right Alt (0x38 ext) — игнорируем как break
            }
            return;
        }

        let released = raw & 0x80 != 0;
        let code = raw & 0x7f;

        // Модификаторы
        match code {
            0x2A | 0x36 => {
                MODS.shift = !released;
                return;
            }
            0x1D => {
                MODS.ctrl = !released;
                return;
            }
            0x38 => {
                MODS.alt = !released;
                return;
            }
            0x3A => {
                if !released {
                    MODS.caps = !MODS.caps;
                }
                return;
            }
            _ => {}
        }

        if released {
            return;
        }

        // Ctrl+Alt+Del
        if MODS.ctrl && MODS.alt && code == 0x53 {
            push_key(K_CTRLALTDEL);
            return;
        }

        // Обычные клавиши
        if (code as usize) < KEYMAP.len() {
            let (normal, shifted) = KEYMAP[code as usize];
            if normal == '\0' {
                return;
            }
            let mut ch = if MODS.shift { shifted } else { normal };
            // CapsLock влияет только на буквы
            if MODS.caps && ch.is_ascii_alphabetic() {
                ch = if ch.is_lowercase() {
                    ch.to_ascii_uppercase()
                } else {
                    ch.to_ascii_lowercase()
                };
            }
            // Ctrl+буква → управляющие коды (^A=1 ... ^Z=26)
            if MODS.ctrl && ch.is_ascii_alphabetic() {
                ch = (ch.to_ascii_lowercase() as u8 - b'a' + 1) as char;
            }
            push_key(ch);
        }
    }
}

/// Человекочитаемое имя спец-клавиши (для UI: «нажми F1» и т.п.).
pub fn fn_label(c: char) -> Option<&'static str> {
    let n = match c {
        '\u{F101}' => 1,
        '\u{F102}' => 2,
        '\u{F103}' => 3,
        '\u{F104}' => 4,
        '\u{F105}' => 5,
        '\u{F106}' => 6,
        '\u{F107}' => 7,
        '\u{F108}' => 8,
        '\u{F109}' => 9,
        '\u{F10A}' => 10,
        '\u{F10B}' => 11,
        '\u{F10C}' => 12,
        _ => return None,
    };
    Some(match n {
        1 => "F1", 2 => "F2", 3 => "F3", 4 => "F4", 5 => "F5", 6 => "F6",
        7 => "F7", 8 => "F8", 9 => "F9", 10 => "F10", 11 => "F11", _ => "F12",
    })
}

/// Вызывается из обработчика прерывания IRQ1.
pub fn irq_handler() {
    if inb(STATUS_PORT) & OUT_BUF_FULL != 0 {
        let raw = inb(DATA_PORT);
        handle_raw(raw);
    }
}

/// Есть ли непрочитанный символ?
pub fn has_key() -> bool {
    unsafe { KB_HEAD != KB_TAIL || inb(STATUS_PORT) & OUT_BUF_FULL != 0 }
}

/// Опросить железо (для режима без прерываний) и вернуть следующий символ.
pub fn poll_key() -> Option<char> {
    // Сначала подтянем свежие байты из контроллера.
    while inb(STATUS_PORT) & OUT_BUF_FULL != 0 {
        let raw = inb(DATA_PORT);
        handle_raw(raw);
    }
    unsafe {
        if KB_HEAD == KB_TAIL {
            None
        } else {
            let c = KEYBUF[KB_TAIL];
            KB_TAIL = (KB_TAIL + 1) % BUF_SIZE;
            Some(c)
        }
    }
}

/// Блокирующее чтение одного символа.
pub fn wait_key() -> char {
    loop {
        if let Some(c) = poll_key() {
            return c;
        }
        unsafe { halt() };
    }
}
