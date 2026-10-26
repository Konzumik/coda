//! Смена трека в плеере (по умолчанию Strawberry) через MPRIS2
//! (https://specifications.freedesktop.org/mpris-spec/latest/).
//!
//! Слушаем сырые сигналы D-Bus в своём потоке:
//! - PropertiesChanged от /org/mpris/MediaPlayer2 для org.mpris.MediaPlayer2.Player —
//!   по спеке так приходит новый Metadata; отправитель сверяется с владельцем
//!   имени Strawberry (Firefox шлёт сигналы с тем же путём);
//! - NameOwnerChanged для org.mpris.MediaPlayer2.<плеер> — плеер закрыли
//!   или запустили заново.
//! Обложку читаем сразу: Strawberry кладёт её во временный файл и удаляет
//! при смене трека.

use crate::color::Lab;
use crate::palette;
use image::{DynamicImage, ImageFormat, imageops::FilterType};
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;
use zbus::MatchRule;
use zbus::blocking::{Connection, MessageIterator, fdo::DBusProxy};
use zbus::message::Type;
use zbus::names::BusName;
use zbus::zvariant::{OwnedValue, Value};

const OBJECT: &str = "/org/mpris/MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";
/// Размер обложки, которую отдаём Kitty; дальше он масштабирует сам.
const COVER_PX: u32 = 400;

pub struct Cover {
    pub palette: Vec<Lab>,
    pub png: Vec<u8>,
}

pub enum Art {
    /// Тот же artUrl, что и раньше: обложку не трогаем.
    Unchanged,
    /// У трека нет обложки (или не прочиталась).
    Missing,
    New(Arc<Cover>),
}

pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art: Art,
}

pub enum Event {
    Track(Track),
    /// Strawberry закрыли.
    Gone,
}

/// `player` — имя после org.mpris.MediaPlayer2., например "strawberry".
pub fn spawn(player: &str) -> mpsc::Receiver<Event> {
    let (tx, rx) = mpsc::channel();
    let bus_name = format!("org.mpris.MediaPlayer2.{player}");
    thread::spawn(move || {
        loop {
            // ошибка = нет шины или она отвалилась; пробуем переподключиться
            if watch(&tx, &bus_name).is_ok() {
                return; // главный поток закрыл канал
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
    rx
}

/// Ok(()) — получатель закрыт, работать больше незачем.
fn watch(tx: &mpsc::Sender<Event>, player: &str) -> zbus::Result<()> {
    let conn = Connection::session()?;
    let dbus = DBusProxy::new(&conn)?;
    dbus.add_match_rule(
        MatchRule::builder()
            .msg_type(Type::Signal)
            .interface(PROPS_IFACE)?
            .member("PropertiesChanged")?
            .path(OBJECT)?
            .arg(0, PLAYER_IFACE)?
            .build(),
    )?;
    dbus.add_match_rule(
        MatchRule::builder()
            .msg_type(Type::Signal)
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .arg(0, player)?
            .build(),
    )?;

    let mut last_art: Option<String> = None;
    let mut owner = dbus.get_name_owner(BusName::try_from(player)?).ok().map(|o| o.to_string());
    if owner.is_some() {
        if let Some(md) = fetch_metadata(&conn, player) {
            if !send(tx, Event::Track(track(md, &mut last_art))) {
                return Ok(());
            }
        }
    }

    for msg in MessageIterator::from(&conn) {
        let msg = msg?;
        let header = msg.header();
        if header.message_type() != Type::Signal {
            continue;
        }
        let event = match header.member().map(|m| m.as_str()) {
            Some("NameOwnerChanged") => {
                let (name, _old, new): (String, String, String) = msg.body().deserialize()?;
                if name != player {
                    continue;
                }
                last_art = None;
                if new.is_empty() {
                    owner = None;
                    Event::Gone
                } else {
                    owner = Some(new);
                    match fetch_metadata(&conn, player) {
                        Some(md) => Event::Track(track(md, &mut last_art)),
                        None => continue,
                    }
                }
            }
            Some("PropertiesChanged") => {
                let sender = header.sender().map(|s| s.to_string());
                if sender.is_none() || sender != owner {
                    continue;
                }
                let (iface, mut changed, invalidated): (
                    String,
                    HashMap<String, OwnedValue>,
                    Vec<String>,
                ) = msg.body().deserialize()?;
                if iface != PLAYER_IFACE {
                    continue;
                }
                let md = match changed.remove("Metadata") {
                    Some(v) => Some(v),
                    // по спеке свойство может прийти «инвалидированным» без значения
                    None if invalidated.iter().any(|p| p == "Metadata") => fetch_metadata(&conn, player),
                    None => None,
                };
                match md {
                    Some(md) => Event::Track(track(md, &mut last_art)),
                    None => continue,
                }
            }
            _ => continue,
        };
        if !send(tx, event) {
            return Ok(());
        }
    }
    Err(zbus::Error::Failure("поток сообщений D-Bus закрылся".into()))
}

fn send(tx: &mpsc::Sender<Event>, e: Event) -> bool {
    tx.send(e).is_ok()
}

fn fetch_metadata(conn: &Connection, player: &str) -> Option<OwnedValue> {
    let reply = conn
        .call_method(Some(player), OBJECT, Some(PROPS_IFACE), "Get", &(PLAYER_IFACE, "Metadata"))
        .ok()?;
    reply.body().deserialize::<OwnedValue>().ok()
}

fn track(md: OwnedValue, last_art: &mut Option<String>) -> Track {
    let map: HashMap<String, OwnedValue> = HashMap::try_from(md).unwrap_or_default();
    let field = |k: &str| map.get(k).and_then(|v| text(v)).unwrap_or_default();
    let art_url = field("mpris:artUrl");

    let art = if last_art.as_deref() == Some(art_url.as_str()) {
        Art::Unchanged
    } else {
        *last_art = Some(art_url.clone());
        match load_cover(&art_url) {
            Some(c) => Art::New(Arc::new(c)),
            None => Art::Missing,
        }
    };
    Track { title: field("xesam:title"), artist: field("xesam:artist"), album: field("xesam:album"), art }
}

/// Строка или массив строк (xesam:artist по спеке — список) → текст.
fn text(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(p) => Some(p.to_string()),
        Value::Array(a) => {
            let parts: Vec<String> = a.iter().filter_map(text).collect();
            (!parts.is_empty()).then(|| parts.join(", "))
        }
        Value::Value(inner) => text(inner),
        _ => None,
    }
}

fn load_cover(url: &str) -> Option<Cover> {
    let path = url.strip_prefix("file://").map(percent_decode)?;
    // файл может появиться чуть позже сигнала — пара коротких повторов
    let bytes = (0..4).find_map(|i| {
        if i > 0 {
            thread::sleep(Duration::from_millis(60));
        }
        std::fs::read(&path).ok()
    })?;
    let img = image::load_from_memory(&bytes).ok()?;
    let rgb = img.to_rgb8();
    let palette = palette::extract(&rgb);

    let small = DynamicImage::ImageRgb8(rgb).resize(COVER_PX, COVER_PX, FilterType::CatmullRom);
    let mut png = Vec::new();
    small.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).ok()?;
    Some(Cover { palette, png })
}

/// file:// URL → путь: %D0%9C → байты UTF-8 (кириллица в путях).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(hi), Some(lo)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(hi << 4 | lo);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
