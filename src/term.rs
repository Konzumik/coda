//! Терминал: альтернативный экран, raw-режим, гарантированное восстановление
//! (в том числе при панике).

use crossterm::terminal;
use std::io::{self, Write};

pub struct Term;

impl Term {
    pub fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            hook(info);
        }));
        // альт. экран, спрятать курсор, выключить автоперенос строк
        write_out(b"\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[2J")?;
        Ok(Term)
    }

    pub fn present(&mut self, frame: &str) -> io::Result<()> {
        write_out(frame.as_bytes())
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        restore();
    }
}

fn write_out(bytes: &[u8]) -> io::Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(bytes)?;
    out.flush()
}

fn restore() {
    let _ = write_out(b"\x1b[?2026l\x1b[0m\x1b[?7h\x1b[?25h\x1b[?1049l");
    let _ = terminal::disable_raw_mode();
}
