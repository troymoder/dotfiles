// SPDX-License-Identifier: GPL-3.0-only

//! The apps by name: PipeWire, MPRIS and /proc give raw names (`spotify`,
//! `chromium-browser`, `libao[speech-dispatcher]`); the known ones get their
//! proper name, Tabler's mark for them and their color, the rest a tidied
//! name and a plain window glyph.

use cosmic::{Element, iced::Color, theme, widget};

#[derive(Clone, Debug, PartialEq)]
pub struct AppInfo {
    pub name: String,
    pub icon: &'static str,
    pub tint: Option<Color>,
}

/// What to look for in the raw name, in order; the first match wins.
const KNOWN: &[(&[&str], &str, &str, Option<Color>)] = &[
    (
        &["spotify"],
        "Spotify",
        "brand-spotify",
        Some(Color::from_rgb8(0x1d, 0xb9, 0x54)),
    ),
    (
        &["spotifast"],
        "Spotifast",
        "brand-spotify",
        Some(Color::from_rgb8(0x1d, 0xb9, 0x54)),
    ),
    (
        &["chromium"],
        "Chromium",
        "brand-chrome",
        Some(Color::from_rgb8(0x8a, 0xb4, 0xf8)),
    ),
    (
        &["google-chrome", "google chrome", "chrome"],
        "Google Chrome",
        "brand-chrome",
        Some(Color::from_rgb8(0x8a, 0xb4, 0xf8)),
    ),
    (
        &["firefox", "librewolf", "zen"],
        "Firefox",
        "brand-firefox",
        Some(Color::from_rgb8(0xff, 0x9a, 0x57)),
    ),
    (
        &["discord", "vesktop", "webcord"],
        "Discord",
        "brand-discord",
        Some(Color::from_rgb8(0x8c, 0x9e, 0xff)),
    ),
    (
        &["steam"],
        "Steam",
        "brand-steam",
        Some(Color::from_rgb8(0xc7, 0xd5, 0xe0)),
    ),
    (
        &["vlc"],
        "VLC",
        "brand-vlc",
        Some(Color::from_rgb8(0xff, 0x9a, 0x3c)),
    ),
    (
        &["zoom"],
        "Zoom",
        "brand-zoom",
        Some(Color::from_rgb8(0x5a, 0xa2, 0xff)),
    ),
    (
        &["slack"],
        "Slack",
        "brand-slack",
        Some(Color::from_rgb8(0xe9, 0xa3, 0xb4)),
    ),
    (
        &["telegram"],
        "Telegram",
        "brand-telegram",
        Some(Color::from_rgb8(0x6c, 0xc4, 0xf0)),
    ),
    (
        &["teams"],
        "Microsoft Teams",
        "brand-teams",
        Some(Color::from_rgb8(0x9e, 0xa6, 0xff)),
    ),
    (
        &["element", "matrix"],
        "Element",
        "brand-matrix",
        Some(Color::from_rgb8(0x5f, 0xd3, 0xa6)),
    ),
    (
        &["twitch"],
        "Twitch",
        "brand-twitch",
        Some(Color::from_rgb8(0xb8, 0x9c, 0xff)),
    ),
    (
        &["speech-dispatcher", "speechd"],
        "Speech Dispatcher",
        "message-chatbot",
        None,
    ),
    (&["mpv"], "mpv", "player-play", None),
    // Windows, as the switcher names them by app id.
    (
        &["vscode", "code-oss", "visual studio code"],
        "Visual Studio Code",
        "brand-vscode",
        Some(Color::from_rgb8(0x5c, 0xb0, 0xf0)),
    ),
    (&["ghostty"], "Ghostty", "terminal-2", None),
    (
        &["cosmicterm", "cosmic-term"],
        "Terminal",
        "terminal-2",
        None,
    ),
    (&["cosmicfiles", "cosmic-files"], "Files", "folder", None),
    (
        &["cosmicsettings", "cosmic-settings"],
        "Settings",
        "settings",
        None,
    ),
    (
        &["cosmicedit", "cosmic-edit"],
        "Text Editor",
        "file-text",
        None,
    ),
    // Our own recorder, in the privacy menu.
    (
        &["cabin-capture"],
        "Screen recording",
        "player-record",
        None,
    ),
    // Programs, as the System panel's busiest list names them.
    (
        &["rustc", "rust-analyzer", "cargo", "clippy-driver"],
        "",
        "brand-rust",
        Some(Color::from_rgb8(0xf2, 0xa0, 0x7b)),
    ),
    (&["cosmic-comp"], "cosmic-comp", "device-desktop", None),
];

/// The app behind a raw name.
pub fn app_info(raw: &str) -> AppInfo {
    let mut lower = raw.to_lowercase();
    // VS Code's app id and program are a bare `code`: too common a word to
    // look for inside other names.
    if lower == "code" {
        lower = "vscode".to_owned();
    }
    // `libao[speech-dispatcher]`: the part in brackets is the app.
    let inner = lower.find('[').and_then(|start| {
        lower[start + 1..]
            .find(']')
            .map(|end| &lower[start + 1..start + 1 + end])
    });
    for (patterns, name, icon, tint) in KNOWN {
        if patterns
            .iter()
            .any(|p| inner.is_some_and(|i| i.contains(p)) || lower.contains(p))
        {
            // No proper name: the program's own (rustc stays rustc).
            let name = if name.is_empty() {
                raw.trim().to_owned()
            } else {
                (*name).to_owned()
            };
            return AppInfo {
                name,
                icon,
                tint: *tint,
            };
        }
    }
    let base = inner
        .map(str::to_owned)
        .unwrap_or_else(|| raw.trim().to_owned())
        .replace(['-', '_'], " ");
    let mut chars = base.chars();
    let name = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "An app".to_owned(),
    };
    AppInfo {
        name,
        icon: "app-window",
        tint: None,
    }
}

/// The app's proper name.
pub fn name(raw: &str) -> String {
    app_info(raw).name
}

/// The app's mark, so many pixels across, in its color (or the text's).
pub fn glyph<'a, M: 'a>(raw: &str, size: u16) -> Element<'a, M> {
    let info = app_info(raw);
    let icon = super::icon(info.icon, size);
    match info.tint {
        Some(tint) => widget::container(icon)
            .class(theme::Container::custom(move |_| {
                widget::container::Style {
                    icon_color: Some(tint),
                    text_color: Some(tint),
                    ..Default::default()
                }
            }))
            .into(),
        None => icon.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_apps() {
        assert_eq!(app_info("spotify").name, "Spotify");
        assert_eq!(
            app_info("org.mpris.MediaPlayer2.spotifast").name,
            "Spotifast"
        );
        assert_eq!(app_info("chromium-browser").icon, "brand-chrome");
        assert_eq!(app_info("Google Chrome").name, "Google Chrome");
        assert_eq!(
            app_info("libao[speech-dispatcher]").name,
            "Speech Dispatcher"
        );
        assert_eq!(app_info("Vesktop").name, "Discord");
        assert_eq!(app_info("code").icon, "brand-vscode");
        assert_eq!(app_info("opencode").icon, "app-window");
        assert_eq!(app_info("CosmicFiles").name, "Files");
    }

    #[test]
    fn unknown_apps_are_tidied() {
        let info = app_info("my_cool-app");
        assert_eq!(info.name, "My cool app");
        assert_eq!(info.icon, "app-window");
        assert_eq!(info.tint, None);
        assert_eq!(app_info("alsa[thing]").name, "Thing");
    }
}
