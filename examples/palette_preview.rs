//! Контактный лист: обложка → найденные цвета → градиент полос.
//! cargo run --release --example palette_preview -- out.png обложка1.jpg обложка2.jpg …

use image::{Rgb as Px, RgbImage, imageops::FilterType};
use coda::color::Rgb;
use coda::palette;

const ROW: u32 = 120;
const SWATCH: u32 = 90;
const GAP: u32 = 12;
const GRAD_W: u32 = 360;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("куда сохранить png");
    let paths: Vec<String> = args.collect();
    let cols = 2u32;
    let rows = (paths.len() as u32).div_ceil(cols);
    let cell_w = ROW + GAP + 5 * SWATCH + GAP + GRAD_W + GAP * 3;
    let mut sheet = RgbImage::from_pixel(cell_w * cols, (ROW + GAP) * rows + GAP, Px([12, 12, 16]));

    for (n, path) in paths.iter().enumerate() {
        let img = image::open(path).expect(path).to_rgb8();
        let colors = palette::extract(&img);
        let hex: Vec<String> = colors
            .iter()
            .map(|c| {
                let Rgb(r, g, b) = c.to_rgb();
                format!("#{r:02X}{g:02X}{b:02X} (L {:.2} C {:.3})", c.l, c.chroma())
            })
            .collect();
        println!("{}: {}", path.rsplit('/').next().unwrap(), hex.join("  "));

        let (x0, y0) = ((n as u32 % cols) * cell_w + GAP, (n as u32 / cols) * (ROW + GAP) + GAP);
        let thumb = image::imageops::resize(&img, ROW, ROW, FilterType::Triangle);
        image::imageops::overlay(&mut sheet, &thumb, x0 as i64, y0 as i64);

        for (i, c) in colors.iter().enumerate() {
            let Rgb(r, g, b) = c.to_rgb();
            let x = x0 + ROW + GAP + i as u32 * SWATCH;
            for yy in 0..ROW {
                for xx in 2..SWATCH - 2 {
                    sheet.put_pixel(x + xx, y0 + yy, Px([r, g, b]));
                }
            }
        }

        // как это выглядит полосами: градиент снизу вверх, столбики с зазором
        let blend = palette::Blend::new(&colors);
        let grad = blend.gradient();
        let gx = x0 + ROW + GAP + 5 * SWATCH + GAP;
        for xx in 0..GRAD_W {
            if xx % 12 >= 9 {
                continue;
            }
            let h = (ROW as f32 * (0.35 + 0.65 * ((xx as f32 * 0.05).sin() * 0.5 + 0.5))) as u32;
            for yy in 0..h {
                let Rgb(r, g, b) = grad.lab(yy as f32 / ROW as f32).to_rgb();
                sheet.put_pixel(gx + xx, y0 + ROW - 1 - yy, Px([r, g, b]));
            }
        }
    }
    sheet.save(&out).expect("сохраняю");
    println!("→ {out}");
}
