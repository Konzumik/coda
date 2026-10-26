//! Захват системного звука: parec пишет float32 стерео с монитора
//! дефолтного выхода, отдельный поток складывает моно в кольцевой буфер.

use std::io::{self, Read};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const SAMPLE_RATE: u32 = 48_000;
const RING_LEN: usize = 16_384;
/// Если данных нет дольше этого, считаем что тишина (пауза, монитор усыплён).
const STALE_AFTER: Duration = Duration::from_millis(150);
const FRAME_BYTES: usize = 8; // 2 канала × f32
/// Насколько головка чтения отстаёт от последнего пришедшего сэмпла.
/// PipeWire отдаёт звук квантами по 1024, головке нужен запас, чтобы не упереться.
const PLAYHEAD_LAG: u64 = 1024;
/// Если разошлись сильнее (лаг системы, пауза), прыгаем сразу.
const PLAYHEAD_RESYNC: f64 = 4096.0;

/// Звук приходит пачками неравномерно, а кадры рисуются ровно. Головка
/// двигается со скоростью реального времени и мягко подтягивается к данным,
/// поэтому окно FFT каждый кадр сдвигается на одинаковое число сэмплов.
#[derive(Default)]
pub struct Playhead {
    pos: Option<f64>,
}

impl Playhead {
    /// `written` — сколько сэмплов пришло всего. Возвращает конец окна анализа.
    pub fn advance(&mut self, written: u64, dt: f32) -> u64 {
        let target = written.saturating_sub(PLAYHEAD_LAG) as f64;
        let pos = match self.pos {
            Some(p) if (p - target).abs() < PLAYHEAD_RESYNC => {
                let p = p + dt as f64 * SAMPLE_RATE as f64;
                p + (target - p) * 0.05
            }
            _ => target,
        };
        self.pos = Some(pos);
        (pos.max(0.0) as u64).min(written)
    }

    pub fn reset(&mut self) {
        self.pos = None;
    }
}

struct Ring {
    data: Vec<f32>,
    written: u64,
    last_write: Option<Instant>,
}

pub struct Capture {
    child: Child,
    ring: Arc<Mutex<Ring>>,
    playhead: Playhead,
}

impl Capture {
    pub fn start() -> io::Result<Self> {
        let rate = format!("--rate={SAMPLE_RATE}");
        let mut child = Command::new("parec")
            .args([
                "--device=@DEFAULT_MONITOR@",
                "--format=float32le",
                &rate,
                "--channels=2",
                "--raw",
                // без этого parec копит буфер и картинка запаздывает
                "--latency-msec=10",
                "--client-name=visualizer",
                "--stream-name=spectrum",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| io::Error::new(e.kind(), format!("не удалось запустить parec: {e}")))?;

        let mut out = child.stdout.take().expect("stdout piped");
        let ring = Arc::new(Mutex::new(Ring {
            data: vec![0.0; RING_LEN],
            written: 0,
            last_write: None,
        }));

        let writer = Arc::clone(&ring);
        thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            let mut pending: Vec<u8> = Vec::with_capacity(chunk.len() + FRAME_BYTES);
            loop {
                let n = match out.read(&mut chunk) {
                    Ok(0) | Err(_) => break, // parec умер или закрыл поток
                    Ok(n) => n,
                };
                pending.extend_from_slice(&chunk[..n]);
                let whole = pending.len() / FRAME_BYTES * FRAME_BYTES;

                let mut r = writer.lock().unwrap();
                for f in pending[..whole].chunks_exact(FRAME_BYTES) {
                    let l = f32::from_le_bytes([f[0], f[1], f[2], f[3]]);
                    let rr = f32::from_le_bytes([f[4], f[5], f[6], f[7]]);
                    let i = (r.written % RING_LEN as u64) as usize;
                    r.data[i] = (l + rr) * 0.5;
                    r.written += 1;
                }
                r.last_write = Some(Instant::now());
                drop(r);

                pending.drain(..whole);
            }
        });

        Ok(Self { child, ring, playhead: Playhead::default() })
    }

    /// Заполняет `out` окном, которое кончается на головке чтения.
    /// Возвращает false, если звука нет.
    pub fn read(&mut self, out: &mut [f32], dt: f32) -> bool {
        let r = self.ring.lock().unwrap();
        let live = r.last_write.is_some_and(|t| t.elapsed() < STALE_AFTER);
        let n = out.len();
        if !live || r.written < n as u64 {
            self.playhead.reset();
            return false;
        }
        let end = self.playhead.advance(r.written, dt).max(n as u64);
        let start = end - n as u64;
        for (i, s) in out.iter_mut().enumerate() {
            *s = r.data[((start + i as u64) % RING_LEN as u64) as usize];
        }
        true
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
