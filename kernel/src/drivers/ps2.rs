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

/// Англо-американская раскладка, сканкоды set-1 make-кодов 0x00..=0x57.
#[rustfmt::skip]
static KEYMAP: [Keysym; 0x58] = [
    NONE,                 // 00  Error
    (K_ESC, K_ESC),       // 01  Esc
    ('1', '!'), ('2', '@'), ('3', '#'), ('4', '$'), ('5', '%'),  // 02..06
    ('6', '^'), ('7', '&'), ('8', '*'), ('9', '('), ('0', ')'),  // 07..0B
    ('-', '_'), ('=', '+'),                                      // 0C..0D
    (K_BACKSPACE, K_BACKSPACE),                                 // 0E
    (K_TAB, K_TAB),                                             // 0F
    ('q', 'Q'), ('w', 'W'), ('e', 'E'), ('r', 'R'),              // 10..13
    ('t', 'T'), ('y', 'Y'), ('u', 'U'), ('i', 'I'), ('o', 'O'),  // 14..18
    ('p', 'P'), ('[', '{'], (']', '}'),                          // 19..1B
    (K_ENTER, K_ENTER),                                         // 1C
    ('a', 'A'), ('s', 'S'), ('d', 'D'), ('f', 'F'),              // 1D..20
    ('g', 'G'), ('h', 'H'), ('j', 'J'), ('k', 'K'), ('l', 'L'),  // 21..25
    (';', ':'), ('\'', '"'), ('`', '~'),                        // 26..28
    NONE,                       // 29  Left Ctrl (модификатор)
    ('\\', '|'),                // 2A  Backslash
    ('z', 'Z'), ('x', 'X'), ('c', 'C'), ('v', 'V'),             // 2B..2E? нет: 2B..2E
    ('b', 'B'), ('n', 'N'), ('m', 'M'), (',', '<'), ('.', '>'),  //
    ('/', '?'),                 // 35
    NONE,                       // 36  Right Shift
    NONE,                       // 37  Keypad * / PrtSc
    NONE,                       // 38  Left Alt
    (' ', ' '),                 // 39  Space
    (k_fn(1), k_fn(1)), (k_fn(2), k_fn(2)), (k_fn(3), k_fn(3)), (k_fn(4), k_fn(4)), // 3A..3D? Caps/F1-F4 → считаем F-клавиши 3B..3E
    NONE, NONE, NONE,           // 3E..40? ниже поправим индексы
    // Заполним оставшееся до 0x58 заглушками; точные позиции F-клавиш задаём отдельно.
    NONE, NONE, NONE, NONE, NONE, NONE, NONE,                   //
    NONE, NONE, NONE, NONE, NONE, NONE, NONE,                   //
    NONE, NONE, NONE, NONE, NONE, NONE,                         //
    NONE, NONE,                 //
    NONE,                       // 57
];

// Точные позиции функциональных клавиш (set-1): 0x3B=F1 .. 0x44=F10, 0x57=F11, 0x58=F12.
const FN_CODES: [(u8, char); 12] = [
    (0x3B, k_fn(1)),
    (0x3C, k_fn(2)),
    (0x3D, k_fn(3)),
    (0x3E, k_fn(4)),
    (0x3F, k_fn(5)),
    (0x40, k_fn(6)),
    (0x41, k_fn(7)),
    (0x42, k_fn(8)),
    (0x43, k_fn(9)),
    (0x44, k_fn(10)),
    (0x57, k_fn(11)),
    (0x58, k_fn(12)),
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

        // F1..F12
        for &(c, k) in FN_CODES.iter() {
            if c == code {
                push_key(k);
                return;
            }
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
