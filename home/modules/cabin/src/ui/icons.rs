// SPDX-License-Identifier: GPL-3.0-only

//! The icons: Tabler Icons, the outline set, embedded from the pack at
//! build time by name (`TABLER_ICONS_DIR`), drawn in the current color. The
//! names the code grew up with map onto them.

use cosmic::widget;

macro_rules! tabler {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!(env!("TABLER_ICONS_DIR"), "/", $name, ".svg")) as &[u8])),*]
    };
}

const ICONS: &[(&str, &[u8])] = tabler![
    "activity", "arrows-shuffle", "battery", "battery-1", "battery-2", "battery-3", "battery-4", "battery-charging",
    "bell-off", "bluetooth", "bolt", "box", "brand-chrome", "brand-firefox", "brand-spotify", "camera", "check",
    "chevron-down", "chevron-left", "chevron-right", "chevron-up", "cloud", "cloud-rain", "cloud-snow", "cloud-storm",
    "cloud-fog", "device-desktop", "download", "eye", "eye-off", "gauge", "heart", "keyboard", "leaf", "lock", "logout",
    "map-pin", "message-circle", "microphone", "microphone-off", "moon", "mouse", "music", "network", "package", "photo",
    "plane", "player-pause", "player-play", "player-skip-back", "player-skip-forward", "plug-connected", "power",
    "refresh", "repeat", "repeat-once", "rocket", "scale", "server", "settings", "sun", "sunset", "sunrise", "volume",
    "volume-2", "volume-off", "wifi", "wifi-0", "wifi-1", "wifi-2", "wifi-off", "wind", "x", "headphones", "device-speaker",
    "device-gamepad", "device-ipad", "device-mobile", "headset",
    // The known apps' marks, and the rest of the design's glyphs.
    "brand-discord", "brand-steam", "brand-vlc", "brand-zoom", "brand-slack", "brand-telegram", "brand-teams",
    "brand-matrix", "brand-twitch", "message-chatbot", "app-window", "cpu", "cpu-2", "stack-2", "database",
    "arrows-down-up", "propeller", "device-desktop-analytics", "shield-lock", "shield-check", "screen-share",
    "player-stop", "usb", "video", "user", "info-circle", "contrast", "focus-2", "exposure", "temperature",
    "brightness-half", "layout-grid", "terminal-2", "folder", "camera-off", "adjustments", "plug", "aspect-ratio", "trash", "bell", "key",
    // Tabler 3.51: the dedicated and the integrated GPU.
    "gpu", "gpu-2", "battery-eco",
    // Panels: the pin, the System panel's folding and warnings, programs.
    "pin", "pinned", "arrows-diagonal", "arrows-diagonal-minimize-2", "alert-triangle", "brand-rust", "terminal",
];

/// Icons Tabler doesn't draw, made in its style (24 grid, 2 px round
/// strokes, currentColor), kept in `data/icons`.
const OWN: &[(&str, &[u8])] = &[("ethernet", include_bytes!("../../data/icons/ethernet.svg"))];

/// The SVG for a Tabler or own icon name.
fn svg(name: &str) -> Option<&'static [u8]> {
    ICONS.iter().chain(OWN.iter()).find(|(n, _)| *n == name).map(|(_, bytes)| *bytes)
}

/// A symbolic name as the desktop knows it, to the Tabler icon drawn for it.
fn translate(name: &str) -> &str {
    match name {
        "emblem-system-symbolic" => "settings",
        "network-wired-symbolic" => "ethernet",
        "network-server-symbolic" => "server",
        "network-wireless-symbolic" | "network-wireless-signal-excellent-symbolic" | "network-wireless-signal-good-symbolic" => "wifi",
        "network-wireless-signal-ok-symbolic" => "wifi-2",
        "network-wireless-signal-weak-symbolic" => "wifi-1",
        "network-wireless-signal-none-symbolic" => "wifi-0",
        "network-wireless-offline-symbolic" | "network-wireless-disabled-symbolic" => "wifi-off",
        "network-wireless-encrypted-symbolic" => "lock",
        "bluetooth-symbolic" | "bluetooth-active-symbolic" => "bluetooth",
        "video-display-symbolic" | "video-joined-displays-symbolic" | "display-brightness-off-symbolic" => "device-desktop",
        "display-brightness-symbolic" => "sun",
        "keyboard-brightness-symbolic" | "input-keyboard-symbolic" => "keyboard",
        "input-mouse-symbolic" => "mouse",
        "input-gaming-symbolic" => "device-gamepad",
        "input-tablet-symbolic" => "device-ipad",
        "phone-symbolic" => "device-mobile",
        "computer-symbolic" => "device-desktop",
        "audio-headphones-symbolic" => "headphones",
        "audio-headset-symbolic" => "headset",
        "audio-speakers-symbolic" => "device-speaker",
        "power-profile-balanced-symbolic" => "scale",
        "power-profile-performance-symbolic" => "rocket",
        "power-profile-power-saver-symbolic" => "leaf",
        "system-shutdown-symbolic" => "power",
        "system-lock-screen-symbolic" => "lock",
        "system-log-out-symbolic" => "logout",
        "system-suspend-symbolic" => "moon",
        "system-reboot-symbolic" => "refresh",
        "camera-photo-symbolic" | "camera-web-symbolic" => "camera",
        "airplane-mode-symbolic" => "plane",
        "audio-volume-high-symbolic" | "audio-volume-medium-symbolic" => "volume",
        "audio-volume-low-symbolic" => "volume-2",
        "audio-volume-muted-symbolic" => "volume-off",
        "audio-input-microphone-symbolic" => "microphone",
        "microphone-sensitivity-muted-symbolic" => "microphone-off",
        "audio-x-generic-symbolic" => "music",
        "media-playback-start-symbolic" => "player-play",
        "media-playback-pause-symbolic" => "player-pause",
        "media-skip-backward-symbolic" => "player-skip-back",
        "media-skip-forward-symbolic" => "player-skip-forward",
        "media-playlist-shuffle-symbolic" => "arrows-shuffle",
        "media-playlist-repeat-symbolic" => "repeat",
        "media-playlist-repeat-song-symbolic" => "repeat-once",
        "media-record-symbolic" => "activity",
        "go-next-symbolic" => "chevron-right",
        "go-down-symbolic" => "chevron-down",
        "go-up-symbolic" => "chevron-up",
        "object-select-symbolic" => "check",
        "window-close-symbolic" => "x",
        "view-reveal-symbolic" => "eye",
        "view-conceal-symbolic" => "eye-off",
        "notification-symbolic" | "preferences-system-notifications-symbolic" => "message-circle",
        "notifications-disabled-symbolic" => "bell-off",
        "battery-symbolic" | "battery-missing-symbolic" => "battery",
        other => other,
    }
}

/// An icon by name, so many pixels across, in the current color.
/// Whether the name is one of ours (else it is an app's, from the theme).
pub fn known(name: &str) -> bool {
    svg(translate(name)).is_some()
}

pub fn icon(name: &str, size: u16) -> widget::icon::Icon {
    let bytes = svg(translate(name)).or_else(|| svg("activity")).expect("an icon");
    widget::icon::from_svg_bytes(bytes).symbolic(true).icon().size(size)
}

/// The brand mark of a player, if Tabler draws one.
pub fn brand(app: &str) -> Option<&'static str> {
    let app = app.to_lowercase();
    if app.contains("spotify") {
        Some("brand-spotify")
    } else if app.contains("chrom") {
        Some("brand-chrome")
    } else if app.contains("firefox") {
        Some("brand-firefox")
    } else {
        None
    }
}
