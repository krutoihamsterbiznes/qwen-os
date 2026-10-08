//! Таблица прерываний: ассемблерные стабы + диспетчер на Rust.
//!
//! Схема: вектор → стаб (`push vector; jmp isr_common`) → `isr_common`
//! сохраняет все GPR, передаёт указатель на фрейм в `isr_rust_handler`,
//! восстанавливает регистры и делает `iretq`.

use crate::arch;

/// Регистры, сохранённые `isr_common` на стеке (порядок push/pop совпадает
/// с порядком полей структуры).
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Frame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub vector: u64,
    // аппаратный фрейм (IRET):
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
}

// ---------- общий код обработки ----------

core::arch::global_asm!(
    ".text\n",
    ".code64\n",
    "isr_common:\n",
    "    push rax\n    push rbx\n    push rcx\n    push rdx\n",
    "    push rsi\n    push rdi\n    push rbp\n",
    "    push r8\n    push r9\n    push r10\n    push r11\n",
    "    push r12\n    push r13\n    push r14\n    push r15\n",
    "    mov rdi, rsp\n",
    "    call isr_rust_handler\n",
    "    pop r15\n    pop r14\n    pop r13\n    pop r12\n",
    "    pop r11\n    pop r10\n    pop r9\n    pop r8\n",
    "    pop rbp\n    pop rdi\n    pop rsi\n    pop rdx\n",
    "    pop rcx\n    pop rbx\n    pop rax\n",
    "    add rsp, 8\n", // убрать номер вектора
    "    iretq\n",
);

// ---------- стабы для всех 256 векторов ----------
// Все векторы пушат свой номер и прыгают в isr_common. Исключения с кодом
// ошибки (8, 10-14, 17, 21, 29, 30) получают его от CPU автоматически —
// он лежит под номером вектора, но нам для IRQ-стабов это не важно.

macro_rules! define_isr_stub {
    ($name:ident, $vector:expr) => {
        core::arch::global_asm!(concat!(
            ".global ", stringify!($name), "\n",
            ".align 16\n",
            stringify!($name), ":\n",
            "    push ", stringify!($vector), "\n",
            "    jmp isr_common\n",
        ));
    };
}

macro_rules! define_isr_stubs {
    ($($n:expr),+ $(,)?) => {
        $(
            paste::paste! {
                define_isr_stub!([<isr_stub_ $n>], $n);
            }
        )+
    };
}

define_isr_stubs!(
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
    16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
    32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47,
    48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63,
    64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79,
    80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95,
    96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111,
    112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127,
    128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143,
    144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159,
    160, 161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 174, 175,
    176, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191,
    192, 193, 194, 195, 196, 197, 198, 199, 200, 201, 202, 203, 204, 205, 206, 207,
    208, 209, 210, 211, 212, 213, 214, 215, 216, 217, 218, 219, 220, 221, 222, 223,
    224, 225, 226, 227, 228, 229, 230, 231, 232, 233, 234, 235, 236, 237, 238, 239,
    240, 241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253, 254, 255,
);

