//! 1 — классические полосы с пиками.

use super::{Scene, column, eighths, row_colors};
use crate::render::grid::Grid;

pub fn draw(g: &mut Grid, s: &Scene) {
    let h = g.h;
    let colors = row_colors(s.grad, h);
    for i in 0..s.bars.min(s.levels.len()) {
        let x0 = s.left + i * (s.bar_w + s.gap);
        let pk = eighths(s.peaks[i], h).min(h * 8 - 1);
        column(g, x0, s.bar_w, h - 1, h, eighths(s.levels[i], h), pk, &colors);
    }
}
