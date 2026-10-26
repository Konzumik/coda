//! 6 — кольцо: обложка в центре, спектр лучами вокруг (брайль). Бас внизу,
//! верха наверху, левая и правая половины зеркальны. Кольцо медленно
//! вращается.

use super::{Scene, WHITE, sample};
use crate::color::Rgb;
use crate::render::CoverBox;
use crate::render::grid::{Braille, Grid};
use std::f32::consts::{PI, TAU};

/// Высота обложки в доле высоты области.
const COVER_SHARE: f32 = 0.3;
/// Внутренний радиус в долях стороны обложки: чуть больше половины
/// диагонали (0.707), чтобы углы обложки не залезали на лучи.
const INNER: f32 = 0.76;
/// Расстояние между лучами по внутреннему кругу, в точках.
const RAY_SPACING: f32 = 2.2;
const SPIN: f32 = 0.06; // рад/с
const LUT: usize = 32;

#[derive(Default)]
pub struct Ring {
    rot: f32,
}

/// Где стоит обложка в этом режиме: по центру области.
pub fn cover_box(top: usize, area_h: usize, cols: usize, cell_aspect: f32) -> Option<CoverBox> {
    let rows = (area_h as f32 * COVER_SHARE).round() as usize;
    if rows < 2 {
        return None;
    }
    let c = (rows as f32 / cell_aspect).round() as usize;
    Some(CoverBox { row: top + (area_h - rows) / 2, col: cols.saturating_sub(c) / 2 + 1, cols: c, rows })
}

impl Ring {
    pub fn update(&mut self, dt: f32) {
        self.rot = (self.rot + dt * SPIN) % TAU;
    }

    pub fn draw(&self, g: &mut Grid, s: &Scene) {
        let mut b = Braille::new(g.w, g.h);
        // брайлевая точка: (клетка_ш / 2) × (клетка_в / 4); считаем в единицах
        // высоты точки, по x делим на это отношение — круг выходит круглым
        let dot_aspect = 2.0 * s.cell_aspect;
        let (cx, cy) = (b.w as f32 / 2.0 * dot_aspect, b.h as f32 / 2.0);
        let side = (g.h as f32 * COVER_SHARE).round() * 4.0;
        let r0 = side * INNER;
        let len = cx.min(cy) - 1.0 - r0;
        if len < 2.0 {
            return;
        }

        let lut: Vec<Rgb> = (0..LUT).map(|i| s.grad.lab(i as f32 / (LUT - 1) as f32).to_rgb()).collect();
        let color = |t: f32| lut[(t.clamp(0.0, 1.0) * (LUT - 1) as f32).round() as usize];
        let peak_c = s.grad.lab(1.0).mix(WHITE, 0.55).to_rgb();
        let mut dot = |r: f32, a: f32, c: Rgb| {
            b.dot(((cx + r * a.cos()) / dot_aspect) as i32, (cy + r * a.sin()) as i32, c);
        };

        // тонкая окружность вокруг обложки
        let ring_c = s.grad.lab(0.0).to_rgb();
        let around = (TAU * r0 / 1.2) as usize;
        for k in 0..around {
            dot(r0 - 2.0, TAU * k as f32 / around as f32, ring_c);
        }

        let rays = (TAU * r0 / RAY_SPACING) as usize;
        for k in 0..rays {
            let a = TAU * k as f32 / rays as f32 + self.rot;
            // угол от низа экрана (y растёт вниз, низ — это π/2)
            let from_bottom = (a - PI / 2.0).rem_euclid(TAU);
            let u = if from_bottom > PI { TAU - from_bottom } else { from_bottom } / PI;
            let v = sample(s.levels, u);
            let mut r = r0;
            while r <= r0 + v * len {
                dot(r, a, color((r - r0) / len));
                r += 0.5;
            }
            let p = sample(s.peaks, u);
            if p > v + 0.02 {
                dot(r0 + p * len, a, peak_c);
            }
        }
        b.blit(g);
    }
}
