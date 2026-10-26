//! Графический протокол Kitty
//! (https://sw.kovidgoyal.net/kitty/graphics-protocol/).
//!
//! Обложку передаём один раз на трек (a=t), потом размещаем (a=p) с тем же
//! i и p — повторное размещение заменяет старое без мерцания. CSI 2J по
//! спеке удаляет картинки, поэтому после полной очистки экрана передаём заново.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use std::fmt::Write;

const IMAGE_ID: u32 = 7_727;
const PLACEMENT_ID: u32 = 1;
/// Не больше 4096 байт на кусок, все кроме последнего кратны 4.
const CHUNK: usize = 4096;

pub fn supported() -> bool {
    std::env::var_os("KITTY_WINDOW_ID").is_some()
        || std::env::var("TERM").is_ok_and(|t| t == "xterm-kitty")
}

/// Передать PNG без показа. q=2 — терминал не присылает ответы, иначе они
/// попадут во ввод как нажатия клавиш. В продолжениях только m и q.
pub fn transmit(out: &mut String, png: &[u8]) {
    let data = STANDARD.encode(png);
    let total = data.len().div_ceil(CHUNK);
    for (n, chunk) in data.as_bytes().chunks(CHUNK).enumerate() {
        let more = u8::from(n + 1 < total);
        let chunk = std::str::from_utf8(chunk).expect("base64 — это ASCII");
        if n == 0 {
            let _ = write!(out, "\x1b_Ga=t,f=100,t=d,i={IMAGE_ID},q=2,m={more};{chunk}\x1b\\");
        } else {
            let _ = write!(out, "\x1b_Gm={more},q=2;{chunk}\x1b\\");
        }
    }
}

/// Показать в клетке (row, col), 1-based, на cols×rows клеток. Kitty сам
/// вписывает картинку с сохранением пропорций. C=1 — курсор не двигается.
pub fn place(out: &mut String, row: usize, col: usize, cols: usize, rows: usize) {
    let _ = write!(
        out,
        "\x1b[{row};{col}H\x1b_Ga=p,i={IMAGE_ID},p={PLACEMENT_ID},c={cols},r={rows},C=1,q=2\x1b\\"
    );
}

/// Удалить картинку вместе с данными (d=I — заглавная освобождает память).
pub fn delete(out: &mut String) {
    let _ = write!(out, "\x1b_Ga=d,d=I,i={IMAGE_ID},q=2\x1b\\");
}
