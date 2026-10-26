//! Слушает Strawberry и печатает события: cargo run --release --example mpris_probe -- [секунд]
use std::time::{Duration, Instant};
use coda::color::Rgb;
use coda::mpris::{self, Art, Event};

fn main() {
    let secs: u64 = std::env::args().nth(1).map_or(3, |s| s.parse().unwrap());
    let player = std::env::var("VIS_PLAYER").unwrap_or_else(|_| "strawberry".into());
    let rx = mpris::spawn(&player);
    let end = Instant::now() + Duration::from_secs(secs);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(Event::Track(t)) => {
                let art = match &t.art {
                    Art::Unchanged => "обложка та же".to_string(),
                    Art::Missing => "обложки нет".to_string(),
                    Art::New(c) => {
                        let hex: Vec<String> = c.palette.iter().map(|l| {
                            let Rgb(r, g, b) = l.to_rgb();
                            format!("#{r:02X}{g:02X}{b:02X}")
                        }).collect();
                        format!("PNG {} КБ, палитра {}", c.png.len() / 1024, hex.join(" "))
                    }
                };
                println!("трек: {} — {} [{}] | {art}", t.artist, t.title, t.album);
            }
            Ok(Event::Gone) => println!("плеер закрыт"),
            Err(_) => break,
        }
    }
}
