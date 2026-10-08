//! Консоль этапа загрузки (boot services): вывод через EFI stdout.

use core::fmt::Write as _;
use uefi::proto::console::text::Output;
use uefi::system::with_stdout;

/// Печать строки в EFI stdout с переводом строки.
pub fn println(text: &str) {
    with_stdout(|out| print_line(out, text));
}

fn print_line(out: &mut Output, text: &str) {
    // write_fmt сам конвертирует UTF-8 в UTF-16 для протокола stdout.
    let _ = out.write_fmt(format_args!("{text}\n"));
}
