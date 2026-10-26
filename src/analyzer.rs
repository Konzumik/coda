//! FFT → логарифмические полосы → децибелы → автоусиление → сглаживание и пики.

use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

pub const FFT_SIZE: usize = 4096; // ~85 мс окно, 11.7 Гц на бин при 48 кГц

const F_MIN: f32 = 40.0;
const F_MAX: f32 = 12_500.0;
/// Сколько дБ помещается от низа экрана до верха.
const RANGE_DB: f32 = 48.0;
/// Запас над громкой полосой, чтобы бас не упирался в потолок постоянно.
const HEADROOM_DB: f32 = 2.0;
/// Ниже этого потолок не опускается: в тишине не усиливаем шум.
const CEIL_MIN_DB: f32 = -42.0;
const AGC_UP: f32 = 0.08;
const AGC_DOWN: f32 = 4.0;

/// Характер движения. Времена в секундах, ускорения в «экранах/с²».
#[derive(Clone, Copy, Debug)]
pub struct Motion {
    /// Доля соседних полос в значении полосы (сглаживание контура).
    pub spread: f32,
    /// Подавление дрожи на входе, в обе стороны.
    pub smooth_in: f32,
    /// Скорость подъёма полосы.
    pub rise: f32,
    /// Падение как в cava: по параболе, за это время до нуля с любой высоты.
    pub fall_time: f32,
    /// Интегратор на выходе (в cava noise_reduction): скругляет движение.
    pub integral: f32,
    pub peak_hold: f32,
    pub peak_gravity: f32,
    /// Наклон спектра, дБ на октаву относительно 1 кГц.
    pub tilt_db_per_oct: f32,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            spread: 0.15,
            smooth_in: 0.0,
            rise: 0.02,
            fall_time: 0.6,
            integral: 0.025,
            peak_hold: 0.4,
            peak_gravity: 1.6,
            tilt_db_per_oct: -1.0,
        }
    }
}

struct Band {
    lo: f32, // границы в бинах FFT (дробные)
    hi: f32,
    octave: f32, // log2(центр / 1 кГц)
    center_hz: f32,
}

pub struct Analyzer {
    pub motion: Motion,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    buf: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    power: Vec<f32>,
    bin_hz: f32,
    bands: Vec<Band>,
    /// Нормированное значение полос 0..1 до всякого сглаживания.
    pub raw: Vec<f32>,
    input: Vec<f32>,
    /// Высота после стадии подъёма/падения, до интегратора.
    shaped: Vec<f32>,
    fall_from: Vec<f32>,
    fall_t: Vec<f32>,
    /// Текущая высота полос, 0..1
    pub levels: Vec<f32>,
    /// Высота пиков, 0..1
    pub peaks: Vec<f32>,
    hold: Vec<f32>,
    fall: Vec<f32>,
    pub ceiling_db: f32,
}

