//! Сетка клеток: режимы рисуют в неё, а она превращается в ANSI (терминал)
//! или в картинку (examples/snapshot.rs). Пустые клетки не печатаются —
//! курсор их перепрыгивает, поэтому сквозь них виден фон Kitty.

use crate::color::{Lab, Rgb};
use std::fmt::Write;

pub const EMPTY: char = ' ';

#[derive(Clone, Copy, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub fg: Rgb,
    /// None — прозрачный фон терминала.
    pub bg: Option<Rgb>,
}

const BLANK: Cell = Cell { ch: EMPTY, fg: Rgb(0, 0, 0), bg: None };

pub struct Grid {
    pub w: usize,
    pub h: usize,
    cells: Vec<Cell>,
}

impl Grid {
    pub fn new(w: usize, h: usize) -> Self {
        Self { w, h, cells: vec![BLANK; w * h] }
    }

    pub fn resize(&mut self, w: usize, h: usize) {
        if (w, h) != (self.w, self.h) {
            *self = Self::new(w, h);
        }
    }

    pub fn clear(&mut self) {
        self.cells.fill(BLANK);
    }

    pub fn get(&self, x: usize, y: usize) -> Cell {
        self.cells[y * self.w + x]
    }

    pub fn set(&mut self, x: usize, y: usize, ch: char, fg: Rgb) {
        if x < self.w && y < self.h {
            let c = &mut self.cells[y * self.w + x];
            c.ch = ch;
            c.fg = fg;
        }
    }

    pub fn set_bg(&mut self, x: usize, y: usize, bg: Rgb) {
        if x < self.w && y < self.h {
            self.cells[y * self.w + x].bg = Some(bg);
        }
    }

    /// Текст поверх (символы шириной в одну клетку).
    pub fn text(&mut self, x: usize, y: usize, s: &str, fg: Rgb) {
        for (i, ch) in s.chars().enumerate() {
            self.set(x + i, y, ch, fg);
        }
    }

    /// Вывести в терминал начиная со строки `top` (1-based). `bright` < 1
    /// приглушает все цвета (плавная смена режима).
    pub fn write(&self, out: &mut String, top: usize, bright: f32) {
        let dim = |c: Rgb| if bright >= 0.999 { c } else { scale(c, bright) };
        for y in 0..self.h {
            let _ = write!(out, "\x1b[{};1H\x1b[0m\x1b[2K", top + y);
            let (mut fg, mut bg): (Option<Rgb>, Option<Rgb>) = (None, None);
            let mut skip = 0;
            for x in 0..self.w {
                let c = self.get(x, y);
                if c.ch == EMPTY && c.bg.is_none() {
                    skip += 1;
                    continue;
                }
                if skip > 0 {
                    let _ = write!(out, "\x1b[{skip}C");
                    skip = 0;
                }
                let f = dim(c.fg);
                if fg != Some(f) {
                    let _ = write!(out, "\x1b[38;2;{};{};{}m", f.0, f.1, f.2);
                    fg = Some(f);
                }
                let b = c.bg.map(dim);
                if bg != b {
                    match b {
                        Some(b) => {
                            let _ = write!(out, "\x1b[48;2;{};{};{}m", b.0, b.1, b.2);
                        }
                        None => out.push_str("\x1b[49m"),
                    }
                    bg = b;
                }
                out.push(c.ch);
            }
        }
    }
}

/// Яркость и насыщенность × k (в OKLab, оттенок не плывёт).
pub fn scale(c: Rgb, k: f32) -> Rgb {
    let lab = c.to_lab();
    Lab { l: lab.l * k, a: lab.a * k, b: lab.b * k }.to_rgb()
}

/// Брайлевый холст: в каждой клетке 2×4 точки (U+2800–28FF, Kitty рисует
/// их сам). Цвет клетки — цвет последней поставленной в неё точки.
pub struct Braille {
    pub w: usize, // в точках
    pub h: usize,
    cols: usize,
    dots: Vec<u8>,
    color: Vec<Rgb>,
}

impl Braille {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            w: cols * 2,
            h: rows * 4,
            cols,
            dots: vec![0; cols * rows],
            color: vec![Rgb(0, 0, 0); cols * rows],
        }
    }

    pub fn dot(&mut self, x: i32, y: i32, c: Rgb) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        // нумерация точек брайля: 1 2 3 7 в левом столбце, 4 5 6 8 в правом
        const BITS: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];
        let i = (y / 4) * self.cols + x / 2;
        self.dots[i] |= BITS[x % 2][y % 4];
        self.color[i] = c;
    }

    /// Перенести в сетку; клетки без точек не трогаются.
    pub fn blit(&self, grid: &mut Grid) {
        for (i, &d) in self.dots.iter().enumerate() {
            if d != 0 {
                let ch = char::from_u32(0x2800 + d as u32).unwrap();
                grid.set(i % self.cols, i / self.cols, ch, self.color[i]);
            }
        }
    }
}
