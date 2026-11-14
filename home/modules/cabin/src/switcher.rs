// SPDX-License-Identifier: GPL-3.0-only

//! The window switcher. Alt+Tab (or Super+Tab) runs `cabinctl switch next`
//! and the daemon puts the open windows up as a row of app marks on a pane
//! of glass in the middle of the screen, the one used before this one
//! picked. Each Tab after moves on (the compositor runs the shortcut again),
//! Shift goes back, as do the arrow keys; letting go of Alt or Super raises
//! the pick. A press on a mark raises that one; Escape or a press anywhere
//! else leaves things as they were. Super+Tab (`cabinctl switch next
//! --previews`) shows the windows as cards with a live picture of each
//! instead (`backend::live`).

use std::collections::HashMap;

use cosmic::iced::platform_specific::shell::subsurface_widget::{Subsurface, SubsurfaceBuffer};

use cosmic::{
    Element,
    iced::{
        self, Alignment, Background, Color, Length, Point, Rectangle, Size,
        keyboard::Modifiers, window,
    },
    theme, widget,
};

use crate::{
    backend::{self, windows::Window},
    ui::{self, PANE_RADIUS, TILE_RADIUS, palette, popup::Placed, registry},
};

/// A window's square, and at most how many show at once; past that the
/// row slides along with the pick, like a gallery.
const TILE: f32 = 88.0;
const TILE_LIMIT: usize = 9;
const GAP: f32 = 8.0;
/// Either side of a row that slides: a chevron where more windows are.
const ARROW: f32 = 20.0;
const PAD: f32 = 16.0;
/// A window's card with its picture, how many show at once, and inside
/// it: the picture is 16:10, the app's mark and name under it.
const CARD: f32 = 280.0;
const CARD_LIMIT: usize = 5;
pub const CARD_PAD: f32 = 8.0;
pub const LABEL_GAP: f32 = 6.0;
pub const LABEL: f32 = 20.0;
/// Under the row: the picked window's app and title.
const CAPTION_GAP: f32 = 12.0;
const CAPTION: f32 = 22.0;
/// Room enough for a caption, however few windows.
const MIN_WIDTH: f32 = 360.0;
/// Kept clear on either side of the screen.
const MARGIN: f32 = 48.0;

#[derive(Clone, Debug)]
pub enum Message {
    /// A window pressed: raise it.
    Pick(usize),
    /// Move the pick on (back when negative).
    Step(i32),
    /// Raise the pick, or leave things as they were.
    Commit,
    Cancel,
    /// The modifiers now, on this surface.
    Modifiers(window::Id, Modifiers),
    /// The time to have seen Alt or Super held, on this surface.
    Late(window::Id),
    /// Held long enough to be seen.
    Reveal(window::Id),
    Refrost,
}

pub struct Switcher {
    pub id: window::Id,
    pub size: Option<Size>,
    pub windows: Vec<Window>,
    pub selected: usize,
    /// How many times the surface has heard the modifiers (on getting the
    /// keyboard: iced's guess from the keys down, then the compositor's
    /// word), and whether Alt or Super was held then or since.
    pub heard: u32,
    pub held: bool,
    /// Drawn: not until held a moment, so a quick tap doesn't flash it.
    pub shown: bool,
    /// Cards with live pictures (Super+Tab).
    pub previews: bool,
    /// The first window in sight.
    pub offset: usize,
}

/// How the row fits the screen: each square's (or card's) width, how many
/// show at once, and whether there are more than that.
pub struct Fit {
    pub tile: f32,
    pub shown: usize,
    pub sliding: bool,
}

impl Switcher {
    /// Open on `windows`, the one used last first: forward picks the one
    /// before it, back the one used longest ago.
    pub fn new(id: window::Id, windows: Vec<Window>, step: i32, previews: bool) -> Self {
        let selected = first_pick(windows.len(), step);
        Self {
            id,
            size: None,
            windows,
            selected,
            heard: 0,
            held: false,
            shown: false,
            previews,
            offset: 0,
        }
    }

    pub fn step(&mut self, by: i32) {
        self.selected = stepped(self.selected, by, self.windows.len());
        self.follow();
    }

    /// The surface's size, once the compositor sets it.
    pub fn resized(&mut self, size: Size) {
        self.size = Some(size);
        self.follow();
    }

    /// The row slid to keep the pick in sight.
    fn follow(&mut self) {
        if let Some(size) = self.size {
            self.offset = slid(
                self.offset,
                self.selected,
                self.fit(size).shown,
                self.windows.len(),
            );
        }
    }

