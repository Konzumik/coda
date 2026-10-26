//! 2 — зеркало: полосы над осью и тусклое отражение под ней, как неон
//! в мокром асфальте.

use super::{Scene, UPPER, column, eighths, row_colors};
use crate::color::Lab;
use crate::render::grid::Grid;

/// Доля высоты над осью.
const ABOVE: f32 = 0.62;
/// Отражение короче оригинала…
const REFLECT_LEN: f32 = 0.9;
/// …и тусклее, к низу гаснет ещё сильнее.
const REFLECT_BRIGHT: f32 = 0.5;
const REFLECT_FADE: f32 = 0.6;

pub fn draw(g: &mut Grid, s: &Scene) {
    let up = ((g.h as f32 * ABOVE).round() as usize).clamp(1, g.h);
    let down = g.h - up;
    let colors = row_colors(s.grad, up);
    // отражение k-й строки под осью: цвет той же высоты над осью, приглушённый
    let reflect: Vec<_> = (0..down)
        .map(|k| {
            let lab = s.grad.lab(((k as f32 + 0.5) / up as f32).min(1.0));
            let f = REFLECT_BRIGHT * (1.0 - REFLECT_FADE * k as f32 / down.max(1) as f32);
            Lab { l: lab.l * f, a: lab.a * f, b: lab.b * f }.to_rgb()
        })
        .collect();

    for i in 0..s.bars.min(s.levels.len()) {
        let x0 = s.left + i * (s.bar_w + s.gap);
        let ht = eighths(s.levels[i], up);
        let pk = eighths(s.peaks[i], up).min(up * 8 - 1);
        column(g, x0, s.bar_w, up - 1, up, ht, pk, &colors);

        let len = eighths(s.levels[i] * REFLECT_LEN, down);
        for k in 0..down {
            let base = k * 8;
            let ch = if len >= base + 8 {
                '█'
            } else if len > base {
                UPPER[len - base]
            } else {
                break;
            };
            for dx in 0..s.bar_w {
                g.set(x0 + dx, up + k, ch, reflect[k]);
            }
        }
    }
}
