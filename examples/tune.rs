//! Замер плавности анализатора на записи, без терминала.
//! cargo run --release --example tune -- <файл.raw f32le стерео 48к> [fps] [burst|smooth|sweep]
//! burst — окно прыгает пачками, как отдаёт parec; smooth — через Playhead;
//! sweep — перебор параметров движения (через Playhead).

use coda::analyzer::{Analyzer, Motion, FFT_SIZE};
use coda::audio::{Playhead, SAMPLE_RATE};

const BANDS: usize = 62;
const WARMUP_S: f64 = 1.0;

struct Stats {
    reversals: f32,
    jerk: f32,
    peak_jerk: f32,
    dynamics: f32,
    spatial: f32,
    lag_ms: f32,
    balance: [f32; 3],
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("путь к .raw");
    let fps: f32 = args.next().map_or(90.0, |s| s.parse().unwrap());
    let mode = args.next().unwrap_or_else(|| "smooth".into());

    let bytes = std::fs::read(&path).expect("читаю запись");
    let mono: Vec<f32> = bytes
        .chunks_exact(8)
        .map(|f| {
            let l = f32::from_le_bytes([f[0], f[1], f[2], f[3]]);
            let r = f32::from_le_bytes([f[4], f[5], f[6], f[7]]);
            (l + r) * 0.5
        })
        .collect();

    if mode != "sweep" {
        let s = measure(&mono, Motion::default(), fps, mode == "burst");
        println!("режим {mode}, {fps} fps, {:?}", Motion::default());
        println!("  развороты/с на полосу: {:.1}", s.reversals);
        println!("  рывки полос: {:.3}   рывки пиков: {:.3}", s.jerk, s.peak_jerk);
        println!("  динамика (σ, %): {:.1}   задержка: {:.0} мс", s.dynamics, s.lag_ms);
        println!("  соседние полосы, разница %: {:.1}", s.spatial);
        let [b, m, h] = s.balance;
        println!("  средняя высота %: бас {b:.0} | середина {m:.0} | верх {h:.0}");
        return;
    }

    println!("in    fall  integ │ разв/с  рывки  динам  задерж");
    for smooth_in in [0.0, 0.02] {
        for fall_time in [0.25, 0.35, 0.5, 0.7] {
            for integral in [0.0, 0.02, 0.03, 0.05] {
                let m = Motion { smooth_in, fall_time, integral, ..Motion::default() };
                let s = measure(&mono, m, fps, false);
                println!(
                    "{smooth_in:<5} {fall_time:<5} {integral:<5} │ {:>5.1}  {:.3}  {:>5.1}  {:>4.0}мс",
                    s.reversals, s.jerk, s.dynamics, s.lag_ms
                );
            }
        }
    }
}

fn measure(mono: &[f32], motion: Motion, fps: f32, burst: bool) -> Stats {
    let mut an = Analyzer::new(SAMPLE_RATE, BANDS);
    an.motion = motion;
    let dt = 1.0 / fps;
    let rate = SAMPLE_RATE as f64;
    let mut playhead = Playhead::default();

    let mut hist: Vec<Vec<f32>> = Vec::new();
    let mut raw_hist: Vec<Vec<f32>> = Vec::new();
    let mut peak_hist: Vec<Vec<f32>> = Vec::new();
    let mut t = 0.0f64;
    loop {
        t += dt as f64;
        // сколько сэмплов «пришло» к моменту t: квант PipeWire 1024, пачки по 480
        let arrived = ((t * rate / 1024.0).floor() * 1024.0 / 480.0).floor() as u64 * 480;
        let end = if burst { arrived } else { playhead.advance(arrived, dt) } as usize;
        if end > mono.len() {
            break;
        }
        if end < FFT_SIZE {
            continue;
        }
        an.process(&mono[end - FFT_SIZE..end], true, dt);
        if t > WARMUP_S {
            hist.push(an.levels.clone());
            raw_hist.push(an.raw.clone());
            peak_hist.push(an.peaks.clone());
        }
    }

    let frames = hist.len();
    let secs = frames as f32 / fps;
    let jerk = |h: &[Vec<f32>]| {
        let mut s = 0.0;
        for w in h.windows(3) {
            for b in 0..BANDS {
                s += (w[2][b] - 2.0 * w[1][b] + w[0][b]).abs();
            }
        }
        s / ((h.len() - 2) * BANDS) as f32 * 100.0
    };

    let mut reversals = 0usize;
    for b in 0..BANDS {
        let mut dir = 0i8;
        for w in hist.windows(2) {
            let d = w[1][b] - w[0][b];
            let nd = if d > 1e-3 { 1 } else if d < -1e-3 { -1 } else { 0 };
            if nd != 0 && dir != 0 && nd != dir {
                reversals += 1;
            }
            if nd != 0 {
                dir = nd;
            }
        }
    }

    let mut dynamics = 0.0;
    for b in 0..BANDS {
        let m = hist.iter().map(|f| f[b]).sum::<f32>() / frames as f32;
        let var = hist.iter().map(|f| (f[b] - m).powi(2)).sum::<f32>() / frames as f32;
        dynamics += var.sqrt();
    }

    let spatial = hist
        .iter()
        .map(|f| f.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / (BANDS - 1) as f32)
        .sum::<f32>()
        / frames as f32;

    // задержка: сдвиг, при котором бас на экране сильнее всего похож на сырой бас
    let bass: Vec<usize> = (0..BANDS).filter(|&b| an.band_hz(b) < 150.0).collect();
    let avg = |h: &[Vec<f32>]| -> Vec<f32> {
        h.iter().map(|f| bass.iter().map(|&b| f[b]).sum::<f32>() / bass.len() as f32).collect()
    };
    let (shown, raw) = (avg(&hist), avg(&raw_hist));
    let corr = |lag: usize| -> f32 {
        let n = frames - lag;
        let (a, b) = (&raw[..n], &shown[lag..]);
        let (ma, mb) = (a.iter().sum::<f32>() / n as f32, b.iter().sum::<f32>() / n as f32);
        let cov: f32 = a.iter().zip(b).map(|(x, y)| (x - ma) * (y - mb)).sum();
        let va: f32 = a.iter().map(|x| (x - ma).powi(2)).sum();
        let vb: f32 = b.iter().map(|y| (y - mb).powi(2)).sum();
        cov / (va * vb).sqrt().max(1e-9)
    };
    let best_lag = (0..(fps * 0.2) as usize)
        .max_by(|&x, &y| corr(x).total_cmp(&corr(y)))
        .unwrap();

    let region = |lo: f32, hi: f32| {
        let idx: Vec<usize> =
            (0..BANDS).filter(|&b| (lo..hi).contains(&an.band_hz(b))).collect();
        hist.iter().map(|f| idx.iter().map(|&b| f[b]).sum::<f32>()).sum::<f32>()
            / (frames * idx.len()) as f32
            * 100.0
    };

    Stats {
        reversals: reversals as f32 / BANDS as f32 / secs,
        jerk: jerk(&hist),
        peak_jerk: jerk(&peak_hist),
        dynamics: dynamics / BANDS as f32 * 100.0,
        spatial: spatial * 100.0,
        lag_ms: best_lag as f32 / fps * 1000.0,
        balance: [region(0.0, 250.0), region(250.0, 2000.0), region(2000.0, 1e9)],
    }
}
