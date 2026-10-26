//! Цвета и градиенты в OKLab (https://bottosson.github.io/posts/oklab/):
//! смешивание в нём не даёт грязных серо-бурых середин, как в RGB.

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Clone, Copy, Debug)]
pub struct Lab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

impl Rgb {
    pub const fn hex(v: u32) -> Self {
        Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    pub fn to_lab(self) -> Lab {
        let r = to_linear(self.0);
        let g = to_linear(self.1);
        let b = to_linear(self.2);
        let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
        Lab {
            l: 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            a: 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            b: 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
        }
    }
}

impl Lab {
    pub fn from_lch(l: f32, chroma: f32, hue: f32) -> Lab {
        Lab { l, a: chroma * hue.cos(), b: chroma * hue.sin() }
    }

    /// Насыщенность в OKLCh: 0 — серый, ~0.1 — сочный, ~0.3 — предел sRGB.
    pub fn chroma(self) -> f32 {
        self.a.hypot(self.b)
    }

    /// Оттенок в радианах.
    pub fn hue(self) -> f32 {
        self.b.atan2(self.a)
    }

    /// Разница цветов (ΔE в OKLab): ~0.02 едва заметно, ~0.1 явно разные.
    pub fn dist(self, o: Lab) -> f32 {
        ((self.l - o.l).powi(2) + (self.a - o.a).powi(2) + (self.b - o.b).powi(2)).sqrt()
    }

    pub fn in_gamut(self) -> bool {
        self.to_linear().iter().all(|c| (-1e-4..=1.0 + 1e-4).contains(c))
    }

    /// Уменьшает насыщенность, пока цвет не влезет в sRGB; светлота и оттенок
    /// сохраняются (обрезка каналов исказила бы оттенок).
    pub fn into_gamut(self) -> Lab {
        if self.in_gamut() {
            return self;
        }
        let (c, h) = (self.chroma(), self.hue());
        let (mut lo, mut hi) = (0.0, c);
        for _ in 0..20 {
            let mid = (lo + hi) * 0.5;
            if Lab::from_lch(self.l, mid, h).in_gamut() { lo = mid } else { hi = mid }
        }
        Lab::from_lch(self.l, lo, h)
    }

    fn to_linear(self) -> [f32; 3] {
        let l = self.l + 0.3963377774 * self.a + 0.2158037573 * self.b;
        let m = self.l - 0.1055613458 * self.a - 0.0638541728 * self.b;
        let s = self.l - 0.0894841775 * self.a - 1.2914855480 * self.b;
        let (l, m, s) = (l * l * l, m * m * m, s * s * s);
        [
            4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
            -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
            -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
        ]
    }

    pub fn to_rgb(self) -> Rgb {
        let [r, g, b] = self.to_linear();
        Rgb(from_linear(r), from_linear(g), from_linear(b))
    }

    /// Смешивание в OKLab с сохранением насыщенности: оттенок берётся из
    /// обычной смеси, а хрома интерполируется отдельно. Иначе между
    /// противоположными оттенками (синий → оранжевый) середина уходит в серый.
    pub fn mix(self, o: Lab, t: f32) -> Lab {
        let m = Lab {
            l: self.l + (o.l - self.l) * t,
            a: self.a + (o.a - self.a) * t,
            b: self.b + (o.b - self.b) * t,
        };
        let c = m.chroma();
        if c < 1e-4 {
            return m; // ровно противоположные цвета: оттенка нет, оставляем как есть
        }
        let target = self.chroma() + (o.chroma() - self.chroma()) * t;
        Lab::from_lch(m.l, target, m.hue()).into_gamut()
    }
}

fn to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn from_linear(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let c = if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (c * 255.0).round() as u8
}

/// Равномерный градиент по нескольким опорным цветам.
pub struct Gradient {
    stops: Vec<Lab>,
}

impl Gradient {
    pub fn new(colors: &[Rgb]) -> Self {
        Self::from_labs(colors.iter().map(|c| c.to_lab()).collect())
    }

    pub fn from_labs(stops: Vec<Lab>) -> Self {
        assert!(!stops.is_empty());
        Self { stops }
    }

    /// t от 0 до 1
    pub fn lab(&self, t: f32) -> Lab {
        let last = self.stops.len() - 1;
        let x = t.clamp(0.0, 1.0) * last as f32;
        let i = (x.floor() as usize).min(last.saturating_sub(1));
        if last == 0 {
            return self.stops[0];
        }
        self.stops[i].mix(self.stops[i + 1], x - i as f32)
    }
}
