// SPDX-License-Identifier: GPL-3.0-only

//! Text as the design sets it: Cantarell, 400 for body and 600 for titles,
//! in its sizes.

use cosmic::{
    iced::{
        Font,
        font::{Family, Weight},
    },
    widget,
};

pub type Text<'a> = widget::Text<'a, cosmic::Theme, cosmic::Renderer>;

pub const FAMILY: &str = "Cantarell";
pub const REGULAR: Font = Font { family: Family::Name(FAMILY), weight: Weight::Normal, ..Font::DEFAULT };
pub const SEMIBOLD: Font = Font { family: Family::Name(FAMILY), weight: Weight::Semibold, ..Font::DEFAULT };

/// Where the font file is, for loading it ourselves: fontconfig may not
/// know the Nix store.
pub fn file() -> Option<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("QS_FONT_DIR") {
        dirs.push(dir.into());
    }
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(format!("{home}/.local/share/fonts").into());
        dirs.push(format!("{home}/.nix-profile/share/fonts/opentype").into());
    }
    for dir in std::env::var("XDG_DATA_DIRS").unwrap_or_default().split(':') {
        dirs.push(format!("{dir}/fonts/opentype").into());
        dirs.push(format!("{dir}/fonts/truetype").into());
    }
    dirs.push("/run/current-system/sw/share/fonts/opentype".into());
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if name.starts_with("cantarell") && (name.ends_with(".otf") || name.ends_with(".ttf")) {
                return Some(path);
            }
            if path.is_dir() {
                if let Ok(inner) = std::fs::read_dir(&path) {
                    for entry in inner.flatten() {
                        let path = entry.path();
                        let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                        if name.starts_with("cantarell") && (name.ends_with(".otf") || name.ends_with(".ttf")) {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }
    None
}

/// The font loaded into the renderer, where it's found.
pub fn load_font() -> cosmic::Task<()> {
    match file().and_then(|path| std::fs::read(path).ok()) {
        Some(bytes) => cosmic::iced::font::load(std::borrow::Cow::Owned(bytes)).map(|result| {
            if let Err(err) = result {
                tracing::warn!("cannot load the font: {err:?}");
            }
        }),
        None => {
            tracing::warn!("Cantarell not found; set QS_FONT_DIR");
            cosmic::Task::none()
        }
    }
}

fn text<'a>(content: impl Into<String>, size: f32, font: Font) -> Text<'a> {
    widget::text(content.into()).size(size).font(font)
}

/// 13 px, regular: most things.
pub fn body<'a>(content: impl Into<String>) -> Text<'a> {
    text(content, 13.0, REGULAR)
}

/// 13 px, semibold: a tile's name, a notification's title.
pub fn title<'a>(content: impl Into<String>) -> Text<'a> {
    text(content, 13.0, SEMIBOLD)
}

/// 11 px: a tile's status line.
pub fn sub<'a>(content: impl Into<String>) -> Text<'a> {
    text(content, 11.0, REGULAR)
}

/// 12 px: labels, times, notification headers.
pub fn small<'a>(content: impl Into<String>) -> Text<'a> {
    text(content, 12.0, REGULAR)
}

/// 13.5 px: a menu row.
pub fn row<'a>(content: impl Into<String>) -> Text<'a> {
    text(content, 13.5, REGULAR)
}

/// 11.5 px: a menu row's second line.
pub fn row_sub<'a>(content: impl Into<String>) -> Text<'a> {
    text(content, 11.5, REGULAR)
}

/// 14 px semibold: a menu's heading.
pub fn heading<'a>(content: impl Into<String>) -> Text<'a> {
    text(content, 14.0, SEMIBOLD)
}

/// 15 px semibold: a player's title.
pub fn big<'a>(content: impl Into<String>) -> Text<'a> {
    text(content, 15.0, SEMIBOLD)
}

/// Any size, regular.
pub fn sized<'a>(content: impl Into<String>, size: f32) -> Text<'a> {
    text(content, size, REGULAR)
}

/// Any size, semibold.
pub fn sized_bold<'a>(content: impl Into<String>, size: f32) -> Text<'a> {
    text(content, size, SEMIBOLD)
}
