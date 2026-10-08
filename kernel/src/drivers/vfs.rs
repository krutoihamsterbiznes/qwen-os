//! Виртуальная файловая система в оперативной памяти (ramfs).
//!
//! Файлы создаются ядром при старте (hello.txt, readme, конфиг) и могут
//! создаваться/удаляться через команды терминала. Без аллокатора используем
//! статический пул слотов и статические буферы.

pub const MAX_FILES: usize = 32;
pub const MAX_NAME: usize = 24;
pub const MAX_DATA: usize = 4096;

#[derive(Clone, Copy)]
pub struct File {
    pub used: bool,
    pub name: [u8; MAX_NAME],
    pub namelen: usize,
    pub data: [u8; MAX_DATA],
    pub len: usize,
}

const EMPTY: File = File {
    used: false,
    name: [0; MAX_NAME],
    namelen: 0,
    data: [0; MAX_DATA],
    len: 0,
};

static mut FS: [File; MAX_FILES] = [EMPTY; MAX_FILES];

fn name_bytes(name: &str) -> Option<[u8; MAX_NAME]> {
    let b = name.as_bytes();
    if b.is_empty() || b.len() > MAX_NAME {
        return None;
    }
    let mut arr = [0u8; MAX_NAME];
    arr[..b.len()].copy_from_slice(b);
    Some(arr)
}

/// Создать или перезаписать файл. Возвращает false при переполнении/ошибке.
pub fn write_file(name: &str, content: &[u8]) -> bool {
    let Some(nm) = name_bytes(name) else { return false };
    let nlen = name.len();
    unsafe {
        // существующий?
        for f in FS.iter_mut() {
            if f.used && f.namelen == nlen && &f.name[..nlen] == &nm[..nlen] {
                let l = content.len().min(MAX_DATA);
                f.data[..l].copy_from_slice(&content[..l]);
                f.len = l;
                return true;
            }
        }
        for f in FS.iter_mut() {
            if !f.used {
                f.used = true;
                f.name = nm;
                f.namelen = nlen;
                let l = content.len().min(MAX_DATA);
                f.data[..l].copy_from_slice(&content[..l]);
                f.len = l;
                return true;
            }
        }
    }
    false
}

/// Прочитать файл целиком (копирует в out, возвращает срез).
pub fn read_file<'a>(name: &str, out: &'a mut [u8]) -> Option<&'a [u8]> {
    unsafe {
        for f in FS.iter() {
            if f.used && f.namelen == name.len() && &f.name[..f.namelen] == name.as_bytes() {
                let l = f.len.min(out.len());
                out[..l].copy_from_slice(&f.data[..l]);
                return Some(&out[..l]);
            }
        }
    }
    None
}

/// Размер файла, если существует.
pub fn file_size(name: &str) -> Option<usize> {
    unsafe {
        for f in FS.iter() {
            if f.used && f.namelen == name.len() && &f.name[..f.namelen] == name.as_bytes() {
                return Some(f.len);
            }
        }
    }
    None
}

pub fn delete_file(name: &str) -> bool {
    unsafe {
        for f in FS.iter_mut() {
            if f.used && f.namelen == name.len() && &f.name[..f.namelen] == name.as_bytes() {
                *f = EMPTY;
                return true;
            }
        }
    }
    false
}

/// Список имён существующих файлов (до `max`), возвращает количество.
pub fn list<'a>(out: &'a mut [&'a str], max: usize) -> usize {
    let mut n = 0;
    unsafe {
        for f in FS.iter() {
            if f.used && n < max {
                if let Ok(s) = core::str::from_utf8(&f.name[..f.namelen]) {
                    // продлеваем жизнь строки статически: имена лежат в FS
                    let s: &'static str = core::mem::transmute::<&str, &'static str>(s);
                    out[n] = s;
                    n += 1;
                }
            }
        }
    }
    n
}

/// Инициализация: набить диск начальными файлами.
pub fn init() {
    write_file(
        "readme.txt",
        b"QwenOS v0.2 - experimental hobby OS.\nUEFI hybrid kernel, x86_64, written in Rust.\nBoot stage: UEFI application (loader) -> ExitBootServices -> GUI kernel.\n",
    );
    write_file("hello.txt", b"Hello from QwenOS kernel RAM-FS!\n");
    write_file(
        "about.txt",
        b"Features: PS/2 keyboard, PS/2 mouse, GOP framebuffer GUI,\ndesktop with windows, taskbar, terminal with commands.\nType 'help' in the terminal for a command list.\n",
    );
    write_file("config.ini", b"[system]\nhostname=qwen-os\ntheme=blue\nautostart=terminal\n");
    write_file("/etc/motd", b"Welcome to QwenOS. Have fun!\n");
}