    fn fit(&self, size: Size) -> Fit {
        let n = self.windows.len().max(1);
        let (tile, limit) = if self.previews {
            (CARD, CARD_LIMIT)
        } else {
            (TILE, TILE_LIMIT)
        };
        let room = size.width - 2.0 * MARGIN - 2.0 * PAD;
        let fits = |room: f32| (((room + GAP) / (tile + GAP)).floor() as usize).max(1);
        let shown = n.min(limit).min(fits(room));
        if shown < n {
            Fit {
                tile,
                shown: n.min(limit).min(fits(room - 2.0 * (ARROW + GAP))),
                sliding: true,
            }
        } else {
            Fit {
                tile,
                shown,
                sliding: false,
            }
        }
    }

    pub fn picked(&self) -> Option<&Window> {
        self.windows.get(self.selected)
    }

    /// The windows that closed while it showed dropped; the pick stays on
    /// its window if that's still open.
    pub fn keep(&mut self, open: &[Window]) {
        let picked = self.picked().map(|w| w.handle.clone());
        self.windows
            .retain(|w| open.iter().any(|o| o.handle == w.handle));
        self.selected = picked
            .and_then(|p| self.windows.iter().position(|w| w.handle == p))
            .unwrap_or(0)
            .min(self.windows.len().saturating_sub(1));
        self.follow();
    }

    /// The glass's rectangle on the surface, and how the row fits in it.
    pub fn frame(&self) -> Option<(Rectangle, Fit)> {
        let size = self.size?;
        let fit = self.fit(size);
        let k = fit.shown as f32;
        let arrows = if fit.sliding {
            2.0 * (ARROW + GAP)
        } else {
            0.0
        };
        let width = (k * fit.tile + (k - 1.0) * GAP + arrows + 2.0 * PAD).max(MIN_WIDTH);
        // The glass's top highlight is a pixel of its own.
        let height = 1.0 + 2.0 * PAD + self.tile_height(fit.tile) + CAPTION_GAP + CAPTION;
        let at = Point::new(
            ((size.width - width) / 2.0).round(),
            ((size.height - height) / 2.0).round(),
        );
        Some((Rectangle::new(at, Size::new(width, height)), fit))
    }

    /// A square is as tall as it's wide; a card, its picture and label.
    fn tile_height(&self, tile: f32) -> f32 {
        if self.previews {
            2.0 * CARD_PAD + picture_height(tile) + LABEL_GAP + LABEL
        } else {
            tile
        }
    }

    /// The region the compositor frosts.
    pub fn frost_region(&self) -> Option<Vec<Rectangle>> {
        self.shown
            .then(|| self.frame())
            .flatten()
            .map(|(rect, _)| ui::popup::rounded_region(&[rect], PANE_RADIUS))
    }

    /// With the live pictures by window id.
    pub fn view<'a>(&'a self, pictures: &HashMap<String, SubsurfaceBuffer>) -> Element<'a, Message> {
        let (Some(size), Some((frame, fit)), true) = (self.size, self.frame(), self.shown) else {
            return widget::space::horizontal().into();
        };
        let tile = fit.tile;
        let height = self.tile_height(tile);
        let arrow = |name: &str, more: bool| -> Element<'_, Message> {
            let inside: Element<'_, Message> = if more {
                ui::icon(name, 18).into()
            } else {
                widget::space::horizontal().into()
            };
            widget::container(inside)
                .width(Length::Fixed(ARROW))
                .height(Length::Fixed(height))
                .center(Length::Fill)
                .class(theme::Container::custom(|_| widget::container::Style {
                    icon_color: Some(palette::FAINT),
                    ..Default::default()
                }))
                .into()
        };
        let mut tiles: Vec<Element<'_, Message>> = self
            .windows
            .iter()
            .enumerate()
            .skip(self.offset)
            .take(fit.shown)
            .map(|(i, w)| {
                let (inside, padding) = if self.previews {
                    let picture = pictures.get(&w.id);
                    (card(w, &name(&w.app_id), picture, tile), CARD_PAD)
                } else {
                    (mark(&w.app_id, tile), 0.0)
                };
                widget::button::custom(
                    widget::container(inside)
                        .center(Length::Fill)
                        .class(ui::inherit()),
                )
                .width(Length::Fixed(tile))
                .height(Length::Fixed(height))
                .padding(padding)
                .class(tile_style(i == self.selected))
                .on_press(Message::Pick(i))
                .into()
            })
            .collect();
        if fit.sliding {
            tiles.insert(0, arrow("chevron-left", self.offset > 0));
            tiles.push(arrow(
                "chevron-right",
                self.offset + fit.shown < self.windows.len(),
            ));
        }
        let row = widget::row::with_children(tiles)
            .spacing(GAP)
            .height(Length::Fixed(height));
        let caption: Element<'_, Message> = match self.picked() {
            Some(w) => {
                let name = name(&w.app_id);
                // Monospace-ish budget: 7 px a character fits Cantarell at 13 px.
                let room = ((frame.width - 2.0 * PAD) / 7.0) as usize;
                let title = (!w.title.is_empty() && w.title != name)
                    .then(|| ellipsize(&w.title, room.saturating_sub(name.chars().count() + 3)));
                let mut parts = vec![ui::text::heading(name).into()];
                if let Some(title) = title.filter(|t| !t.is_empty()) {
                    parts.push(
                        ui::text::body(title)
                            .class(theme::Text::Custom(ui::dimmed))
                            .into(),
                    );
                }
                widget::row::with_children(parts)
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .into()
            }
            None => widget::space::horizontal().into(),
        };
        let content = widget::column::with_children(vec![
            widget::container(row).center_x(Length::Fill).into(),
            widget::container(caption)
                .center_x(Length::Fill)
                .center_y(Length::Fixed(CAPTION))
                .into(),
        ])
        .spacing(CAPTION_GAP);
        let glass = ui::glass(content, PAD, Some(frame.width));
        Placed::new(size)
            .push(glass, frame.position(), frame.width)
            .on_press_outside(Message::Cancel)
            .into()
    }
}

