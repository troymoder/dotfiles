// SPDX-License-Identifier: GPL-3.0-only

//! What's playing, through MPRIS: every player on the bus, with its cover
//! and controls.

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    path::PathBuf,
    time::{Duration, Instant},
};

use cosmic::iced::Subscription;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};

/// Longer than this and it's no track: Chromium gives a live stream the
/// largest length there is.
const LONGEST_TRACK: i64 = 7 * 24 * 3600 * 1_000_000;

#[derive(Clone, Debug)]
pub struct Media {
    /// The bus name, to send controls to.
    pub service: String,
    /// The player's name: "Spotify", "Chromium".
    pub app: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Whether it shuffles, where the player says.
    pub shuffle: Option<bool>,
    /// "None", "Track" or "Playlist", where the player says.
    pub loop_status: Option<String>,
    pub can_raise: bool,
    pub art: Option<PathBuf>,
    /// The cover blurred wide, to lie behind the card.
    pub backdrop: Option<PathBuf>,
    /// The player's own icon, by name.
    pub icon: Option<String>,
    pub color: Option<[u8; 3]>,
    pub playing: bool,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_seek: bool,
    pub track: Option<OwnedObjectPath>,
    /// Microseconds.
    pub length: i64,
    /// Microseconds, as of `at`.
    pub position: i64,
    pub at: Instant,
}

impl PartialEq for Media {
    /// The same but for where playback is, which moves on its own.
    fn eq(&self, other: &Self) -> bool {
        self.service == other.service
            && self.app == other.app
            && self.title == other.title
            && self.artist == other.artist
            && self.album == other.album
            && self.shuffle == other.shuffle
            && self.loop_status == other.loop_status
            && self.art == other.art
            && self.backdrop == other.backdrop
            && self.playing == other.playing
            && self.can_next == other.can_next
            && self.can_previous == other.can_previous
            && self.can_seek == other.can_seek
            && self.track == other.track
            && self.length == other.length
            && (self.position - other.position).abs() < 3_000_000
    }
}

impl Media {
    /// Where playback is now, counting from when it was read.
    pub fn position_now(&self) -> i64 {
        let elapsed = if self.playing {
            self.at.elapsed().as_micros() as i64
        } else {
            0
        };
        let now = self.position.max(0) + elapsed;
        if self.is_live() {
            now
        } else {
            now.min(self.length)
        }
    }

    /// A stream with no end to seek in: no length, or an absurd one.
    pub fn is_live(&self) -> bool {
        self.length <= 0 || self.length > LONGEST_TRACK
    }

    /// Whether a seek bar makes sense: a track, and a player that seeks.
    pub fn seekable(&self) -> bool {
        self.can_seek && !self.is_live()
    }
}

