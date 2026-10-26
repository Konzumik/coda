//! Режимы отображения. Каждый рисует в сетку размером с область под шапкой.

mod bars;
mod mirror;
mod mountains;
pub mod ring;

use super::grid::{Grid, scale};
use crate::color::{Gradient, Lab, Rgb};

/// Блоки, растущие снизу: 0…7 восьмых (U+2581–2587).
pub const LOWER: [char; 8] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇'];
/// Блоки, растущие сверху: 0…7 восьмых (▔ ▀ и U+1FB82–1FB86).
pub const UPPER: [char; 8] = [' ', '▔', '\u{1FB82}', '\u{1FB83}', '▀', '\u{1FB84}', '\u{1FB85}', '\u{1FB86}'];
/// Тонкая черта в k-й восьмой клетки снизу: ▁, HORIZONTAL ONE EIGHTH
/// BLOCK-7…2 (U+1FB7B…1FB76), ▔.
pub const LINE: [char; 8] = ['▁', '\u{1FB7B}', '\u{1FB7A}', '\u{1FB79}', '\u{1FB78}', '\u{1FB77}', '\u{1FB76}', '▔'];
pub const WHITE: Lab = Lab { l: 1.0, a: 0.0, b: 0.0 };

const SWITCH_S: f32 = 0.15;
const TOAST_S: f32 = 1.4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Bars,
    Mirror,
    Mountains,
    Ring,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Bars, Kind::Mirror, Kind::Mountains, Kind::Ring];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Bars => "ПОЛОСЫ",
            Kind::Mirror => "ЗЕРКАЛО",
            Kind::Mountains => "ГОРЫ",
            Kind::Ring => "КОЛЬЦО",
        }
    }

    /// Клавиши 1–4.
    pub fn from_key(c: char) -> Option<Kind> {
        let i = c.to_digit(10)? as usize;
        Kind::ALL.get(i.checked_sub(1)?).copied()
    }

    fn number(self) -> usize {
        Kind::ALL.iter().position(|&k| k == self).unwrap() + 1
    }
}

/// Всё, что нужно режимам для кадра.
pub struct Scene<'a> {
    pub levels: &'a [f32],
    pub peaks: &'a [f32],
    pub grad: &'a Gradient,
    pub dt: f32,
    /// Ширина клетки / высота клетки в пикселях.
    pub cell_aspect: f32,
    // геометрия столбиков из Layout
    pub bars: usize,
    pub bar_w: usize,
    pub gap: usize,
    pub left: usize,
}

pub struct Modes {
    kind: Kind,
    /// Идёт смена: куда и сколько прошло (0…1).
    switching: Option<(Kind, f32)>,
    toast: f32,
    ring: ring::Ring,
}

impl Modes {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            switching: None,
            toast: 0.0,
            ring: ring::Ring::default(),
        }
    }

    pub fn select(&mut self, k: Kind) {
        let target = self.switching.map_or(self.kind, |(to, _)| to);
        if k != target {
            self.switching = Some((k, 0.0));
        }
        self.toast = TOAST_S;
    }

    /// Режим, который сейчас на экране (при смене меняется на середине).
    pub fn visible(&self) -> Kind {
        self.kind
    }

    /// Рисует кадр, возвращает яркость (во время смены режим гаснет и
    /// загорается новый).
    pub fn draw(&mut self, g: &mut Grid, s: &Scene) -> f32 {
        let mut bright = 1.0;
        if let Some((to, t)) = self.switching {
            let t = t + s.dt / SWITCH_S;
            if t >= 0.5 {
                self.kind = to;
            }
            if t >= 1.0 {
                self.switching = None;
            } else {
                self.switching = Some((to, t));
                bright = (2.0 * t - 1.0).abs();
            }
        }

        self.ring.update(s.dt);

        g.clear();
        match self.kind {
            Kind::Bars => bars::draw(g, s),
            Kind::Mirror => mirror::draw(g, s),
            Kind::Mountains => mountains::draw(g, s),
            Kind::Ring => self.ring.draw(g, s),
        }

        if self.toast > 0.0 {
            self.toast -= s.dt;
            let target = self.switching.map_or(self.kind, |(to, _)| to);
            let text = format!(" {:02} ━━ {} ", target.number(), target.name());
            let fade = (self.toast / 0.4).clamp(0.0, 1.0);
            let c = scale(s.grad.lab(1.0).mix(WHITE, 0.2).to_rgb(), fade);
            let x = g.w.saturating_sub(text.chars().count() + 1);
            g.text(x, 0, &text, c);
        }
        bright
    }
}

/// Высота в восьмых долях клетки.
pub fn eighths(v: f32, rows: usize) -> usize {
    ((v * (rows * 8) as f32).round() as usize).min(rows * 8)
}

/// Значение спектра в дробной позиции u ∈ [0, 1]: Catmull-Rom между
/// полосами, чтобы контур был гладким, а не ступенчатым.
pub fn sample(v: &[f32], u: f32) -> f32 {
    let n = v.len();
    if n == 0 {
        return 0.0;
    }
    let x = u.clamp(0.0, 1.0) * (n - 1) as f32;
    let i = x.floor() as usize;
    let t = x - i as f32;
    let at = |k: isize| v[k.clamp(0, n as isize - 1) as usize];
    let (p0, p1, p2, p3) = (at(i as isize - 1), at(i as isize), at(i as isize + 1), at(i as isize + 2));
    let r = 0.5
        * (2.0 * p1
            + (-p0 + p2) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t * t * t);
    r.clamp(0.0, 1.0)
}

/// Цвета по строкам снизу вверх: (основной, светлый для пиков).
fn row_colors(grad: &Gradient, rows: usize) -> Vec<(Rgb, Rgb)> {
    (0..rows)
        .map(|c| {
            let lab = grad.lab((c as f32 + 0.5) / rows as f32);
            (lab.to_rgb(), lab.mix(WHITE, 0.55).to_rgb())
        })
        .collect()
}

/// Столбик из блоков снизу вверх с пиком-чертой. `y_bottom` — нижняя строка.
#[allow(clippy::too_many_arguments)]
fn column(g: &mut Grid, x0: usize, w: usize, y_bottom: usize, rows: usize, ht: usize, pk: usize, colors: &[(Rgb, Rgb)]) {
    for cell in 0..rows {
        let base = cell * 8;
        let (bar_c, peak_c) = colors[cell];
        let (ch, c) = if ht >= base + 8 {
            ('█', bar_c)
        } else if ht > base {
            (LOWER[ht - base], bar_c)
        } else if pk > ht && (base..base + 8).contains(&pk) {
            (LINE[pk - base], peak_c)
        } else {
            continue;
        };
        for dx in 0..w {
            g.set(x0 + dx, y_bottom - cell, ch, c);
        }
    }
}