/// The first window in sight, moved only as far as keeps the pick in it.
fn slid(offset: usize, selected: usize, shown: usize, n: usize) -> usize {
    let offset = if selected < offset {
        selected
    } else if selected >= offset + shown {
        selected + 1 - shown
    } else {
        offset
    };
    offset.min(n.saturating_sub(shown))
}

/// A card's picture's height, 16:10 inside the card's padding.
pub fn picture_height(tile: f32) -> f32 {
    ((tile - 2.0 * CARD_PAD) * 0.625).round()
}

/// A window as its card, `tile` wide padding and all: its live picture
/// (its mark until that comes), the app's mark and `label` under it. The
/// picture is a subsurface, over whatever the card's surface draws there.
pub fn card<'a, M: Clone + 'a>(
    w: &Window,
    label_text: &str,
    picture: Option<&SubsurfaceBuffer>,
    tile: f32,
) -> Element<'a, M> {
    let (width, height) = (tile - 2.0 * CARD_PAD, picture_height(tile));
    // Shrink: the compositor scales it into the room, its shape kept.
    let picture: Element<'a, M> = match picture {
        Some(buffer) => Subsurface::new(buffer.clone()).into(),
        None => mark(&w.app_id, height),
    };
    let picture = widget::container(picture)
        .center_x(Length::Fixed(width))
        .center_y(Length::Fixed(height))
        .class(theme::Container::custom(|_| widget::container::Style {
            background: Some(Background::Color(palette::CARD)),
            border: iced::Border {
                radius: 10.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }));
    widget::column::with_children(vec![picture.into(), label(w, label_text, width, None)])
        .spacing(LABEL_GAP)
        .into()
}

/// A window's label, `width` wide: the app's mark, `text` cut to fit, and
/// `trailing` at the end.
pub fn label<'a, M: 'a>(
    w: &Window,
    text: &str,
    width: f32,
    trailing: Option<Element<'a, M>>,
) -> Element<'a, M> {
    let room = width - if trailing.is_some() { LABEL + 8.0 } else { 0.0 };
    let text = ellipsize(text, ((room / 7.5) as usize).saturating_sub(5));
    let mut parts = vec![
        mark(&w.app_id, LABEL * 1.6),
        ui::text::body(text).width(Length::Fill).into(),
    ];
    parts.extend(trailing);
    widget::row::with_children(parts)
        .spacing(6)
        .align_y(Alignment::Center)
        .height(Length::Fixed(LABEL))
        .into()
}

/// The pick on opening: forward, the window used before this one (this
/// one, if it's alone); back, the one used longest ago.
fn first_pick(n: usize, step: i32) -> usize {
    if step < 0 {
        n.saturating_sub(1)
    } else {
        1.min(n.saturating_sub(1))
    }
}

/// `from` moved `by`, round the ends.
fn stepped(from: usize, by: i32, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    (from as i64 + by as i64).rem_euclid(n as i64) as usize
}

/// The app's own name and icon, from its desktop entry, unless the
/// registry has a mark for it (it gives the rest a plain window).
fn own(app_id: &str) -> Option<(String, String)> {
    let known = registry::app_info(app(app_id)).icon != "app-window";
    if known {
        None
    } else {
        backend::apps::find(app_id, None)
    }
}