#[zbus::proxy(
    interface = "org.mpris.MediaPlayer2.Player",
    default_path = "/org/mpris/MediaPlayer2"
)]
trait Player {
    fn play_pause(&self) -> zbus::Result<()>;
    fn next(&self) -> zbus::Result<()>;
    fn previous(&self) -> zbus::Result<()>;
    fn set_position(&self, track: &ObjectPath<'_>, position: i64) -> zbus::Result<()>;
    fn seek(&self, offset: i64) -> zbus::Result<()>;
    #[zbus(property)]
    fn can_seek(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn shuffle(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn set_shuffle(&self, shuffle: bool) -> zbus::Result<()>;
    #[zbus(property)]
    fn loop_status(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn set_loop_status(&self, status: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn position(&self) -> zbus::Result<i64>;
    #[zbus(property)]
    fn playback_status(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn metadata(&self) -> zbus::Result<HashMap<String, OwnedValue>>;
    #[zbus(property)]
    fn can_go_next(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn can_go_previous(&self) -> zbus::Result<bool>;
}

#[zbus::proxy(
    interface = "org.mpris.MediaPlayer2",
    default_path = "/org/mpris/MediaPlayer2"
)]
trait MediaPlayer {
    #[zbus(property)]
    fn identity(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn can_raise(&self) -> zbus::Result<bool>;
    fn raise(&self) -> zbus::Result<()>;
}

async fn player(conn: &zbus::Connection, service: &str) -> zbus::Result<PlayerProxy<'static>> {
    PlayerProxy::builder(conn)
        .destination(service.to_owned())?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

fn text(metadata: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    let value = metadata.get(key)?;
    String::try_from(value.clone()).ok().or_else(|| {
        Vec::<String>::try_from(value.clone())
            .ok()
            .map(|v| v.join(", "))
    })
}

fn cache_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .map(|dir| dir.join(crate::APP_ID).join("covers"))
}

/// A square local copy of the cover: players give a `file://` path or a web
/// address; browsers a video's wide thumbnail, the art in its middle.
async fn cover(url: &str) -> Option<PathBuf> {
    let dir = cache_dir()?;
    let local = url.strip_prefix("file://").map(PathBuf::from);
    if local.is_none() && !url.starts_with("http") {
        return None;
    }
    let mut hasher = std::hash::DefaultHasher::new();
    url.hash(&mut hasher);
    // Browsers write each cover over the same file.
    if let Some(path) = &local {
        let modified = tokio::fs::metadata(path).await.ok()?.modified().ok()?;
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs()
            .hash(&mut hasher);
    }
    let square = dir.join(format!("{:016x}.png", hasher.finish()));
    if square.exists() {
        return Some(square);
    }
    tokio::fs::create_dir_all(&dir).await.ok()?;
    let bytes = match &local {
        Some(path) => tokio::fs::read(path).await.ok()?,
        None => {
            let output = tokio::process::Command::new("curl")
                .args(["-sfL", "--max-time", "5", url])
                .output()
                .await
                .ok()?;
            if !output.status.success() {
                return None;
            }
            output.stdout
        }
    };
    let saved = square.clone();
    let cropped = tokio::task::spawn_blocking(move || {
        let image = image::load_from_memory(&bytes)?;
        let side = image.width().min(image.height());
        image
            .crop_imm(
                (image.width() - side) / 2,
                (image.height() - side) / 2,
                side,
                side,
            )
            .thumbnail(256, 256)
            .save(&saved)?;
        Ok::<(), image::ImageError>(())
    })
    .await;
    match cropped {
        Ok(Ok(())) => Some(square),
        _ => local,
    }
}

/// The card's whole background, kept next to the cover: the cover blurred
/// (a tiny copy blown back up) and darkened, fading from the left into the
/// card's color by the right, where the controls are. All one picture, so
/// its rounded edge is one edge.
async fn backdrop(cover: &std::path::Path, _tint: Option<[u8; 3]>) -> Option<PathBuf> {
    let baked = cover.with_extension("card3.png");
    if baked.exists() {
        return Some(baked);
    }
    let (from, to) = (cover.to_owned(), baked.clone());
    tokio::task::spawn_blocking(move || {
        // The design: the cover scaled to cover a box a little larger than the
        // card, blurred wide, made more vivid, and laid at half strength over
        // the card's dark base. Even all over: no fade.
        let image = image::open(&from).ok()?;
        let (width, height) = (400u32, 120u32);
        let (small_w, small_h, pad) = (width / 4, height / 4, 8u32);
        let small = image
            .resize_to_fill(
                small_w + pad * 2,
                small_h + pad * 2,
                image::imageops::FilterType::Triangle,
            )
            .to_rgba8();
        let mut blurred = image::imageops::blur(&small, 6.5);
        let mut cropped =
            image::imageops::crop(&mut blurred, pad, pad, small_w, small_h).to_image();
        const DARK: [f32; 3] = [8.0, 12.0, 26.0];
        const ART_MIX: f32 = 0.5;
        for pixel in cropped.pixels_mut() {
            let [r, g, b, _] = pixel.0.map(f32::from);
            let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let vivid = |c: f32| (luma + (c - luma) * 1.3).clamp(0.0, 255.0);
            let mix = |c: f32, d: f32| (d + (vivid(c) - d) * ART_MIX).round() as u8;
            pixel.0 = [mix(r, DARK[0]), mix(g, DARK[1]), mix(b, DARK[2]), 255];
        }
        let full = image::imageops::resize(
            &cropped,
            width,
            height,
            image::imageops::FilterType::CatmullRom,
        );
        full.save(&to).ok()
    })
    .await
    .ok()??;
    Some(baked)
}

/// A cover's color: its pixels averaged, the more colorful counting more.
/// Kept next to the cover once worked out.
async fn cover_color(path: &std::path::Path) -> Option<[u8; 3]> {
    let kept = path.with_extension("rgb");
    if let Ok(bytes) = tokio::fs::read(&kept).await {
        if let [r, g, b] = bytes[..] {
            return Some([r, g, b]);
        }
    }
    let path = path.to_owned();
    let color = tokio::task::spawn_blocking(move || {
        let image = image::open(&path).ok()?.thumbnail(24, 24).to_rgb8();
        let (mut sum, mut weights) = ([0.0f32; 3], 0.0f32);
        for pixel in image.pixels() {
            let [r, g, b] = pixel.0.map(f32::from);
            let saturation = (r.max(g).max(b) - r.min(g).min(b)) / 255.0;
            let weight = 0.05 + saturation * saturation;
            sum = [
                sum[0] + r * weight,
                sum[1] + g * weight,
                sum[2] + b * weight,
            ];
            weights += weight;
        }
        (weights > 0.0).then(|| sum.map(|channel| (channel / weights).round() as u8))
    })
    .await
    .ok()??;
    if cache_dir().is_some_and(|dir| kept.starts_with(dir)) {
        _ = tokio::fs::write(&kept, color).await;
    }
    Some(color)
}

async fn read(conn: &zbus::Connection, service: &str) -> zbus::Result<Media> {
    let proxy = player(conn, service).await?;
    let metadata = proxy.metadata().await.unwrap_or_default();
    let art = match text(&metadata, "mpris:artUrl") {
        Some(url) => cover(&url).await,
        None => None,
    };
    let color = match &art {
        Some(path) => cover_color(path).await,
        None => None,
    };
    let backdrop = match &art {
        Some(path) if cache_dir().is_some_and(|dir| path.starts_with(dir)) => {
            backdrop(path, color).await
        }
        _ => None,
    };
    let root = MediaPlayerProxy::builder(conn)
        .destination(service.to_owned())?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await?;
    let app = root.identity().await.unwrap_or_else(|_| {
        service
            .trim_start_matches("org.mpris.MediaPlayer2.")
            .to_owned()
    });
    let can_raise = root.can_raise().await.unwrap_or(false);
    let icon = super::apps::icon(
        &app,
        Some(
            service
                .trim_start_matches("org.mpris.MediaPlayer2.")
                .split('.')
                .next()
                .unwrap_or_default(),
        ),
    );
    Ok(Media {
        service: service.to_owned(),
        app,
        icon,
        title: text(&metadata, "xesam:title").unwrap_or_default(),
        artist: text(&metadata, "xesam:artist").unwrap_or_default(),
        album: text(&metadata, "xesam:album").unwrap_or_default(),
        shuffle: proxy.shuffle().await.ok(),
        loop_status: proxy.loop_status().await.ok(),
        can_raise,
        art,
        backdrop,
        color,
        playing: proxy.playback_status().await? == "Playing",
        can_next: proxy.can_go_next().await.unwrap_or(false),
        can_previous: proxy.can_go_previous().await.unwrap_or(false),
        can_seek: proxy.can_seek().await.unwrap_or(false),
        // Spotify sends the track id as a string, not an object path.
        track: metadata.get("mpris:trackid").and_then(|v| {
            OwnedObjectPath::try_from(v.clone()).ok().or_else(|| {
                String::try_from(v.clone())
                    .ok()
                    .and_then(|s| OwnedObjectPath::try_from(s).ok())
            })
        }),
        length: metadata
            .get("mpris:length")
            .and_then(|v| {
                i64::try_from(v.clone())
                    .ok()
                    .or_else(|| u64::try_from(v.clone()).ok().map(|l| l as i64))
            })
            .unwrap_or(0),
        position: proxy.position().await.unwrap_or(0),
        at: Instant::now(),
    })
}

/// Every player on the bus, in the bus's order; no player, no card.
async fn players(conn: &zbus::Connection) -> Vec<Media> {
    let Ok(names) = zbus::fdo::DBusProxy::new(conn).await else {
        return Vec::new();
    };
    let Ok(names) = names.list_names().await else {
        return Vec::new();
    };
    let mut players = Vec::new();
    for name in names {
        let name = name.as_str();
        if !name.starts_with("org.mpris.MediaPlayer2.") || name.contains("playerctld") {
            continue;
        }
        match read(conn, name).await {
            Ok(media) if !media.title.is_empty() || media.playing => players.push(media),
            Ok(_) => {}
            Err(err) => tracing::debug!("cannot read {name}: {err}"),
        }
    }
    players
}

pub async fn now() -> Option<Vec<Media>> {
    let conn = zbus::Connection::session().await.ok()?;
    Some(players(&conn).await)
}

pub fn subscription() -> Subscription<Vec<Media>> {
    fn rules() -> Vec<zbus::MatchRule<'static>> {
        let path = "/org/mpris/MediaPlayer2";
        [
            super::signal(
                None,
                Some(path),
                Some("org.freedesktop.DBus.Properties"),
                Some("PropertiesChanged"),
            ),
            // A jump in what's playing, which changes no property.
            super::signal(
                None,
                Some(path),
                Some("org.mpris.MediaPlayer2.Player"),
                Some("Seeked"),
            ),
            // Players come and go; only theirs, or the bus's chatter would
            // hold up the rest.
            zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender("org.freedesktop.DBus")
                .ok()
                .and_then(|b| b.member("NameOwnerChanged").ok())
                .and_then(|b| b.arg0ns("org.mpris.MediaPlayer2").ok())
                .map(|b| b.build()),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
    Subscription::run(|| {
        super::watch(
            Duration::from_secs(15),
            || super::bus_changes(false, rules),
            now,
        )
    })
}

/// Playback moved to a position: by the track's id when there is one, else
/// relative to where the player says it is.
pub async fn seek(
    service: String,
    track: Option<OwnedObjectPath>,
    position: i64,
) -> zbus::Result<()> {
    let conn = zbus::Connection::session().await?;
    let player = player(&conn, &service).await?;
    match track {
        Some(track) => player.set_position(&track, position).await,
        None => {
            let now = player.position().await?;
            player.seek(position - now).await
        }
    }
}

/// The player's window brought up.
pub async fn raise(service: String) -> zbus::Result<()> {
    let conn = zbus::Connection::session().await?;
    MediaPlayerProxy::builder(&conn)
        .destination(service)?
        .build()
        .await?
        .raise()
        .await
}

pub async fn set_shuffle(service: String, shuffle: bool) -> zbus::Result<()> {
    let conn = zbus::Connection::session().await?;
    player(&conn, &service).await?.set_shuffle(shuffle).await
}

pub async fn set_loop(service: String, status: String) -> zbus::Result<()> {
    let conn = zbus::Connection::session().await?;
    player(&conn, &service)
        .await?
        .set_loop_status(&status)
        .await
}

/// Microseconds as "3:28", or "1:02:05" past an hour.
pub fn time(micros: i64) -> String {
    let seconds = (micros.max(0) / 1_000_000) as u64;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Control {
    PlayPause,
    Next,
    Previous,
}

pub async fn control(service: String, control: Control) -> zbus::Result<()> {
    let conn = zbus::Connection::session().await?;
    let proxy = player(&conn, &service).await?;
    match control {
        Control::PlayPause => proxy.play_pause().await,
        Control::Next => proxy.next().await,
        Control::Previous => proxy.previous().await,
    }
}