/// Таблица указателей на стабы — читается из `arch::install_idt`.
core::arch::global_asm!(
    ".section .rodata\n",
    ".align 8\n",
    ".global isr_stub_table\n",
    "isr_stub_table:\n",
    ".quad isr_stub_0,   isr_stub_1,   isr_stub_2,   isr_stub_3,   isr_stub_4,   isr_stub_5,   isr_stub_6,   isr_stub_7\n",
    ".quad isr_stub_8,   isr_stub_9,   isr_stub_10,  isr_stub_11,  isr_stub_12,  isr_stub_13,  isr_stub_14,  isr_stub_15\n",
    ".quad isr_stub_16,  isr_stub_17,  isr_stub_18,  isr_stub_19,  isr_stub_20,  isr_stub_21,  isr_stub_22,  isr_stub_23\n",
    ".quad isr_stub_24,  isr_stub_25,  isr_stub_26,  isr_stub_27,  isr_stub_28,  isr_stub_29,  isr_stub_30,  isr_stub_31\n",
    ".quad isr_stub_32,  isr_stub_33,  isr_stub_34,  isr_stub_35,  isr_stub_36,  isr_stub_37,  isr_stub_38,  isr_stub_39\n",
    ".quad isr_stub_40,  isr_stub_41,  isr_stub_42,  isr_stub_43,  isr_stub_44,  isr_stub_45,  isr_stub_46,  isr_stub_47\n",
    ".quad isr_stub_48,  isr_stub_49,  isr_stub_50,  isr_stub_51,  isr_stub_52,  isr_stub_53,  isr_stub_54,  isr_stub_55\n",
    ".quad isr_stub_56,  isr_stub_57,  isr_stub_58,  isr_stub_59,  isr_stub_60,  isr_stub_61,  isr_stub_62,  isr_stub_63\n",
    ".quad isr_stub_64,  isr_stub_65,  isr_stub_66,  isr_stub_67,  isr_stub_68,  isr_stub_69,  isr_stub_70,  isr_stub_71\n",
    ".quad isr_stub_72,  isr_stub_73,  isr_stub_74,  isr_stub_75,  isr_stub_76,  isr_stub_77,  isr_stub_78,  isr_stub_79\n",
    ".quad isr_stub_80,  isr_stub_81,  isr_stub_82,  isr_stub_83,  isr_stub_84,  isr_stub_85,  isr_stub_86,  isr_stub_87\n",
    ".quad isr_stub_88,  isr_stub_89,  isr_stub_90,  isr_stub_91,  isr_stub_92,  isr_stub_93,  isr_stub_94,  isr_stub_95\n",
    ".quad isr_stub_96,  isr_stub_97,  isr_stub_98,  isr_stub_99,  isr_stub_100, isr_stub_101, isr_stub_102, isr_stub_103\n",
    ".quad isr_stub_104, isr_stub_105, isr_stub_106, isr_stub_107, isr_stub_108, isr_stub_109, isr_stub_110, isr_stub_111\n",
    ".quad isr_stub_112, isr_stub_113, isr_stub_114, isr_stub_115, isr_stub_116, isr_stub_117, isr_stub_118, isr_stub_119\n",
    ".quad isr_stub_120, isr_stub_121, isr_stub_122, isr_stub_123, isr_stub_124, isr_stub_125, isr_stub_126, isr_stub_127\n",
    ".quad isr_stub_128, isr_stub_129, isr_stub_130, isr_stub_131, isr_stub_132, isr_stub_133, isr_stub_134, isr_stub_135\n",
    ".quad isr_stub_136, isr_stub_137, isr_stub_138, isr_stub_139, isr_stub_140, isr_stub_141, isr_stub_142, isr_stub_143\n",
    ".quad isr_stub_144, isr_stub_145, isr_stub_146, isr_stub_147, isr_stub_148, isr_stub_149, isr_stub_150, isr_stub_151\n",
    ".quad isr_stub_152, isr_stub_153, isr_stub_154, isr_stub_155, isr_stub_156, isr_stub_157, isr_stub_158, isr_stub_159\n",
    ".quad isr_stub_160, isr_stub_161, isr_stub_162, isr_stub_163, isr_stub_164, isr_stub_165, isr_stub_166, isr_stub_167\n",
    ".quad isr_stub_168, isr_stub_169, isr_stub_170, isr_stub_171, isr_stub_172, isr_stub_173, isr_stub_174, isr_stub_175\n",
    ".quad isr_stub_176, isr_stub_177, isr_stub_178, isr_stub_179, isr_stub_180, isr_stub_181, isr_stub_182, isr_stub_183\n",
    ".quad isr_stub_184, isr_stub_185, isr_stub_186, isr_stub_187, isr_stub_188, isr_stub_189, isr_stub_190, isr_stub_191\n",
    ".quad isr_stub_192, isr_stub_193, isr_stub_194, isr_stub_195, isr_stub_196, isr_stub_197, isr_stub_198, isr_stub_199\n",
    ".quad isr_stub_200, isr_stub_201, isr_stub_202, isr_stub_203, isr_stub_204, isr_stub_205, isr_stub_206, isr_stub_207\n",
    ".quad isr_stub_208, isr_stub_209, isr_stub_210, isr_stub_211, isr_stub_212, isr_stub_213, isr_stub_214, isr_stub_215\n",
    ".quad isr_stub_216, isr_stub_217, isr_stub_218, isr_stub_219, isr_stub_220, isr_stub_221, isr_stub_222, isr_stub_223\n",
    ".quad isr_stub_224, isr_stub_225, isr_stub_226, isr_stub_227, isr_stub_228, isr_stub_229, isr_stub_230, isr_stub_231\n",
    ".quad isr_stub_232, isr_stub_233, isr_stub_234, isr_stub_235, isr_stub_236, isr_stub_237, isr_stub_238, isr_stub_239\n",
    ".quad isr_stub_240, isr_stub_241, isr_stub_242, isr_stub_243, isr_stub_244, isr_stub_245, isr_stub_246, isr_stub_247\n",
    ".quad isr_stub_248, isr_stub_249, isr_stub_250, isr_stub_251, isr_stub_252, isr_stub_253, isr_stub_254, isr_stub_255\n",
);