pub fn name(app_id: &str) -> String {
    own(app_id)
        .map(|(name, _)| name)
        .unwrap_or_else(|| registry::name(app(app_id)))
}

/// A window's mark, sized for its square: the registry's, else the app's
/// own icon (a name in the icon theme, or a file).
pub fn mark<'a, M: 'a>(app_id: &str, tile: f32) -> Element<'a, M> {
    match own(app_id) {
        Some((_, icon)) => {
            let size = (tile * 0.6) as u16;
            let handle = if icon.starts_with('/') {
                widget::icon::from_path(icon.into())
            } else {
                widget::icon::from_name(icon).size(size).handle()
            };
            widget::icon(handle).size(size).into()
        }
        None => registry::glyph(app(app_id), (tile * 0.5) as u16),
    }
}

/// The app part of an id the registry knows the last word of
/// (`com.mitchellh.ghostty`, `org.gnome.Nautilus`).
fn app(app_id: &str) -> &str {
    app_id
        .rsplit('.')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(app_id)
}

/// At most `max` characters, the last an ellipsis when cut.
pub fn ellipsize(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
    cut.truncate(cut.trim_end().len());
    if !cut.is_empty() {
        cut.push('…');
    }
    cut
}

/// A window's square: lit in the accent when picked, faint under the pointer.
fn tile_style(picked: bool) -> theme::Button {
    let style = move |hovered: bool| widget::button::Style {
        background: Some(Background::Color(if picked {
            palette::ROW_SELECTED
        } else if hovered {
            palette::ROW_HOVER
        } else {
            Color::TRANSPARENT
        })),
        border_radius: TILE_RADIUS.into(),
        border_width: if picked { 2.0 } else { 0.0 },
        border_color: palette::ACCENT,
        icon_color: Some(palette::TEXT),
        text_color: Some(palette::TEXT),
        ..widget::button::Style::new()
    };
    theme::Button::Custom {
        active: Box::new(move |_, _| style(false)),
        disabled: Box::new(move |_| style(false)),
        hovered: Box::new(move |_, _| style(true)),
        pressed: Box::new(move |_, _| style(true)),
    }
}

/// Whether these modifiers hold the switcher open.
pub fn holding(modifiers: Modifiers) -> bool {
    modifiers.alt() || modifiers.logo()
}

/// Keys on the switcher's surface, as its messages.
pub fn key(key: &iced::keyboard::Key, modifiers: Modifiers) -> Option<Message> {
    use iced::keyboard::{Key, key::Named};
    match key {
        Key::Named(Named::Tab) => Some(Message::Step(if modifiers.shift() { -1 } else { 1 })),
        Key::Named(Named::ArrowRight) => Some(Message::Step(1)),
        Key::Named(Named::ArrowLeft) => Some(Message::Step(-1)),
        Key::Named(Named::Enter) => Some(Message::Commit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_on_the_window_before() {
        assert_eq!(first_pick(0, 1), 0);
        assert_eq!(first_pick(1, 1), 0);
        assert_eq!(first_pick(4, 1), 1);
        assert_eq!(first_pick(4, -1), 3);
    }

    #[test]
    fn the_row_slides_with_the_pick() {
        // 12 windows, 5 in sight.
        assert_eq!(slid(0, 4, 5, 12), 0);
        assert_eq!(slid(0, 5, 5, 12), 1);
        assert_eq!(slid(1, 3, 5, 12), 1);
        assert_eq!(slid(1, 0, 5, 12), 0);
        // Round the end, back from the first: the last five.
        assert_eq!(slid(0, 11, 5, 12), 7);
        // Fewer windows than room: never slides.
        assert_eq!(slid(0, 2, 5, 3), 0);
        // A window closed under it: no gap at the end.
        assert_eq!(slid(7, 9, 5, 10), 5);
    }

    #[test]
    fn steps_round_the_ends() {
        assert_eq!(stepped(3, 1, 4), 0);
        assert_eq!(stepped(0, -1, 4), 3);
        assert_eq!(stepped(1, 1, 4), 2);
        assert_eq!(stepped(0, 1, 0), 0);
    }

    #[test]
    fn app_ids_by_their_last_word() {
        assert_eq!(app("com.mitchellh.ghostty"), "ghostty");
        assert_eq!(app("chromium-browser"), "chromium-browser");
        assert_eq!(app(""), "");
    }

    #[test]
    fn long_titles_are_cut() {
        assert_eq!(ellipsize("short", 10), "short");
        assert_eq!(ellipsize("a long window title", 8), "a long…");
        assert_eq!(ellipsize("abc", 0), "");
    }
}
