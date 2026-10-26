//! Сборка кадра в одну строку ANSI. Кадр обёрнут в synchronized output
//! (режим 2026, в Kitty это PENDING_UPDATE): терминал показывает его целиком.
//! Блоки и брайль Kitty рисует сам, без шрифта (font_for_cell → BOX_FONT).

pub mod grid;
pub mod modes;

use crate::color::{Gradient, Rgb};
use grid::Grid;
use modes::Scene;
use std::fmt::Write;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const MAX_BARS: usize = 110;
const GAP: usize = 1;
const DIM: Rgb = Rgb::hex(0x6B6B85);
/// Ниже этой высоты окна шапку с обложкой не показываем: полосам нужнее.
const HUD_MIN_ROWS: usize = 18;

/// Где стоит обложка, в клетках (row, col — 1-based).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CoverBox {
    pub row: usize,
    pub col: usize,
    pub cols: usize,
    pub rows: usize,
}

pub struct Layout {
    pub cols: usize,
    pub rows: usize,
    pub bars: usize,
    bar_w: usize,
    left: usize,
    /// Первая строка области режима (1-based).
    pub top: usize,
    pub area_h: usize,
    hud_h: usize,
    /// Обложка в шапке; None — окно слишком низкое, шапки нет.
    pub cover: Option<CoverBox>,
    pub cell_aspect: f32,
}

impl Layout {
    /// `cell_aspect` — ширина клетки / высота клетки в пикселях.
    pub fn new(cols: u16, rows: u16, cell_aspect: f32) -> Self {
        let (cols, rows) = (cols as usize, rows as usize);
        let mut bar_w = if cols < 64 { 1 } else { 2 };
        while (cols + GAP) / (bar_w + GAP) > MAX_BARS {
            bar_w += 1;
        }
        let bars = ((cols + GAP) / (bar_w + GAP)).max(1);
        let used = bars * bar_w + (bars - 1) * GAP;

        let hud_h = if rows >= HUD_MIN_ROWS { (rows / 5).clamp(5, 11) } else { 0 };
        let cover = (hud_h > 0).then(|| {
            let r = hud_h - 2; // по строке отступа сверху и снизу
            // квадрат: клетки высокие, поэтому столбцов больше, чем строк
            CoverBox { row: 2, col: 3, rows: r, cols: (r as f32 / cell_aspect).round() as usize }
        });
        Self {
            cols,
            rows,
            bars,
            bar_w,
            left: cols.saturating_sub(used) / 2,
            top: hud_h + 1,
            area_h: rows.saturating_sub(hud_h + 1).max(1), // последняя строка под статус
            hud_h,
            cover,
            cell_aspect,
        }
    }

    pub fn scene<'a>(
        &self,
        levels: &'a [f32],
        peaks: &'a [f32],
        grad: &'a Gradient,
        dt: f32,
    ) -> Scene<'a> {
        Scene {
            levels,
            peaks,
            grad,
            dt,
            cell_aspect: self.cell_aspect,
            bars: self.bars,
            bar_w: self.bar_w,
            gap: GAP,
            left: self.left,
        }
    }

    /// Где стоит обложка в режиме `kind`.
    pub fn cover_for(&self, kind: modes::Kind) -> Option<CoverBox> {
        match kind {
            modes::Kind::Ring => modes::ring::cover_box(self.top, self.area_h, self.cols, self.cell_aspect),
            _ => self.cover,
        }
    }
}

pub struct Hud<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
    /// Обложка сейчас на экране (текст встаёт справа от неё).
    pub cover_shown: bool,
    pub title_c: Rgb,
    pub artist_c: Rgb,
}

pub struct Status {
    pub live: bool,
    pub fps: f32,
    pub ceiling_db: f32,
}

/// Помнит текущий цвет, чтобы не слать одинаковые escape-коды.
#[derive(Default)]
struct Pen(Option<Rgb>);

impl Pen {
    fn fg(&mut self, out: &mut String, c: Rgb) {
        if self.0 != Some(c) {
            let _ = write!(out, "\x1b[38;2;{};{};{}m", c.0, c.1, c.2);
            self.0 = Some(c);
        }
    }
}

/// `gfx` — команды графики Kitty; идут после очистки экрана, т.к. CSI 2J
/// удаляет картинки. `body` — кадр режима, `bright` — его яркость.
#[allow(clippy::too_many_arguments)]
pub fn frame(
    out: &mut String,
    lay: &Layout,
    body: &Grid,
    bright: f32,
    clear: bool,
    gfx: &str,
    hud: &Hud,
    st: &Status,
) {
    out.clear();
    out.push_str("\x1b[?2026h");
    if clear {
        out.push_str("\x1b[0m\x1b[2J");
    }
    out.push_str(gfx);
    if lay.hud_h > 0 {
        hud_lines(out, lay, hud);
    }
    body.write(out, lay.top, bright);
    if lay.rows >= 2 {
        status_line(out, lay, st);
    }
    out.push_str("\x1b[0m\x1b[?2026l");
}

/// Шапка: название, исполнитель, альбом справа от обложки.
/// Стирание строки картинки не трогает (по спеке протокола Kitty).
fn hud_lines(out: &mut String, lay: &Layout, hud: &Hud) {
    for row in 1..=lay.hud_h {
        let _ = write!(out, "\x1b[{row};1H\x1b[0m\x1b[2K");
    }
    let Some(cover) = lay.cover else { return };
    let col = if hud.cover_shown { cover.col + cover.cols + 3 } else { cover.col };
    let width = lay.cols.saturating_sub(col + 1);
    if width < 4 {
        return;
    }
    let first = cover.row + cover.rows.saturating_sub(3) / 2;

    let mut pen = Pen::default();
    let _ = write!(out, "\x1b[{first};{col}H");
    if hud.title.is_empty() {
        pen.fg(out, DIM);
        out.push_str(&fit("ожидание трека…", width));
        return;
    }
    pen.fg(out, hud.title_c);
    let _ = write!(out, "\x1b[1m{}\x1b[22m", fit(&hud.title.to_uppercase(), width));
    let _ = write!(out, "\x1b[{};{col}H", first + 1);
    pen.fg(out, hud.artist_c);
    out.push_str(&fit(hud.artist, width));
    if !hud.album.is_empty() && cover.rows >= 3 {
        let _ = write!(out, "\x1b[{};{col}H", first + 2);
        pen.fg(out, DIM);
        out.push_str(&fit(hud.album, width));
    }
}

/// Обрезать по ширине в клетках (иероглифы и эмодзи шириной 2) с «…».
fn fit(s: &str, width: usize) -> String {
    if s.width() <= width {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw + 1 > width {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

fn status_line(out: &mut String, lay: &Layout, st: &Status) {
    let _ = write!(out, "\x1b[{};1H\x1b[0m\x1b[2K", lay.rows);
    let (dot, state) = if st.live { ('●', "звук") } else { ('○', "тишина") };
    let left = format!(
        " {dot} {state}   {:>3.0} fps   agc {:+5.1} dB   {} полос",
        st.fps, st.ceiling_db, lay.bars
    );
    let right = "q — выход ";
    let (lw, rw) = (left.chars().count(), right.chars().count());

    let mut pen = Pen::default();
    pen.fg(out, DIM);
    if lw + rw < lay.cols {
        out.push_str(&left);
        out.extend(std::iter::repeat_n(' ', lay.cols - lw - rw));
        out.push_str(right);
    } else {
        out.extend(left.chars().take(lay.cols));
    }
}