// ---------- обработчик на Rust ----------

/// Счётчик тиков таймера PIT (100 Гц → 10 тиков = 100 мс).
pub static mut TICKS: u64 = 0;

const EXCEPTION_NAMES: [&str; 32] = [
    "Division Error", "Debug", "Non-Maskable Interrupt", "Breakpoint",
    "Overflow", "BOUND Range Exceeded", "Invalid Opcode", "Device Not Available",
    "Double Fault", "Coprocessor Segment Overrun", "Invalid TSS", "Segment Not Present",
    "Stack-Segment Fault", "General Protection Fault", "Page Fault", "Reserved",
    "x87 FPU Floating-Point Error", "Alignment Check", "Machine Check",
    "SIMD Floating-Point Exception", "Virtualization Exception",
    "Control Protection Exception", "Reserved", "Reserved", "Reserved", "Reserved",
    "Reserved", "Reserved", "Hypervisor Injection", "VMM Communication",
    "Security Exception", "Reserved",
];

/// Единая точка входа для всех прерываний/исключений.
#[no_mangle]
extern "C" fn isr_rust_handler(frame: &Frame) {
    let vector = frame.vector as u8;
    match vector {
        // IRQ0 → PIT (таймер)
        arch::IRQ_BASE => {
            unsafe { TICKS += 1 };
            arch::pic_eoi(arch::IRQ_BASE);
        }
        // IRQ1 → клавиатура PS/2
        v if v == arch::IRQ_BASE + 1 => {
            crate::drivers::ps2::irq_handler();
            arch::pic_eoi(v);
        }
        // IRQ12 → мышь PS/2 (aux)
        v if v == arch::IRQ_BASE + 12 => {
            crate::drivers::mouse::irq_handler();
            arch::pic_eoi(v);
        }
        // Исключения ЦП
        0..=31 => exception_panic(vector, frame),
        // Прочие IRQ — просто заглушаем EOI'ем
        _ => arch::pic_eoi(vector),
    }
}

fn exception_panic(vector: u8, frame: &Frame) {
    use crate::vga;
    vga::set_color(0x1f); // белый на синем — «экран паники»
    vga::clear();
    vga::write_str("QwenOS KERNEL PANIC\n\n");
    vga::write_str("Exception: ");
    vga::write_str(EXCEPTION_NAMES[vector as usize % 32]);
    vga::write_str("\nVector:  ");
    let mut buf = [0u8; 8];
    vga::write_str(itoa(vector as u64, &mut buf));
    vga::write_str("\nRIP:     ");
    let mut b2 = [0u8; 20];
    vga::write_str(hex64(frame.rip, &mut b2));
    vga::write_str("\n\nSystem halted. Press Ctrl+Alt+Del or reset.\n");
    loop {
        arch::disable_interrupts();
        arch::halt();
    }
}

/// Число в десятичную строку (без аллокатора). Возвращает &str внутри buf.
pub fn itoa(value: u64, buf: &mut [u8]) -> &str {
    let mut i = buf.len();
    let mut v = value;
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    core::str::from_utf8(&buf[i..]).unwrap_or("0")
}

/// Число в hex-строку "0x...".
pub fn hex64(value: u64, buf: &mut [u8]) -> &str {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut i = buf.len() - 1;
    let mut v = value;
    loop {
        buf[i] = DIGITS[(v & 0xf) as usize];
        i -= 1;
        v >>= 4;
        if v == 0 {
            break;
        }
    }
    buf[i] = b'x';
    if i == 0 {
        return core::str::from_utf8(&buf[..]).unwrap_or("0x0");
    }
    buf[i - 1] = b'0';
    core::str::from_utf8(&buf[i - 1..=buf.len() - 1])
        .unwrap_or("0x0")
        .trim_end_matches('\0')
}