impl Analyzer {
    pub fn new(sample_rate: u32, bands: usize) -> Self {
        let fft = FftPlanner::new().plan_fft_forward(FFT_SIZE);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        // периодическое окно Ханна
        let window = (0..FFT_SIZE)
            .map(|i| {
                0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT_SIZE as f32).cos()
            })
            .collect();
        let mut a = Self {
            motion: Motion::default(),
            fft,
            window,
            buf: vec![Complex::default(); FFT_SIZE],
            scratch,
            power: vec![0.0; FFT_SIZE / 2 + 1],
            bin_hz: sample_rate as f32 / FFT_SIZE as f32,
            bands: Vec::new(),
            raw: Vec::new(),
            input: Vec::new(),
            shaped: Vec::new(),
            fall_from: Vec::new(),
            fall_t: Vec::new(),
            levels: Vec::new(),
            peaks: Vec::new(),
            hold: Vec::new(),
            fall: Vec::new(),
            ceiling_db: CEIL_MIN_DB,
        };
        a.set_bands(bands);
        a
    }

    /// Перестраивает полосы (при ресайзе). Высоты пересэмплируются,
    /// чтобы картинка не обнулялась.
    pub fn set_bands(&mut self, n: usize) {
        let n = n.max(1);
        let ratio = F_MAX / F_MIN;
        let edge = |i: usize| F_MIN * ratio.powf(i as f32 / n as f32);
        self.bands = (0..n)
            .map(|i| {
                let (f0, f1) = (edge(i), edge(i + 1));
                let center = (f0 * f1).sqrt();
                Band {
                    lo: f0 / self.bin_hz,
                    hi: f1 / self.bin_hz,
                    octave: (center / 1000.0).log2(),
                    center_hz: center,
                }
            })
            .collect();
        self.input = resample(&self.input, n);
        self.shaped = resample(&self.shaped, n);
        self.fall_from = self.shaped.clone();
        self.levels = resample(&self.levels, n);
        self.peaks = resample(&self.peaks, n);
        self.raw = vec![0.0; n];
        self.fall_t = vec![0.0; n];
        self.hold = vec![0.0; n];
        self.fall = vec![0.0; n];
    }

    pub fn band_hz(&self, i: usize) -> f32 {
        self.bands[i].center_hz
    }

    pub fn process(&mut self, samples: &[f32], live: bool, dt: f32) {
        let m = self.motion;
        for (i, c) in self.buf.iter_mut().enumerate() {
            let s = if live { samples[i] * self.window[i] } else { 0.0 };
            *c = Complex::new(s, 0.0);
        }
        self.fft.process_with_scratch(&mut self.buf, &mut self.scratch);

        // нормировка: синус полной амплитуды ≈ 0 дБ (сумма окна Ханна = N/2)
        let norm = 4.0 / FFT_SIZE as f32;
        for (p, c) in self.power.iter_mut().zip(&self.buf) {
            *p = c.norm_sqr() * norm * norm;
        }

        // пока в raw лежат дБ
        let mut frame_max = f32::MIN;
        for (d, b) in self.raw.iter_mut().zip(&self.bands) {
            *d = 10.0 * (band_power(&self.power, b.lo, b.hi) + 1e-14).log10()
                + m.tilt_db_per_oct * b.octave;
            frame_max = frame_max.max(*d);
        }

        // автоусиление: быстро вверх, медленно вниз
        let target = frame_max.max(CEIL_MIN_DB);
        let tau = if target > self.ceiling_db { AGC_UP } else { AGC_DOWN };
        self.ceiling_db += (target - self.ceiling_db) * smooth(dt, tau);

        let floor = self.ceiling_db + HEADROOM_DB - RANGE_DB;
        for d in &mut self.raw {
            *d = ((*d - floor) / RANGE_DB).clamp(0.0, 1.0);
        }

        let n = self.bands.len();
        let k_in = smooth(dt, m.smooth_in);
        let k_rise = smooth(dt, m.rise);
        let k_int = smooth(dt, m.integral);
        for i in 0..n {
            let left = self.raw[i.saturating_sub(1)];
            let right = self.raw[(i + 1).min(n - 1)];
            let v = self.raw[i] * (1.0 - 2.0 * m.spread) + (left + right) * m.spread;
            self.input[i] += (v - self.input[i]) * k_in;
            let inp = self.input[i];

            // вверх быстро; вниз по параболе от точки начала падения (как в cava)
            let cur = self.shaped[i];
            let shaped = if inp >= cur {
                let v = cur + (inp - cur) * k_rise;
                self.fall_from[i] = v;
                self.fall_t[i] = 0.0;
                v
            } else {
                self.fall_t[i] += dt;
                let x = self.fall_t[i] / m.fall_time;
                (self.fall_from[i] * (1.0 - x * x)).max(inp)
            };
            self.shaped[i] = shaped;
            self.levels[i] += (shaped - self.levels[i]) * k_int;
            let lvl = self.levels[i];

            // пик: висит, потом падает с ускорением
            if lvl >= self.peaks[i] {
                self.peaks[i] = lvl;
                self.hold[i] = m.peak_hold;
                self.fall[i] = 0.0;
            } else if self.hold[i] > 0.0 {
                self.hold[i] -= dt;
            } else {
                self.fall[i] += m.peak_gravity * dt;
                self.peaks[i] = (self.peaks[i] - self.fall[i] * dt).max(lvl);
            }
        }
    }
}

/// Коэффициент экспоненциального сглаживания, не зависящий от FPS.
fn smooth(dt: f32, tau: f32) -> f32 {
    if tau <= 0.0 { 1.0 } else { 1.0 - (-dt / tau).exp() }
}

/// Энергия в полосе [lo, hi) в бинах: каждый бин k покрывает [k-0.5, k+0.5),
/// берём долю перекрытия. Работает и для полос уже одного бина.
fn band_power(power: &[f32], lo: f32, hi: f32) -> f32 {
    let k0 = (lo + 0.5).floor() as usize;
    let k1 = ((hi + 0.5).floor() as usize).min(power.len() - 1);
    (k0..=k1)
        .map(|k| {
            let overlap = hi.min(k as f32 + 0.5) - lo.max(k as f32 - 0.5);
            if overlap > 0.0 { power[k] * overlap } else { 0.0 }
        })
        .sum()
}

fn resample(v: &[f32], n: usize) -> Vec<f32> {
    if v.is_empty() {
        return vec![0.0; n];
    }
    (0..n)
        .map(|i| {
            let x = (i as f32 + 0.5) / n as f32 * v.len() as f32 - 0.5;
            let x = x.clamp(0.0, (v.len() - 1) as f32);
            let (j, t) = (x.floor() as usize, x.fract());
            let k = (j + 1).min(v.len() - 1);
            v[j] * (1.0 - t) + v[k] * t
        })
        .collect()
}
