//! Палитра из обложки: k-means в OKLab, отсев тёмных и серых кластеров,
//! подтяжка насыщенности, плавный переход между палитрами.

use crate::color::{Gradient, Lab, Rgb};
use image::{imageops::FilterType, RgbImage};

/// Палитра хранится как 5 опорных цветов градиента: так любые две палитры
/// (из 3, 4 или 5 цветов) перетекают друг в друга поточечно.
pub const STOPS: usize = 5;

const THUMB: u32 = 64;
const K: usize = 8;
const ITERATIONS: usize = 16;
/// Темнее — «грязь», светлее — почти белый.
const MIN_L: f32 = 0.30;
const MAX_L: f32 = 0.97;
/// Серее этого кластер не берём.
const MIN_CHROMA: f32 = 0.045;
/// Цвета палитры должны заметно различаться.
const MIN_DIST: f32 = 0.08;
const MAX_COLORS: usize = 5;
/// Итоговые цвета должны светиться на тёмном фоне.
const OUT_L: (f32, f32) = (0.58, 0.88);
const OUT_MIN_CHROMA: f32 = 0.12;
const TRANSITION_S: f32 = 1.5;

/// Палитра по умолчанию (нет плеера или обложки), снизу вверх.
const FALLBACK: [Rgb; 4] = [
    Rgb::hex(0x02D7F2),
    Rgb::hex(0xB967FF),
    Rgb::hex(0xFF2A6D),
    Rgb::hex(0xFCEE0A),
];
/// Акцент для чёрно-белых обложек.
const MONO_ACCENT: Rgb = Rgb::hex(0xFF2A55);

pub fn fallback() -> Vec<Lab> {
    FALLBACK.iter().map(|c| c.to_lab()).collect()
}

struct Cluster {
    center: Lab,
    weight: f32,
}

/// 3–5 цветов в порядке градиента: низ полос → верх.
pub fn extract(img: &RgbImage) -> Vec<Lab> {
    let thumb = image::imageops::resize(img, THUMB, THUMB, FilterType::Triangle);
    let px: Vec<Lab> = thumb.pixels().map(|p| Rgb(p[0], p[1], p[2]).to_lab()).collect();
    let clusters = kmeans(&px);

    let mut candidates: Vec<&Cluster> = clusters
        .iter()
        .filter(|c| (MIN_L..=MAX_L).contains(&c.center.l) && c.center.chroma() >= MIN_CHROMA)
        .collect();
    // крупный и сочный важнее мелкого и блёклого
    let score = |c: &Cluster| c.weight.powf(0.6) * c.center.chroma();
    candidates.sort_by(|a, b| score(b).total_cmp(&score(a)));

    // различие проверяем уже после осветления: иначе близкие тёмные цвета
    // после подъёма светлоты слипаются в один
    let mut colors: Vec<Lab> = Vec::new();
    for c in candidates {
        if colors.len() == MAX_COLORS {
            break;
        }
        let c = beautify(c.center);
        if colors.iter().all(|p| p.dist(c) >= MIN_DIST) {
            colors.push(c);
        }
    }

    match colors.len() {
        0 => return monochrome(&clusters),
        1 => colors = companions(colors[0]),
        2 => {
            // третий — светлый блик от более светлого цвета
            let top = if colors[0].l > colors[1].l { colors[0] } else { colors[1] };
            colors.push(Lab::from_lch((top.l + 0.08).min(0.92), top.chroma() * 0.85, top.hue()).into_gamut());
        }
        _ => {}
    }
    smoothest_order(colors)
}

/// Порядок цветов в градиенте: перебираем все перестановки (их максимум 120)
/// и берём ту, где переходы между соседями в сумме самые короткие. Иначе при
/// сортировке по светлоте рядом встают разные оттенки и полосы идут зеброй.
/// Светлый конец — наверх.
fn smoothest_order(colors: Vec<Lab>) -> Vec<Lab> {
    let n = colors.len();
    let path_len = |order: &[usize]| -> f32 {
        order.windows(2).map(|w| colors[w[0]].dist(colors[w[1]])).sum()
    };
    let mut best: Vec<usize> = (0..n).collect();
    let mut best_len = path_len(&best);
    let mut order: Vec<usize> = (0..n).collect();
    permute(&mut order, 0, &mut |o| {
        let len = path_len(o);
        if len < best_len - 1e-6 {
            best_len = len;
            best = o.to_vec();
        }
    });
    if colors[best[0]].l > colors[best[n - 1]].l {
        best.reverse();
    }
    best.into_iter().map(|i| colors[i]).collect()
}

fn permute(v: &mut [usize], k: usize, visit: &mut impl FnMut(&[usize])) {
    if k == v.len() {
        visit(v);
        return;
    }
    for i in k..v.len() {
        v.swap(k, i);
        permute(v, k + 1, visit);
        v.swap(k, i);
    }
}

/// Поднять в рабочий диапазон светлоты и подтянуть насыщенность.
fn beautify(c: Lab) -> Lab {
    let l = c.l.clamp(min_lightness(c.hue()), OUT_L.1);
    Lab::from_lch(l, c.chroma().max(OUT_MIN_CHROMA), c.hue()).into_gamut()
}

