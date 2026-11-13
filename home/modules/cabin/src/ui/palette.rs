// SPDX-License-Identifier: GPL-3.0-only

//! The colors, fixed: the design's periwinkle and pale text on a navy taken
//! from the wallpaper's own blue, lighter than the design's glass.

use cosmic::iced::Color;

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

pub const fn rgb_(r: u8, g: u8, b: u8) -> Color {
    rgb(r, g, b)
}

pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

/// A panel: the wallpaper's shadow navy as a glass tint, over the frost the
/// compositor lays behind the panes (ext-background-effect), opaque enough
/// that what's behind stays unreadable.
pub const PANE: Color = rgba(34, 46, 84, 0.84);
pub const PANE_BORDER: Color = rgba(200, 220, 255, 0.2);
pub const PANE_HIGHLIGHT: Color = rgba(255, 255, 255, 0.08);
/// A menu inside a panel, a notification, the calendar: an inset pool.
pub const CARD: Color = rgba(12, 18, 40, 0.32);
/// A button or pill resting on the panel, and under the pointer.
pub const SURFACE: Color = rgba(200, 220, 255, 0.12);
pub const SURFACE_HOVER: Color = rgba(200, 220, 255, 0.2);
/// A menu row under the pointer, and the one picked.
pub const ROW_HOVER: Color = rgba(200, 220, 255, 0.12);
pub const ROW_SELECTED: Color = rgba(200, 220, 255, 0.18);
/// What's on.
pub const ACCENT: Color = rgb(0xa9, 0xc3, 0xec);
pub const ACCENT_HOVER: Color = rgb(0xbc, 0xd1, 0xf2);
pub const ON_ACCENT: Color = rgb(0x14, 0x1d, 0x38);
pub const ON_ACCENT_SUB: Color = rgb(0x3a, 0x4a, 0x72);
/// Text and icons.
pub const TEXT: Color = rgb(0xe8, 0xec, 0xf6);
pub const MUTED: Color = rgba(232, 236, 246, 0.72);
pub const FAINT: Color = rgba(232, 236, 246, 0.5);
/// Lines.
pub const SEP: Color = rgba(200, 220, 255, 0.18);
/// A slider's rail, and its knob.
pub const TRACK: Color = rgba(200, 220, 255, 0.16);
pub const KNOB: Color = rgb(0xf1, 0xe2, 0xec);
/// Sound past 100 %.
pub const OVER: Color = rgb(0xe2, 0x78, 0x8a);
/// Shutting down.
pub const RED: Color = rgb(0xb8, 0x38, 0x4a);
pub const RED_HOVER: Color = rgb(0xc9, 0x4a, 0x5c);
pub const ON_RED: Color = rgb(0xfb, 0xe9, 0xec);
/// A battery running out: its icon on the bar and in the controls.
pub const LOW: Color = rgb(0xe8, 0x4f, 0x63);
/// A muted app's badge.
pub const MUTE_BADGE: Color = rgb(0xd0, 0x43, 0x5a);
/// The plate behind a player's badge on its cover.
pub const BADGE_PLATE: Color = rgb(0x15, 0x1b, 0x32);
/// A right-click menu.
pub const CTX: Color = rgba(34, 46, 84, 0.97);
pub const CTX_BORDER: Color = rgba(200, 220, 255, 0.2);
/// A player's card without a cover.
pub const MEDIA: Color = rgba(12, 18, 40, 0.5);
/// A calendar's event dot, and the upload line of the network graph.
pub const EVENT: Color = rgb(0xe9, 0xa3, 0xb4);
/// On the coral: screen sharing, the Stop button, the bubble past 100 %.
pub const ON_OVER: Color = rgb(0x2a, 0x0c, 0x14);
pub const OVER_HOVER: Color = rgb(0xec, 0x92, 0xa1);
/// Amber: a camera or microphone in use.
pub const AMBER: Color = rgb(0xf2, 0xb4, 0x5a);
pub const AMBER_HOVER: Color = rgb(0xf6, 0xc5, 0x7a);
pub const ON_AMBER: Color = rgb(0x2a, 0x1a, 0x04);
pub const ON_AMBER_SUB: Color = rgb(0x5a, 0x3a, 0x08);
pub const ON_OVER_SUB: Color = rgb(0x5a, 0x1a, 0x28);

/// White or black, whichever reads better on a color.
pub fn on(background: Color) -> Color {
    let luma = 0.2126 * background.r + 0.7152 * background.g + 0.0722 * background.b;
    if luma > 0.55 { ON_ACCENT } else { TEXT }
}

pub const fn alpha(mut color: Color, a: f32) -> Color {
    color.a = a;
    color
}
