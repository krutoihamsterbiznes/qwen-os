//! Платформенный код x86_64: CPUID, порты, остановка ЦП.

/// Выполнить CPUID (eax=in), вернуть (eax, ebx, ecx, edx).
#[inline]
pub fn cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    let (eax, ebx, ecx, edx): (u32, u32, u32, u32);
    unsafe {
        core::arch::asm!(
            "cpuid",
            inout("eax") leaf => eax,
            inout("ebx") 0 => ebx,
            out("ecx") ecx,
            out("edx") edx,
        );
    }
    (eax, ebx, ecx, edx)
}

/// Строка вендора CPU (12 байт из EBX+EDX+ECX) в ASCII-буфер (NUL-терминированный).
pub fn cpuid_vendor(buf: &mut [u8; 13]) {
    let (_eax, ebx, ecx, edx) = cpuid(0);
    let parts = [ebx, edx, ecx];
    for (i, p) in parts.iter().enumerate() {
        buf[i * 4..i * 4 + 4].copy_from_slice(&p.to_le_bytes());
    }
    buf[12] = 0;
}

/// Ввод из порта (x86 I/O).
#[inline]
pub fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        core::arch::asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack));
    }
    value
}

/// Вывод в порт (x86 I/O).
#[inline]
pub fn outb(port: u16, value: u8) {
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack));
    }
}

/// Остановить процессор до ближайшего прерывания (hlt).
#[inline]
pub fn halt() {
    unsafe {
        core::arch::asm!("hlt", options(nomem, nostack));
    }
}

/// Инициализация архитектуры: маскируем прерывания классического PIC.
pub fn init() {
    outb(0x21, 0xff);
    outb(0xa1, 0xff);
    disable_interrupts();
}

/// CLI — запрет аппаратных прерываний.
#[inline]
pub fn disable_interrupts() {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
}
