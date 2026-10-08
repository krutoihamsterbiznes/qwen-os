//! Графическая подсистема ядра: framebuffer (GOP), растровый шрифт, 2D-примитивы.

pub mod fb;
pub mod font;

pub use fb::{fb_height, fb_width, fill_rect, blend_rect, line, rect, circle, hline, vline, put_pixel, clear, gradient_bg, Color};