/// Минимальная светлота зависит от оттенка: тёмный оранжевый — это
/// коричневый, тёмный жёлтый — оливковый. Им нужно больше света, чем
/// красному или синему, чтобы остаться чистыми.
fn min_lightness(hue: f32) -> f32 {
    let h = hue.to_degrees().rem_euclid(360.0);
    // плавный «холм» высотой height вокруг оттенка center (в градусах)
    let hill = |center: f32, width: f32, height: f32| {
        let d = (h - center).abs();
        let d = d.min(360.0 - d);
        if d < width { height * 0.5 * (1.0 + (std::f32::consts::PI * d / width).cos()) } else { 0.0 }
    };
    OUT_L.0 + hill(60.0, 40.0, 0.13) + hill(100.0, 35.0, 0.23)
}

/// Из одного цвета — тройка соседей по оттенку и светлоте.
fn companions(c: Lab) -> Vec<Lab> {
    let (l, ch, h) = (c.l, c.chroma(), c.hue());
    let shift = 20f32.to_radians();
    vec![
        Lab::from_lch((l - 0.14).max(0.5), ch * 1.1, h - shift).into_gamut(),
        c,
        Lab::from_lch((l + 0.12).min(0.92), ch * 0.75, h + shift).into_gamut(),
    ]
}

/// Чёрно-белая обложка: оттенки серого и один неоновый акцент. Если в
/// обложке есть хоть намёк на цвет, акцент берёт его оттенок.
fn monochrome(clusters: &[Cluster]) -> Vec<Lab> {
    let hint = clusters
        .iter()
        .filter(|c| c.weight >= 0.01 && c.center.chroma() >= 0.02)
        .max_by(|a, b| a.center.chroma().total_cmp(&b.center.chroma()));
    let accent = match hint {
        Some(c) => Lab::from_lch(0.64, 0.17, c.center.hue()).into_gamut(),
        None => MONO_ACCENT.to_lab(),
    };
    let grey = |l| Lab { l, a: 0.0, b: 0.0 };
    let mut colors = vec![grey(0.5), accent, grey(0.92)];
    colors.sort_by(|a, b| a.l.total_cmp(&b.l));
    colors
}

fn kmeans(px: &[Lab]) -> Vec<Cluster> {
    // детерминированный xorshift: одна обложка — всегда одна палитра
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut rand = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32
    };
    let d2 = |a: &Lab, b: &Lab| (a.l - b.l).powi(2) + (a.a - b.a).powi(2) + (a.b - b.b).powi(2);

    // k-means++: каждый следующий центр подальше от уже выбранных
    let mut centers = vec![px[(rand() * px.len() as f32) as usize % px.len()]];
    while centers.len() < K {
        let dist: Vec<f32> = px
            .iter()
            .map(|p| centers.iter().map(|c| d2(p, c)).fold(f32::MAX, f32::min))
            .collect();
        let total: f32 = dist.iter().sum();
        if total <= 0.0 {
            break; // картинка из одного цвета
        }
        let mut r = rand() * total;
        let mut idx = px.len() - 1;
        for (i, d) in dist.iter().enumerate() {
            r -= d;
            if r <= 0.0 {
                idx = i;
                break;
            }
        }
        centers.push(px[idx]);
    }

    let mut counts = vec![0usize; centers.len()];
    for _ in 0..ITERATIONS {
        let mut sums = vec![(0.0f32, 0.0f32, 0.0f32); centers.len()];
        counts.fill(0);
        for p in px {
            let (best, _) = centers
                .iter()
                .enumerate()
                .map(|(i, c)| (i, d2(p, c)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap();
            sums[best].0 += p.l;
            sums[best].1 += p.a;
            sums[best].2 += p.b;
            counts[best] += 1;
        }
        for (c, (s, &n)) in centers.iter_mut().zip(sums.iter().zip(&counts)) {
            if n > 0 {
                let n = n as f32;
                *c = Lab { l: s.0 / n, a: s.1 / n, b: s.2 / n };
            }
        }
    }

    centers
        .into_iter()
        .zip(counts)
        .map(|(center, n)| Cluster { center, weight: n as f32 / px.len() as f32 })
        .collect()
}

/// Плавный переход между палитрами в OKLab.
pub struct Blend {
    from: [Lab; STOPS],
    to: [Lab; STOPS],
    t: f32,
}

impl Blend {
    pub fn new(colors: &[Lab]) -> Self {
        let s = stops(colors);
        Self { from: s, to: s, t: 1.0 }
    }

    /// Новая цель. Если прошлый переход не закончился, стартуем с того,
    /// что сейчас на экране, — без скачка.
    pub fn set_target(&mut self, colors: &[Lab]) {
        self.from = self.current();
        self.to = stops(colors);
        self.t = 0.0;
    }

    pub fn update(&mut self, dt: f32) {
        self.t = (self.t + dt / TRANSITION_S).min(1.0);
    }

    pub fn current(&self) -> [Lab; STOPS] {
        let e = self.t * self.t * (3.0 - 2.0 * self.t); // smoothstep
        std::array::from_fn(|i| self.from[i].mix(self.to[i], e))
    }

    pub fn gradient(&self) -> Gradient {
        Gradient::from_labs(self.current().to_vec())
    }
}

fn stops(colors: &[Lab]) -> [Lab; STOPS] {
    let g = Gradient::from_labs(colors.to_vec());
    std::array::from_fn(|i| g.lab(i as f32 / (STOPS - 1) as f32))
}
