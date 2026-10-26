use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::sync::Arc;
use std::time::{Duration, Instant};
use visualizer::color::Lab;
use visualizer::mpris::{self, Art, Cover};
use visualizer::palette::{self, Blend, STOPS};
use visualizer::render::grid::Grid;
use visualizer::render::modes::{Kind, Modes};
use visualizer::render::{self, CoverBox, Layout};
use visualizer::{analyzer, audio, kitty, term};

/// 90 fps: на мониторе 180 Гц каждый кадр ровно на два обновления экрана.
const FPS: f64 = 90.0;
/// Трек пришёл без обложки — ждём столько, прежде чем уйти в палитру по
/// умолчанию: плеер может прислать метаданные дважды, сначала без artUrl.
const MISSING_ART_GRACE: Duration = Duration::from_millis(600);

fn main() {
    if let Err(e) = run() {
        eprintln!("visualizer: {e}");
        std::process::exit(1);
    }
}

#[derive(Default)]
struct NowPlaying {
    title: String,
    artist: String,
    album: String,
    cover: Option<Arc<Cover>>,
}

/// Ширина клетки / высота клетки в пикселях (Kitty сообщает размер окна в px).
fn cell_aspect() -> f32 {
    match crossterm::terminal::window_size() {
        Ok(w) if w.width > 0 && w.height > 0 && w.columns > 0 && w.rows > 0 => {
            (w.width as f32 / w.columns as f32) / (w.height as f32 / w.rows as f32)
        }
        _ => 0.5,
    }
}

/// Цвет текста из палитры, но не темнее `min_l`, чтобы читался на тёмном фоне.
fn text_color(c: Lab, min_l: f32) -> visualizer::color::Rgb {
    Lab { l: c.l.max(min_l), ..c }.into_gamut().to_rgb()
}

fn run() -> std::io::Result<()> {
    let mut audio = audio::Capture::start()?;
    let player = std::env::var("VIS_PLAYER").unwrap_or_else(|_| "strawberry".into());
    let media = mpris::spawn(&player);
    let graphics = kitty::supported();
    let mut term = term::Term::enter()?;

    let (cols, rows) = crossterm::terminal::size()?;
    let mut layout = Layout::new(cols, rows, cell_aspect());
    let mut analyzer = analyzer::Analyzer::new(audio::SAMPLE_RATE, layout.bars);
    let mut blend = Blend::new(&palette::fallback());
    let mut modes = Modes::new(Kind::Bars);
    let mut grid = Grid::new(layout.cols, layout.area_h);
    let mut now_playing = NowPlaying::default();
    let mut missing_since: Option<Instant> = None;
    // где обложка стоит сейчас; None — на экране её нет
    let mut shown: Option<CoverBox> = None;
    let mut need_transmit = false;

    let mut samples = vec![0.0f32; analyzer::FFT_SIZE];
    let mut frame = String::with_capacity(64 * 1024);
    let mut gfx = String::new();
    let frame_dt = Duration::from_secs_f64(1.0 / FPS);
    let mut last = Instant::now();
    let mut deadline = last + frame_dt;
    let mut fps = FPS as f32;
    let mut clear = true;

    loop {
        // ждём следующий кадр по ровной сетке, по пути обрабатывая клавиши и ресайз
        while let Some(timeout) = deadline.checked_duration_since(Instant::now()) {
            if !event::poll(timeout)? {
                break;
            }
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    KeyCode::Char(c) => {
                        if let Some(kind) = Kind::from_key(c) {
                            modes.select(kind);
                        }
                    }
                    _ => {}
                },
                Event::Resize(c, r) => {
                    layout = Layout::new(c, r, cell_aspect());
                    analyzer.set_bands(layout.bars);
                    grid.resize(layout.cols, layout.area_h);
                    clear = true;
                }
                _ => {}
            }
        }

        let now = Instant::now();
        let dt = (now - last).as_secs_f32().min(0.1);
        last = now;
        deadline += frame_dt;
        if deadline < now {
            deadline = now + frame_dt; // отстали (система подвисла) — не догоняем пачкой
        }
        fps += (1.0 / dt.max(1e-3) - fps) * 0.05;

        // плеер: новый трек, обложка, закрытие
        while let Ok(ev) = media.try_recv() {
            match ev {
                mpris::Event::Track(t) => {
                    now_playing.title = t.title;
                    now_playing.artist = t.artist;
                    now_playing.album = t.album;
                    match t.art {
                        Art::Unchanged => {}
                        Art::New(cover) => {
                            blend.set_target(&cover.palette);
                            now_playing.cover = Some(cover);
                            need_transmit = true;
                            missing_since = None;
                        }
                        Art::Missing => missing_since = Some(now),
                    }
                }
                mpris::Event::Gone => {
                    now_playing = NowPlaying::default();
                    blend.set_target(&palette::fallback());
                    missing_since = None;
                }
            }
        }
        if missing_since.is_some_and(|t| now - t >= MISSING_ART_GRACE) {
            missing_since = None;
            now_playing.cover = None;
            blend.set_target(&palette::fallback());
        }
        blend.update(dt);

        // обложка через Kitty: передать, поставить, убрать
        gfx.clear();
        if clear {
            shown = None; // CSI 2J стирает картинки
        }
        let live = audio.read(&mut samples, dt);
        analyzer.process(&samples, live, dt);
        let gradient = blend.gradient();
        let scene = layout.scene(&analyzer.levels, &analyzer.peaks, &gradient, dt);
        let bright = modes.draw(&mut grid, &scene);

        // в «Кольце» обложка в центре, в остальных — в шапке
        let want = if graphics && now_playing.cover.is_some() {
            layout.cover_for(modes.visible())
        } else {
            None
        };
        match (want, &now_playing.cover) {
            (Some(b), Some(cover)) => {
                if need_transmit || clear {
                    kitty::delete(&mut gfx);
                    kitty::transmit(&mut gfx, &cover.png);
                    need_transmit = false;
                    shown = None;
                }
                if shown != Some(b) {
                    kitty::place(&mut gfx, b.row, b.col, b.cols, b.rows);
                    shown = Some(b);
                }
            }
            _ => {
                if shown.take().is_some() {
                    kitty::delete(&mut gfx);
                }
            }
        }

        let stops = blend.current();
        let hud = render::Hud {
            title: &now_playing.title,
            artist: &now_playing.artist,
            album: &now_playing.album,
            cover_shown: shown.is_some() && shown == layout.cover,
            title_c: text_color(stops[STOPS - 1], 0.8),
            artist_c: text_color(stops[STOPS / 2], 0.68),
        };
        let status = render::Status { live, fps, ceiling_db: analyzer.ceiling_db };
        render::frame(&mut frame, &layout, &grid, bright, clear, &gfx, &hud, &status);
        term.present(&frame)?;
        clear = false;
    }
}
