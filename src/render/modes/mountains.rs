//! 4 — горы: спектр сплошным силуэтом с гладким контуром (каждый столбец
//! экрана свой, высота с точностью 1/8 клетки), светлая кромка по гребню.
//! Позади — тёмная дальняя гряда из пиков: она держится и медленно оседает.

use super::{LOWER, Scene, WHITE, eighths, sample};
use crate::color::Lab;
use crate::render::grid::Grid;

const RIM: f32 = 0.3;
const BACK: f32 = 0.36;

pub fn draw(g: &mut Grid, s: &Scene) {
    let h = g.h;
    let rows: Vec<_> = (0..h)
        .map(|c| {
            let lab = s.grad.lab((c as f32 + 0.5) / h as f32);
            let back = Lab { l: lab.l * BACK, a: lab.a * BACK, b: lab.b * BACK };
            (lab.to_rgb(), lab.mix(WHITE, RIM).to_rgb(), back.to_rgb())
        })
        .collect();

    for x in 0..g.w {
        let u = (x as f32 + 0.5) / g.w as f32;
        let v = sample(s.levels, u);
        let ht = eighths(v, h);
        let pk = eighths(sample(s.peaks, u).max(v), h);
        for cell in 0..h {
            let base = cell * 8;
            let y = h - 1 - cell;
            let (front, rim, back) = rows[cell];
            if ht >= base + 8 {
                // верхняя целая клетка гребня — тоже кромка
                g.set(x, y, '█', if ht < base + 16 { rim } else { front });
            } else if ht > base {
                g.set(x, y, LOWER[ht - base], rim);
                if pk >= base + 8 {
                    g.set_bg(x, y, back); // дальняя гряда видна над гребнем
                }
            } else if pk >= base + 8 {
                g.set(x, y, '█', back);
            } else if pk > base {
                g.set(x, y, LOWER[pk - base], back);
            } else {
                break;
            }
        }
    }
}
