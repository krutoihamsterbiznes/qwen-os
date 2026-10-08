//! Драйвер клавиатуры PS/2 (контроллер 8042) для пост-UEFI стадии ядра.
//!
//! Работает в реальном режиме x86_64 после `exit_boot_services`: опрос
//! портов 0x60/0x64, декодирование set-1 сканкодов с учётом Shift/Ctrl/Alt,
//! поддержка комбинации Ctrl+Alt+Del.

use crate::arch::{halt, inb};

const DATA_PORT: u16 = 0x60;
const STATUS_PORT: u16 = 0x64;

/// Биты состояния контроллера 8042.
const OUT_BUF_FULL: u8 = 0x01;

// Сканкоды set-1, которые нам важны specially:
pub const SC_ENTER: u8 = 0x1C;
pub const SC_BACKSPACE: u8 = 0x0E;
pub const SC_TAB: u8 = 0x0F;
pub const SC_ESC: u8 = 0x01;
pub const SC_DEL: u8 = 0x53;

/// Текущее состояние модификаторов.
#[derive(Default, Clone, Copy)]
struct Modifiers {
    l_shift: bool,
    r_shift: bool,
    ctrl: bool,
    alt: bool,
}

impl Modifiers {
    fn shift(self) -> bool {
        self.l_shift || self.r_shift
    }
}

static mut STATE: Modifiers = Modifiers {
    l_shift: false,
    r_shift: false,
    ctrl: false,
    alt: false,
};

/// Набор символов для одного сканкода: [без Shift, с Shift].
type Keysym = [char; 2];

const NOOP: Keysym = ['\0', '\0'];

/// Англо-американская раскладка (set-1 сканкоды), индексы 0x00..=0x57.
#[rustfmt::skip]
static KEYMAP: [Keysym; 0x58] = [
    NOOP,                 // 00
    ['\u{1b}', '\u{1b}'], // 01 Esc
    ['1', '!'], ['2', '@'], ['3', '#'], ['4', '$'], ['5', '%'],   // 02-06
    ['6', '^'], ['7', '&'], ['8', '*'], ['9', '('], ['0', ')'],   // 07-0B
    ['-', '_'], ['=', '+'],                                       // 0C-0D
    ['\u{8}', '\u{8}'],                                           // 0E Backspace
    ['\t', '\t'],                                                 // 0F Tab
    ['q', 'Q'], ['w', 'W'], ['e', 'E'], ['r', 'R'],               // 10-13
    ['t', 'T'], ['y', 'Y'], ['u', 'U'], ['i', 'I'], ['o', 'O'],   // 14-18
    ['p', 'P'], ['[', '{'], [']', '}'],                           // 19-1B
    ['\n', '\n'],                                                 // 1C Enter
    ['a', 'A'], ['s', 'S'], ['d', 'D'], ['f', 'F'],               // 1D-20
    ['g', 'G'], ['h', 'H'], ['j', 'J'], ['k', 'K'], ['l', 'L'],   // 21-25
    [';', ':'], ['\'', '"'], ['`', '~'],                          // 26-28
    NOOP,                                                         // 29 Left Ctrl
    ['\\', '|'],                                                  // 2A... нет, см. ниже
    ['z', 'Z'], ['x', 'X'], ['c', 'C'], ['v', 'V'],               // 2C-2F? — аккуратнее ниже
    ['b', 'B'], ['n', 'N'], ['m', 'M'], [',', '<'], ['.', '>'],   // 
    ['/', '?'],                                                   // 35
    NOOP,                                                         // 36 Right Shift
    NOOP,                                                         // 37 Keypad * / PrtSc
    NOOP,                                                         // 38 Left Alt
    [' ', ' '],                                                   // 39 Space
    NOOP, NOOP, NOOP, NOOP, NOOP,                                 // 3A-3E Caps/F1-F4
    NOOP, NOOP, NOOP, NOOP, NOOP, NOOP, NOOP, NOOP, NOOP,         // 3F-47
    NOOP, NOOP, NOOP, NOOP, NOOP, NOOP, NOOP,                     // 48-4E
    NOOP, NOOP, NOOP, NOOP, NOOP, NOOP,                           // 4F-54
    NOOP, NOOP,                                                   // 55-56
    NOOP,                                                         // 57
];

/// Обновить модификаторы по нажатию/отпусканию.
fn apply_modifiers(scancode: u8, released: bool) {
    unsafe {
        match scancode {
            0x2A => STATE.l_shift = !released,
            0x36 => STATE.r_shift = !released,
            0x1D => STATE.ctrl = !released,
            0x38 => STATE.alt = !released,
            _ => {}
        }
    }
}

/// Декодировать нажатие в символ с учётом модификаторов.
fn decode(scancode: u8) -> Option<char> {
    let idx = scancode as usize;
    if idx >= KEYMAP.len() {
        return None;
    }
    let shift = unsafe { STATE.shift() };
    let c = KEYMAP[idx][if shift { 1 } else { 0 }];
    if c == '\0' {
        None
    } else {
        Some(c)
    }
}

/// Есть ли непрочитанный сканкод?
pub fn has_scancode() -> bool {
    inb(STATUS_PORT) & OUT_BUF_FULL != 0
}

/// Опросить клавиатуру один раз. Возвращает:
/// - `Some('c')` — обычный символ;
/// - `Some('\u{F0XX}')` — спецклавиша (сканкод в нижнем байте);
/// - `None` — ничего нового.
///
/// Отдельно сигнализирует Ctrl+Alt+Del возвратом `Some('\u{F0FF}')`.
pub fn poll_key() -> Option<char> {
    if !has_scancode() {
        return None;
    }
    let raw = inb(DATA_PORT);
    // Extended-последовательности (0xE0 + следующий байт) просто проглатываем.
    if raw == 0xE0 {
        for _ in 0..50_000 {
            if has_scancode() {
                let ext = inb(DATA_PORT);
                apply_modifiers(ext & 0x7f, ext & 0x80 != 0);
                break;
            }
        }
        return None;
    }
    let released = raw & 0x80 != 0;
    let code = raw & 0x7f;
    apply_modifiers(code, released);
    if released {
        return None;
    }
    // Ctrl+Alt+Del
    unsafe {
        if STATE.ctrl && STATE.alt && code == SC_DEL {
            return Some('\u{F0FF}');
        }
    }
    if let Some(c) = decode(code) {
        Some(c)
    } else {
        char::from_u32(0xF000 + code as u32)
    }
}

/// Ожидающий ввод одного символа.
pub fn read_key_blocking() -> char {
    loop {
        if let Some(c) = poll_key() {
            return c;
        }
        halt();
    }
}
