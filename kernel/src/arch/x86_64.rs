//! Платформенный код x86_64: CPUID, порты ввода-вывода, PIC, PIT, GDT/IDT.

/// Выполнить CPUID (eax=leaf, ecx=0), вернуть (eax, ebx, ecx, edx).
#[inline]
pub fn cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    let (eax, out_ebx, edx): (u32, u32, u32);
    unsafe {
        core::arch::asm!(
            "cpuid",
            inout("eax") leaf => eax,
            inout("ecx") 0 => _,
            lateout("ebx") out_ebx,
            lateout("edx") edx,
        );
    }
    (eax, out_ebx, 0, edx)
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

/// CLI — запрет аппаратных прерываний.
#[inline]
pub fn disable_interrupts() {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
}

/// STI — разрешение аппаратных прерываний.
#[inline]
pub fn enable_interrupts() {
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

// ==================== PIC 8259A (legacy IRQ) ====================

const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD: u16 = 0xa0;
const PIC2_DATA: u16 = 0xa1;
const ICW4: u8 = 0x01;
const INIT: u8 = 0x11;
const EOI: u8 = 0x20;

pub const IRQ_BASE: u8 = 32;

/// Перепрограммировать сдвоенный PIC: IRQ0-7 → векторы 32..39, IRQ8-15 → 40..47.
/// Разрешает IRQ0 (PIT) и IRQ1 (клавиатура PS/2).
pub fn pic_remap() {
    unsafe {
        outb(PIC1_CMD, INIT);
        outb(PIC2_CMD, INIT);
        outb(PIC1_DATA, IRQ_BASE);
        outb(PIC2_DATA, IRQ_BASE + 8);
        outb(PIC1_DATA, 1 << 2); // cascade: slave на IRQ2
        outb(PIC2_DATA, 2);
        outb(PIC1_DATA, ICW4);
        outb(PIC2_DATA, ICW4);
        // Маска: разрешаем только IRQ0 (таймер) и IRQ1 (клавиатура).
        outb(PIC1_DATA, !(1 << 0 | 1 << 1));
        outb(PIC2_DATA, 0xff);
    }
}

/// Скрыть все IRQ (маска 0xff).
pub fn pic_mask_all() {
    outb(PIC1_DATA, 0xff);
    outb(PIC2_DATA, 0xff);
}

/// Подтвердить окончание обработки прерывания (`vector` — номер вектора IDT).
pub fn pic_eoi(vector: u8) {
    if vector >= IRQ_BASE + 8 {
        outb(PIC2_CMD, EOI);
    }
    outb(PIC1_CMD, EOI);
}

// ==================== PIT (таймер 8254) ====================

/// Запустить PIT канал 0 на частоте `hz` Гц.
pub fn pit_start(hz: u32) {
    let hz = hz.max(18);
    let divisor = 1_193_182u32 / hz;
    unsafe {
        outb(0x43, 0x36); // канал 0, lo/hi, rate generator
        outb(0x40, divisor as u8);
        outb(0x40, (divisor >> 8) as u8);
    }
}

/// Динамик PC: гудок `freq_hz` на ~`ms` миллисекунд (грубая busy-wait пауза).
pub fn beep(freq_hz: u32, ms: u32) {
    unsafe {
        let port_a: u16 = 0x61;
        let old = inb(port_a);
        outb(0x43, 0xb6); // канал 2, square wave
        let divisor = 1_193_182u32 / freq_hz.max(20);
        outb(0x42, divisor as u8);
        outb(0x42, (divisor >> 8) as u8);
        outb(port_a, old | 0x3);
        stall_ms(ms);
        outb(port_a, old & !0x3);
    }
}

/// Примитивная busy-wait задержка (порядка ms; точность не гарантируется).
pub fn stall_ms(ms: u32) {
    for _ in 0..ms {
        for _ in 0..30_000 {
            core::hint::spin_loop();
        }
    }
}

// ==================== GDT ====================

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_mid: u8,
    access: u8,
    limit_hi_flags: u8,
    base_hi: u8,
}

const fn gdt_zero() -> GdtEntry {
    GdtEntry {
        limit_low: 0,
        base_low: 0,
        base_mid: 0,
        access: 0,
        limit_hi_flags: 0,
        base_hi: 0,
    }
}

static GDT: [GdtEntry; 3] = [
    gdt_zero(),
    GdtEntry {
        limit_low: 0xffff,
        base_low: 0,
        base_mid: 0,
        access: 0x9a, // code, ring 0
        limit_hi_flags: 0xaf,
        base_hi: 0,
    },
    GdtEntry {
        limit_low: 0xffff,
        base_low: 0,
        base_mid: 0,
        access: 0x92, // data, ring 0
        limit_hi_flags: 0xcf,
        base_hi: 0,
    },
];

#[repr(C, packed)]
struct DescriptorTablePointer {
    limit: u16,
    base: u64,
}

/// Селектор кода ядра для записей IDT.
pub const KERNEL_CODE_SELECTOR: u16 = 0x08;

/// Загрузить простую GDT (null/code/data). Вызывать после exit_boot_services.
/// # Safety
/// Только один раз при инициализации ядра.
pub unsafe fn load_gdt() {
    let gdtr = DescriptorTablePointer {
        limit: (core::mem::size_of::<[GdtEntry; 3]>() - 1) as u16,
        base: core::ptr::addr_of!(GDT) as u64,
    };
    core::arch::asm!(
        "lgdt [{}]",
        in(reg) &gdtr,
        options(readonly, nostack, preserves_flags)
    );
}

// ==================== IDT ====================

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    fn from_fn(handler: unsafe extern "C" fn(), code_sel: u16) -> Self {
        let addr = handler as usize as u64;
        Self {
            offset_low: addr as u16,
            selector: code_sel,
            ist: 0,
            type_attr: 0x8e, // present, ring 0, 64-bit interrupt gate
            offset_mid: (addr >> 16) as u16,
            offset_high: (addr >> 32) as u32,
            reserved: 0,
        }
    }
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::missing(); 256];

// Символ-массив (определён в arch/isr.rs через global_asm!) — адрес таблицы стабов.
#[allow(rustdoc::invalid_html_tags)]
unsafe extern "C" {
    static isr_stub_table: [unsafe extern "C" fn(); 256];
}

/// Устанавливает IDT из таблицы стабов (сгенерированы макросом в isr.rs) и загружает её.
/// # Safety
/// Вызывается ровно один раз при инициализации ядра (после `load_gdt`).
pub unsafe fn install_idt() {
    for i in 0..256 {
        let stub = isr_stub_table[i];
        IDT[i] = IdtEntry::from_fn(stub, KERNEL_CODE_SELECTOR);
    }
    let idtr = DescriptorTablePointer {
        limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
        base: core::ptr::addr_of!(IDT) as u64,
    };
    core::arch::asm!(
        "lidt [{}]",
        in(reg) &idtr,
        options(readonly, nostack, preserves_flags)
    );
}

/// Инициализация архитектуры на ранней стадии ядра.
pub fn init() {
    pic_mask_all();
    disable_interrupts();
}
