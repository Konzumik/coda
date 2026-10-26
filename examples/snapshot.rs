//! «Скриншот» режимов без терминала: прогоняет запись через анализатор и
//! рисует ту же сетку, что ушла бы в Kitty, в PNG (все режимы на листе).
//! cargo run --release --example snapshot -- <запись.raw> <обложка.jpg> <out.png> [секунда]

use image::{Rgb as Px, RgbImage, imageops::FilterType};
use coda::analyzer::{Analyzer, FFT_SIZE};
use coda::audio::{Playhead, SAMPLE_RATE};
use coda::color::Rgb;
use coda::palette::{self, Blend};
use coda::render::Layout;
use coda::render::grid::Grid;
use coda::render::modes::{Kind, Modes};

const COLS: u16 = 110;
const ROWS: u16 = 34;
const CW: u32 = 9;
const CH: u32 = 19;
const FPS: f32 = 90.0;
const BG: Px<u8> = Px([12, 12, 16]);

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (raw, cover_path, out) = (&a[0], &a[1], &a[2]);
    let at: f32 = a.get(3).map_or(6.0, |s| s.parse().unwrap());

    let bytes = std::fs::read(raw).unwrap();
    let mono: Vec<f32> = bytes
        .chunks_exact(8)
        .map(|f| {
            let l = f32::from_le_bytes([f[0], f[1], f[2], f[3]]);
            let r = f32::from_le_bytes([f[4], f[5], f[6], f[7]]);
            (l + r) * 0.5
        })
        .collect();
    let cover = image::open(cover_path).unwrap().to_rgb8();
    let blend = Blend::new(&palette::extract(&cover));
    let grad = blend.gradient();
    let lay = Layout::new(COLS, ROWS, CW as f32 / CH as f32);

    let pw = COLS as u32 * CW;
    let ph = lay.area_h as u32 * CH;
    let mut sheet = RgbImage::from_pixel(pw * 2 + 30, (ph + 20) * 2 + 10, Px([40, 40, 48]));

    for (n, kind) in Kind::ALL.iter().enumerate() {
        let mut an = Analyzer::new(SAMPLE_RATE, lay.bars);
        let mut modes = Modes::new(*kind);
        let mut grid = Grid::new(lay.cols, lay.area_h);
        let mut head = Playhead::default();
        let dt = 1.0 / FPS;
        let mut t = 0.0f64;
        let mut bright = 1.0;
        while t < at as f64 {
            t += dt as f64;
            let arrived = (t * SAMPLE_RATE as f64) as u64;
            let end = head.advance(arrived, dt) as usize;
            if end < FFT_SIZE || end > mono.len() {
                continue;
            }
            let win = &mono[end - FFT_SIZE..end];
            an.process(win, true, dt);
            let scene = lay.scene(&an.levels, &an.peaks, &grad, dt);
            bright = modes.draw(&mut grid, &scene);
        }

        let mut img = RgbImage::from_pixel(pw, ph, BG);
        for y in 0..grid.h {
            for x in 0..grid.w {
                cell(&mut img, x as u32, y as u32, grid.get(x, y), bright);
            }
        }
        if let Some(b) = lay.cover_for(*kind) {
            let (bw, bh) = (b.cols as u32 * CW, b.rows as u32 * CH);
            let side = bw.min(bh);
            let thumb = image::imageops::resize(&cover, side, side, FilterType::Triangle);
            let x = (b.col as u32 - 1) * CW + (bw - side) / 2;
            let y = (b.row as i64 - lay.top as i64) * CH as i64 + ((bh - side) / 2) as i64;
            if y >= 0 {
                image::imageops::overlay(&mut img, &thumb, x as i64, y);
            }
        }
        let (ox, oy) = ((n as u32 % 2) * (pw + 10) + 10, (n as u32 / 2) * (ph + 20) + 10);
        image::imageops::overlay(&mut sheet, &img, ox as i64, oy as i64);
    }
    sheet.save(out).unwrap();
    println!("→ {out}");
}

fn cell(img: &mut RgbImage, cx: u32, cy: u32, c: coda::render::grid::Cell, _bright: f32) {
    let (x0, y0) = (cx * CW, cy * CH);
    let px = |c: Rgb| Px([c.0, c.1, c.2]);
    let rect = |img: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, col: Px<u8>| {
        for yy in y..(y + h).min(CH) {
            for xx in x..(x + w).min(CW) {
                img.put_pixel(x0 + xx, y0 + yy, col);
            }
        }
    };
    if let Some(bg) = c.bg {
        rect(img, 0, 0, CW, CH, px(bg));
    }
    let fg = px(c.fg);
    let eighth = |k: u32| (CH * k + 4) / 8;
    let ch = c.ch as u32;
    match c.ch {
        ' ' => {}
        '█' => rect(img, 0, 0, CW, CH, fg),
        '\u{2581}'..='\u{2587}' => {
            let k = ch - 0x2580;
            rect(img, 0, CH - eighth(k), CW, eighth(k), fg);
        }
        '▀' => rect(img, 0, 0, CW, eighth(4), fg),
        '▔' => rect(img, 0, 0, CW, eighth(1), fg),
        '\u{1FB82}'..='\u{1FB86}' => {
            let k = [2, 3, 5, 6, 7][(ch - 0x1FB82) as usize];
            rect(img, 0, 0, CW, eighth(k), fg);
        }
        '\u{1FB76}'..='\u{1FB7B}' => {
            let k = ch - 0x1FB76 + 1; // BLOCK-2…7: k-я восьмая сверху (0-based)
            rect(img, 0, eighth(k), CW, eighth(k + 1) - eighth(k), fg);
        }
        '\u{2800}'..='\u{28FF}' => {
            let bits = ch - 0x2800;
            const POS: [(u32, u32); 8] = [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (0, 3), (1, 3)];
            for (i, (dx, dy)) in POS.iter().enumerate() {
                if bits & (1 << i) != 0 {
                    let d = 3;
                    rect(img, dx * CW / 2 + (CW / 2 - d) / 2, dy * CH / 4 + (CH / 4 - d) / 2, d, d, fg);
                }
            }
        }
        // текст: заглушка
        _ => rect(img, 1, CH / 4, CW - 2, CH / 2, fg),
    }
}
