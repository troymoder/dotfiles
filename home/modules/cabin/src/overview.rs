// SPDX-License-Identifier: GPL-3.0-only

//! The workspace overview, in place of cosmic-workspaces (slow to come up,
//! and it crashed when toggled fast). Super, or a press on the panel's
//! dots, puts this workspace's windows up as cards with a live picture of
//! each over the bare desktop, and a live picture of each workspace in a
//! column at the left.
//!
//! A press on a card raises its window; its × closes it; dragged onto a
//! workspace, it moves there. A press on another workspace switches to
//! it, on the one showing closes the overview; a workspace's pin keeps it
//! when it has no windows. The wheel steps through the workspaces, round and round.
//! Typing hands the text to the launcher; Escape, Super again or a press
//! on nothing closes.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use cosmic::{
    Element,
    iced::{
        self, Background, Color, Length, Point, Rectangle, Size, Vector,
        platform_specific::shell::subsurface_widget::{Subsurface, SubsurfaceBuffer},
        keyboard::{Key, Modifiers},
        mouse::ScrollDelta,
        window,
    },
    theme, widget,
};

use crate::{
    backend::windows::{Window, Workspace},
    switcher::{self, LABEL},
    ui::{self, TILE_RADIUS, palette, popup::Placed},
};

/// Kept clear round the edge, and between the cards.
const MARGIN: f32 = 32.0;
const GAP: f32 = 20.0;
/// A workspace in the column: its width, at most its height, the gap
/// between two, the column's padding.
const SPACE: f32 = 240.0;
const SPACE_HEIGHT: f32 = 186.0;
/// A workspace's pin: a small chip inside its picture's top right corner,
/// squared off like the pictures.
const PIN: f32 = 22.0;
const PIN_INSET: f32 = 7.0;
const PIN_RADIUS: f32 = 6.0;
/// The wheel and touchpad: the first bit of a scroll steps one workspace;
/// carrying on, each this far more (in pixels; a wheel's notch counts
/// half), and no sooner than this after the last step, steps again; this
/// far back the other way steps back at once (less is a touchpad's
/// jitter). A scroll ends after a pause this long.
const SCROLL_AGAIN: f32 = 400.0;
const SCROLL_EVERY: Duration = Duration::from_millis(250);
const SCROLL_BACK: f32 = 10.0;
/// A touchpad pushed again mid-scroll: going more than this many times as
/// fast as its slowest since the last step, and this much faster in pixels
/// a second, steps too (no sooner than `SCROLL_EVERY` after the last).
const SURGE: f32 = 2.0;
const SURGE_BY: f32 = 600.0;
const SCROLL_PAUSE: Duration = Duration::from_millis(400);
const SPACE_GAP: f32 = 10.0;
/// The window under the pointer while dragged: its picture this wide.
const GHOST: f32 = 240.0;
/// How far a press must travel to be a drag, not a click.
const DRAG: f32 = 6.0;
/// A card's close button, at the end of its title: as tall as the label,
/// its × as big as a 12 px icon would be.
const CLOSE: f32 = LABEL;
const CLOSE_X: f32 = 12.0;
/// A card: its title bar over its picture, the gap between, and the ring
/// round the picture (lit under the pointer).
const TITLE: f32 = 30.0;
const TITLE_GAP: f32 = 6.0;
const TITLE_PAD: f32 = 10.0;
const RING: f32 = 3.0;
/// A card's title bar is at least this wide, however small its window.
const MIN_TITLE: f32 = 180.0;
/// A window's size before its first picture says otherwise.
const UNKNOWN: Size = Size::new(1600.0, 1000.0);
/// On top of their sizes, the windows used last are larger: each after the
/// first gets this much less of its share, down to 70 %.
const RECENCY: f32 = 0.08;
/// The title bar: night over whatever the wallpaper is, lighter under the
/// pointer.
const TITLE_BG: Color = palette::rgba(16, 21, 38, 0.92);
const TITLE_HOVER: Color = palette::rgba(44, 56, 96, 0.96);
/// A picture's outline at rest, a hair to set it off the wallpaper.
const EDGE: Color = palette::rgba(8, 11, 22, 0.3);

#[derive(Clone, Debug)]
pub enum Message {
    /// The button went down on a window's card (its id).
    Press(String),
    /// A window's × pressed.
    CloseWindow(String),
    /// A workspace pressed: switch to it, or close if it's the one showing.
    Space(Workspace),
    /// The pointer came onto (true) or left a card or a workspace.
    Hover(Target, bool),
    /// A character typed: for the launcher.
    Typed(String),
    /// The surface got (true) or lost the keyboard.
    Focus(window::Id, bool),
    /// The wheel: on through the workspaces.
    Scroll(ScrollDelta),
    /// A workspace's pin pressed.
    Pin(Workspace),
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Card(String),
    Space(Workspace),
    /// A workspace's pin: it lies over the workspace's corner, and counts
    /// as the workspace (its outline, a drop).
    Pin(Workspace),
}

/// A card pressed, maybe being dragged.
pub struct Drag {
    /// The window's id, and where the press was.
    pub window: String,
    pub from: Point,
    /// Gone further than a click would.
    pub moved: bool,
}

pub struct Overview {
    pub id: window::Id,
    /// The windows by id, the one used last first, as they were when it
    /// opened (closed ones dropped): the cards keep their places and sizes
    /// while it shows, whatever takes the focus meanwhile.
    pub order: Vec<String>,
    pub cursor: Point,
    pub hover: Option<Target>,
    pub drag: Option<Drag>,
    /// The compositor gave it the keyboard, and something was typed (for
    /// the launcher): losing the keyboard after both closes the overview.
    pub focused: bool,
    pub typed: bool,
    /// Scrolling not yet a step, and the workspace stepped to until the
    /// compositor says it's active.
    pub scrolled: f32,
    pub scroll_at: Option<Instant>,
    pub stepped_at: Option<Instant>,
    /// The way the last step went: 1 on, -1 back.
    pub stepped: isize,
    /// A touchpad's speed (px/s, smoothed) and its slowest since the last step.
    pub speed: f32,
    pub lull: f32,
    pub pending: Option<usize>,
}

impl Overview {
    /// Open on the windows in `order` (their ids, the one used last first).
    pub fn new(id: window::Id, order: Vec<String>) -> Self {
        Self {
            id,
            order,
            cursor: Point::ORIGIN,
            hover: None,
            drag: None,
            focused: false,
            typed: false,
            scrolled: 0.0,
            scroll_at: None,
            stepped_at: None,
            stepped: 0,
            speed: 0.0,
            lull: f32::INFINITY,
            pending: None,
        }
    }

    /// The step (one workspace on, back, or none) this scrolling makes at
    /// `now`: down or right goes on, up or left back. A new scroll steps at
    /// once; carrying on, every `SCROLL_AGAIN` more steps again.
    pub fn scrolled(&mut self, delta: ScrollDelta, now: Instant) -> isize {
        let (amount, touchpad) = match delta {
            ScrollDelta::Lines { x, y } => {
                ((if y != 0.0 { -y } else { x }) * SCROLL_AGAIN / 2.0, false)
            }
            ScrollDelta::Pixels { x, y } => (if y != 0.0 { -y } else { x }, true),
        };
        if amount == 0.0 {
            return 0;
        }
        let since = self.scroll_at.map(|at| now.saturating_duration_since(at));
        let new = since.is_none_or(|since| since > SCROLL_PAUSE);
        self.scroll_at = Some(now);
        let way = amount.signum() as isize;
        if new {
            self.speed = 0.0;
            return self.step(way, now);
        }
        let soon = self
            .stepped_at
            .is_some_and(|at| now.saturating_duration_since(at) < SCROLL_EVERY);
        if touchpad {
            let dt = since.unwrap_or_default().as_secs_f32().max(0.004);
            let speed = amount.abs() / dt;
            self.speed = if self.speed == 0.0 {
                speed
            } else {
                (self.speed + speed) / 2.0
            };
            // Not while the push that stepped is still getting going: its
            // own speeding up is no new push.
            if !soon {
                self.lull = self.lull.min(self.speed);
            }
        }
        // Turning back: what had gathered the old way goes, and a little
        // the new way steps at once.
        if way != self.stepped {
            if self.scrolled.signum() as isize == self.stepped {
                self.scrolled = 0.0;
            }
            self.scrolled += amount;
            return if self.scrolled.abs() >= SCROLL_BACK {
                self.step(way, now)
            } else {
                0
            };
        }
        self.scrolled += amount;
        // Pushed again: well faster than it had slowed to.
        let surge = touchpad && self.speed > self.lull * SURGE && self.speed - self.lull > SURGE_BY;
        if soon || (self.scrolled.abs() < SCROLL_AGAIN && !surge) {
            return 0;
        }
        self.step(way, now)
    }

    fn step(&mut self, way: isize, now: Instant) -> isize {
        self.scrolled = 0.0;
        self.stepped_at = Some(now);
        self.stepped = way;
        // The next surge is against how slow it gets from here.
        self.lull = f32::INFINITY;
        way
    }

    /// The pointer moved: a press that goes far enough is a drag.
    pub fn moved(&mut self, at: Point) {
        self.cursor = at;
        if let Some(drag) = &mut self.drag {
            drag.moved |= drag.from.distance(at) > DRAG;
        }
    }

    /// Laid out at the size the surface has, not one learnt from a resize
    /// event after it: cosmic-comp hides the windows the moment this
    /// surface shows a frame, so the first must be whole.
    pub fn view<'a>(
        &'a self,
        windows: &'a [Window],
        groups: &'a [Vec<Workspace>],
        pictures: &'a HashMap<String, SubsurfaceBuffer>,
    ) -> Element<'a, Message> {
        widget::responsive(move |size| self.laid_out(size, windows, groups, pictures)).into()
    }

    fn laid_out<'a>(
        &'a self,
        size: Size,
        windows: &'a [Window],
        groups: &'a [Vec<Workspace>],
        pictures: &'a HashMap<String, SubsurfaceBuffer>,
    ) -> Element<'a, Message> {
        let spaces = group(groups, windows);
        let mut shown = shown(windows, spaces);
        shown.sort_by_key(|w| {
            self.order
                .iter()
                .position(|id| *id == w.id)
                .unwrap_or(usize::MAX)
        });

        // The workspaces down the left, straight on the desktop like the
        // cards, each as large as the column allows.
        let n = spaces.len().max(1) as f32;
        let tall = ((size.height - 2.0 * MARGIN - (n - 1.0) * SPACE_GAP) / n).min(SPACE_HEIGHT);
        let side = SPACE;
        let mut placed = Placed::new(size);
        let mut y = MARGIN;
        for space in spaces {
            let ratio = pictures
                .get(&space.key)
                .map_or(1.6, |b| b.width() as f32 / b.height().max(1) as f32);
            // The picture's own shape, as wide as the column or as tall as allowed.
            let mut box_ = Size::new(SPACE - 2.0 * RING, (SPACE - 2.0 * RING) / ratio);
            if box_.height > tall - 2.0 * RING {
                box_ = Size::new((tall - 2.0 * RING) * ratio, tall - 2.0 * RING);
            }
            let width = box_.width + 2.0 * RING;
            let at = Point::new(MARGIN + (SPACE - width) / 2.0, y);
            placed = placed.push(self.space(space, windows, pictures, box_), at, width);
            // After the tile, so it lies over the picture and takes the press.
            let corner = at + Vector::new(width - PIN - PIN_INSET, PIN_INSET);
            placed = placed.push(pin_button(space), corner, PIN);
            y += box_.height + 2.0 * RING + SPACE_GAP;
        }

        // This workspace's windows, as large as they all fit right of the
        // column, each row centred on the screen unless that would put it
        // over the column.
        let left = MARGIN + side + GAP;
        let room = Size::new(size.width - left - MARGIN, size.height - 2.0 * MARGIN);
        let centre = size.width / 2.0 - left;
        // Each window's size, from its picture: the cards keep the windows'
        // shapes, and their sizes against each other.
        let sizes: Vec<Size> = shown
            .iter()
            .map(|w| {
                pictures.get(&w.id).map_or(UNKNOWN, |b| {
                    Size::new(b.width() as f32, b.height() as f32)
                })
            })
            .collect();
        for card in cards(&sizes, room, centre) {
            let at = card.at + Vector::new(left, MARGIN);
            placed = placed.push(
                self.card(shown[card.index], pictures, card.width, card.picture),
                at,
                card.width,
            );
        }

        // The window being dragged, under the pointer: its picture, above
        // the other pictures (they're subsurfaces too).
        if let Some(w) = self
            .drag
            .as_ref()
            .filter(|d| d.moved)
            .and_then(|d| windows.iter().find(|w| w.id == d.window))
        {
            let ghost: Element<'a, Message> = match pictures.get(&w.id) {
                Some(buffer) => Subsurface::new(buffer.clone()).z(1).alpha(0.85).into(),
                None => switcher::mark(&w.app_id, GHOST / 2.0),
            };
            let tall = GHOST * 0.625;
            placed = placed.push(
                widget::container(ghost).center_x(Length::Fixed(GHOST)).center_y(Length::Fixed(tall)),
                self.cursor - Vector::new(GHOST / 2.0, tall / 2.0),
                GHOST,
            );
        }

        // See-through: cosmic-comp draws no windows while it shows, so
        // what's behind is the desktop.
        widget::mouse_area(placed.on_press_outside(Message::Close))
            .on_scroll(Message::Scroll)
            .into()
    }

    /// A workspace: its live picture (its windows' marks until that comes),
    /// below this surface so the pin and the outline can lie over it. The
    /// outline hugs it: the accent on the one that shows, lighter under the
    /// pointer, where a dragged window would land, with a tint.
    fn space<'a>(
        &self,
        space: &Workspace,
        windows: &[Window],
        pictures: &HashMap<String, SubsurfaceBuffer>,
        box_: Size,
    ) -> Element<'a, Message> {
        let picture: Element<'a, Message> = match pictures.get(&space.key) {
            Some(buffer) => Subsurface::new(buffer.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .z(-1)
                .into(),
            None => {
                let marks: Vec<Element<'a, Message>> = windows
                    .iter()
                    .filter(|w| w.workspaces.contains(&space.handle))
                    .take(6)
                    .map(|w| switcher::mark(&w.app_id, 40.0))
                    .collect();
                widget::container(widget::row::with_children(marks).spacing(2).wrap())
                    .center(Length::Fill)
                    .into()
            }
        };
        let target = Target::Space(space.clone());
        // Under the pointer itself: lit and tinted (where a dragged window
        // would land); only its pin under the pointer: a softer hint.
        let hovered = matches!(&self.hover, Some(Target::Space(s)) if s.handle == space.handle);
        let pin_hovered = matches!(&self.hover, Some(Target::Pin(s)) if s.handle == space.handle);
        let edge = if space.active {
            palette::ACCENT
        } else if hovered {
            palette::alpha(palette::TEXT, 0.7)
        } else if pin_hovered {
            palette::alpha(palette::TEXT, 0.3)
        } else {
            EDGE
        };
        let tint = if hovered {
            palette::alpha(palette::ACCENT, 0.16)
        } else {
            Color::TRANSPARENT
        };
        let tile = widget::container(picture)
            .center_x(Length::Fixed(box_.width + 2.0 * RING))
            .center_y(Length::Fixed(box_.height + 2.0 * RING))
            .padding(RING)
            .class(outline(edge, tint));
        widget::mouse_area(tile)
            .on_press(Message::Space(space.clone()))
            .on_enter(Message::Hover(target.clone(), true))
            .on_exit(Message::Hover(target, false))
            .into()
    }

    /// A window's card, `width` wide: its title bar (mark, title, ×) over
    /// its live picture at `picture` size, straight on the desktop; ringed
    /// tight under the pointer, faint while dragged away.
    fn card<'a>(
        &self,
        w: &Window,
        pictures: &HashMap<String, SubsurfaceBuffer>,
        width: f32,
        picture: Size,
    ) -> Element<'a, Message> {
        let target = Target::Card(w.id.clone());
        let hovered = self.hover.as_ref() == Some(&target);
        let dragged = self
            .drag
            .as_ref()
            .is_some_and(|d| d.moved && d.window == w.id);
        let title = if w.title.is_empty() {
            switcher::name(&w.app_id)
        } else {
            w.title.clone()
        };
        let label = switcher::label(
            w,
            &title,
            width - 2.0 * TITLE_PAD,
            Some(close_button(&w.id)),
        );
        let bar = widget::container(label)
            .padding([0.0, TITLE_PAD])
            .width(Length::Fixed(width))
            .center_y(Length::Fixed(TITLE))
            .class(fill(if hovered { TITLE_HOVER } else { TITLE_BG }, None));
        let size = picture;
        let picture: Element<'a, Message> = match pictures.get(&w.id) {
            // Filling a box of its own shape: nothing round it.
            Some(buffer) => Subsurface::new(buffer.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .alpha(if dragged { 0.4 } else { 1.0 })
                .into(),
            None => switcher::mark(&w.app_id, size.height.min(size.width) * 0.6),
        };
        let edge = if hovered { palette::ACCENT } else { EDGE };
        let frame = widget::container(picture)
            .center_x(Length::Fixed(size.width + 2.0 * RING))
            .center_y(Length::Fixed(size.height + 2.0 * RING))
            .padding(RING)
            .class(outline(edge, Color::TRANSPARENT));
        let card = widget::column::with_children(vec![bar.into(), frame.into()])
            .spacing(TITLE_GAP)
            .align_x(iced::Alignment::Center);
        widget::mouse_area(card)
            .on_press(Message::Press(w.id.clone()))
            .on_enter(Message::Hover(target.clone(), true))
            .on_exit(Message::Hover(target, false))
            .into()
    }
}

/// The workspaces of the output in use: the group holding the window used
/// last, else the first.
// shortcut: one output's workspaces, on the active output; a surface per
// output (as cosmic-workspaces had) once a second screen gets used.
pub fn group<'a>(groups: &'a [Vec<Workspace>], windows: &[Window]) -> &'a [Workspace] {
    windows
        .iter()
        .find_map(|w| {
            groups
                .iter()
                .find(|g| g.iter().any(|s| w.workspaces.contains(&s.handle)))
        })
        .or(groups.first())
        .map_or(&[], Vec::as_slice)
}

/// The windows on the workspace that shows, or on all of them.
fn shown<'a>(windows: &'a [Window], spaces: &[Workspace]) -> Vec<&'a Window> {
    let active = spaces.iter().find(|s| s.active);
    windows
        .iter()
        .filter(|w| {
            w.workspaces.is_empty() || active.is_some_and(|s| w.workspaces.contains(&s.handle))
        })
        .collect()
}


/// A card laid out: which window, its top left corner and width, and its
/// picture's size.
#[derive(Debug)]
struct Card {
    index: usize,
    at: Point,
    width: f32,
    picture: Size,
}

/// A window's card at `scale`: its width (the title's least, if the
/// picture is narrower) and its picture's size, the window's shape.
fn card_size(window: Size, scale: f32) -> (f32, Size) {
    let picture = Size::new(
        (window.width * scale).floor().max(1.0),
        (window.height * scale).floor().max(1.0),
    );
    ((picture.width + 2.0 * RING).max(MIN_TITLE), picture)
}

/// The share of its size the `i`th window used gets.
fn recency(i: usize) -> f32 {
    (1.0 - RECENCY * i as f32).max(0.7)
}

fn card_height(picture: Size) -> f32 {
    TITLE + TITLE_GAP + picture.height + 2.0 * RING
}

/// The windows' cards at `scale`, flowed into rows no wider than `width`
/// (a card too wide stays alone in its row): index, width, picture.
fn rows(windows: &[Size], scale: f32, width: f32) -> Vec<Vec<(usize, f32, Size)>> {
    let mut rows: Vec<Vec<(usize, f32, Size)>> = Vec::new();
    let mut used = 0.0;
    for (i, window) in windows.iter().enumerate() {
        let (card, picture) = card_size(*window, scale * recency(i));
        match rows.last_mut() {
            Some(row) if used + GAP + card <= width => {
                row.push((i, card, picture));
                used += GAP + card;
            }
            _ => {
                rows.push(vec![(i, card, picture)]);
                used = card;
            }
        }
    }
    rows
}

fn row_width(row: &[(usize, f32, Size)]) -> f32 {
    row.iter().map(|c| c.1).sum::<f32>() + GAP * row.len().saturating_sub(1) as f32
}

fn row_height(row: &[(usize, f32, Size)]) -> f32 {
    row.iter().map(|c| card_height(c.2)).fold(0.0, f32::max)
}

fn block_height(rows: &[Vec<(usize, f32, Size)>]) -> f32 {
    rows.iter().map(|r| row_height(r)).sum::<f32>() + GAP * rows.len().saturating_sub(1) as f32
}

/// The cards of windows this size (in their own pixels), all scaled
/// alike and as large as they all fit `room`, never past their own size:
/// each row centred on `centre` (across `room`) as far as it stays inside,
/// the block centred down, each card centred in its row's height.
fn cards(windows: &[Size], room: Size, centre: f32) -> Vec<Card> {
    if windows.is_empty() {
        return Vec::new();
    }
    let fits = |scale: f32| {
        let rows = rows(windows, scale, room.width);
        rows.iter().all(|r| row_width(r) <= room.width) && block_height(&rows) <= room.height
    };
    // The largest scale that fits, by halving.
    let (mut small, mut large) = (0.0f32, 1.0f32);
    if fits(large) {
        small = large;
    }
    for _ in 0..30 {
        let mid = (small + large) / 2.0;
        if fits(mid) {
            small = mid;
        } else {
            large = mid;
        }
    }
    let rows = rows(windows, small, room.width);
    let mut y = ((room.height - block_height(&rows)) / 2.0).max(0.0);
    let mut laid = Vec::with_capacity(windows.len());
    for row in rows {
        let height = row_height(&row);
        let span = row_width(&row);
        let mut x = (centre - span / 2.0).clamp(0.0, (room.width - span).max(0.0));
        for (index, width, picture) in row {
            let top = y + (height - card_height(picture)) / 2.0;
            laid.push(Card {
                index,
                at: Point::new(x.round(), top.round()),
                width,
                picture,
            });
            x += width + GAP;
        }
        y += height + GAP;
    }
    laid
}

/// A workspace's pin, in its picture's corner: a pin on a chip of the title
/// bars' night glass, tilted while loose, upright and lit in the accent
/// once pinned.
fn pin_button<'a>(space: &Workspace) -> Element<'a, Message> {
    let pinned = space.pinned;
    let style = move |hovered: bool| widget::button::Style {
        background: Some(Background::Color(match (pinned, hovered) {
            (true, false) => palette::ACCENT,
            (true, true) => palette::ACCENT_HOVER,
            (false, false) => palette::alpha(TITLE_BG, 0.78),
            (false, true) => TITLE_HOVER,
        })),
        border_radius: PIN_RADIUS.into(),
        // Glass's faint edge, so it reads over any picture; none when lit.
        border_width: if pinned { 0.0 } else { 1.0 },
        border_color: palette::alpha(palette::TEXT, if hovered { 0.45 } else { 0.2 }),
        ..widget::button::Style::new()
    };
    let color = if pinned {
        palette::ON_ACCENT
    } else {
        palette::alpha(palette::TEXT, 0.85)
    };
    // Tilted while loose, upright once pinned.
    let icon = widget::container(ui::icon(if pinned { "pinned" } else { "pin" }, 13))
        .center(Length::Fixed(PIN))
        .class(theme::Container::custom(move |_| widget::container::Style {
            icon_color: Some(color),
            ..Default::default()
        }));
    let button = widget::button::custom(icon)
        .width(Length::Fixed(PIN))
        .height(Length::Fixed(PIN))
        .padding(0)
        .class(theme::Button::Custom {
            active: Box::new(move |_, _| style(false)),
            disabled: Box::new(move |_| style(false)),
            hovered: Box::new(move |_, _| style(true)),
            pressed: Box::new(move |_, _| style(true)),
        })
        .on_press(Message::Pin(space.clone()));
    let target = Target::Pin(space.clone());
    widget::mouse_area(button)
        .on_enter(Message::Hover(target.clone(), true))
        .on_exit(Message::Hover(target, false))
        .into()
}

/// A window's ×: white on a bright red circle, a shade darker under the
/// pointer.
fn close_button<'a>(id: &str) -> Element<'a, Message> {
    let bare = |_: bool| widget::button::Style::new();
    widget::button::custom(
        widget::canvas(CloseMark)
            .width(Length::Fixed(CLOSE))
            .height(Length::Fixed(CLOSE)),
    )
    .width(Length::Fixed(CLOSE))
    .height(Length::Fixed(CLOSE))
    .padding(0)
    // The button only takes the press (and redraws on hover); the mark
    // draws itself.
    .class(theme::Button::Custom {
        active: Box::new(move |_, _| bare(false)),
        disabled: Box::new(move |_| bare(false)),
        hovered: Box::new(move |_, _| bare(true)),
        pressed: Box::new(move |_, _| bare(true)),
    })
    .on_press(Message::CloseWindow(id.to_owned()))
    .into()
}

/// The close button's circle and ×, drawn as one shape: on the same grid,
/// the × sits dead centre at any scale (an icon is snapped to whole pixels
/// and the circle isn't, which put it half a pixel off at 1.25×). The ×
/// is Tabler's: arms to a quarter in from the edges, line 1/12 of the size.
struct CloseMark;

impl<M> widget::canvas::Program<M, cosmic::Theme, cosmic::Renderer> for CloseMark {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Vec<widget::canvas::Geometry<cosmic::Renderer>> {
        use widget::canvas::{Frame, LineCap, Path, Stroke};
        let mut frame = Frame::new(renderer, bounds.size());
        let centre = frame.center();
        let radius = bounds.width.min(bounds.height) / 2.0;
        let fill = if cursor.is_over(bounds) {
            palette::rgb_(0xc2, 0x38, 0x4b)
        } else {
            palette::LOW
        };
        frame.fill(&Path::circle(centre, radius), fill);
        // Tabler's × spans 6..18 of 24 at 12 px in: half of 12/24 of 12.
        let arm = CLOSE_X / 4.0;
        let cross = Path::new(|path| {
            path.move_to(centre + Vector::new(-arm, -arm));
            path.line_to(centre + Vector::new(arm, arm));
            path.move_to(centre + Vector::new(arm, -arm));
            path.line_to(centre + Vector::new(-arm, arm));
        });
        frame.stroke(
            &cross,
            Stroke::default()
                .with_color(Color::WHITE)
                .with_width(CLOSE_X / 12.0 * 1.2)
                .with_line_cap(LineCap::Round),
        );
        vec![frame.into_geometry()]
    }
}

/// A picture's outline in the ring round it, over `tint`: 2 px when lit,
/// 1 px at rest.
fn outline(edge: Color, tint: Color) -> theme::Container<'static> {
    theme::Container::custom(move |_| widget::container::Style {
        background: Some(Background::Color(tint)),
        border: iced::Border {
            radius: RING.into(),
            width: if edge == EDGE { 1.0 } else { 2.0 },
            color: edge,
        },
        ..Default::default()
    })
}

/// A rounded fill, edged in `edge` if given.
fn fill(background: Color, edge: Option<Color>) -> theme::Container<'static> {
    theme::Container::custom(move |_| widget::container::Style {
        background: Some(Background::Color(background)),
        border: iced::Border {
            radius: TILE_RADIUS.into(),
            width: if edge.is_some() { 2.0 } else { 0.0 },
            color: edge.unwrap_or(Color::TRANSPARENT),
        },
        icon_color: Some(palette::TEXT),
        text_color: Some(palette::TEXT),
        ..Default::default()
    })
}

/// A character typed with no shortcut modifier, for the launcher.
pub fn typed(key: &Key, modifiers: Modifiers) -> Option<Message> {
    match key {
        Key::Character(c) if !modifiers.control() && !modifiers.alt() && !modifiers.logo() => {
            Some(Message::Typed(c.to_string()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: Size = Size::new(1400.0, 1000.0);

    const SCREEN: Size = Size::new(2048.0, 1247.0);
    const MIDDLE: f32 = ROOM.width / 2.0;

    #[test]
    fn rows_centre_on_the_screen_as_far_as_they_fit() {
        // The screen's middle left of the room's: a narrow row sits on it,
        // a wide one slides right just enough to stay in.
        let centre = 500.0;
        let one = cards(&[Size::new(400.0, 300.0)], ROOM, centre);
        assert_eq!(one[0].at.x + one[0].width / 2.0, centre);
        let wide = cards(&[SCREEN], ROOM, centre);
        assert_eq!(wide[0].at.x, 0.0);
        assert!(wide[0].width <= ROOM.width);
    }

    #[test]
    fn fewer_windows_make_larger_cards() {
        let first: Vec<f32> = (1..=12)
            .map(|n| cards(&vec![SCREEN; n], ROOM, MIDDLE)[0].picture.width)
            .collect();
        assert!(first.windows(2).all(|w| w[0] >= w[1]), "{first:?}");
        assert!(first[0] > first[3] && first[3] > first[8], "{first:?}");
        // A small window alone is shown at its own size, not blown up.
        let small = cards(&[Size::new(400.0, 300.0)], ROOM, MIDDLE);
        assert_eq!(small[0].picture, Size::new(400.0, 300.0));
    }

    #[test]
    fn cards_keep_the_windows_sizes_and_shapes() {
        let windows = [SCREEN, Size::new(1024.0, 623.0), Size::new(800.0, 1200.0)];
        let laid = cards(&windows, ROOM, MIDDLE);
        let picture = |i: usize| laid.iter().find(|c| c.index == i).unwrap().picture;
        // Half the window, half the picture, less the second's recency.
        assert!((picture(1).width * 2.0 / recency(1) - picture(0).width).abs() <= 3.0);
        // Each its window's shape, to the pixel.
        for (i, window) in windows.iter().enumerate() {
            let p = picture(i);
            let ratio = window.width / window.height;
            assert!((p.width / p.height - ratio).abs() < 0.02, "{i}: {p:?}");
        }
        // A tiny window still gets a title wide enough to read.
        let tiny = cards(&[SCREEN, Size::new(60.0, 40.0)], ROOM, MIDDLE);
        assert!(tiny.iter().all(|c| c.width >= MIN_TITLE));
    }

    #[test]
    fn the_windows_used_last_are_larger() {
        // Alike in size: each after the first smaller, down to 60 %.
        let laid = cards(&[SCREEN; 6], ROOM, MIDDLE);
        let mut widths: Vec<(usize, f32)> = laid.iter().map(|c| (c.index, c.picture.width)).collect();
        widths.sort_by_key(|w| w.0);
        assert!(widths.windows(2).all(|w| w[0].1 >= w[1].1), "{widths:?}");
        assert!(widths[0].1 > widths[1].1 && widths[1].1 > widths[3].1, "{widths:?}");
        // The floor: the fifth and after alike, at 70 %.
        assert_eq!(widths[4].1, widths[5].1);
        assert!((widths[5].1 / widths[0].1 - 0.7).abs() < 0.01, "{widths:?}");
    }

    #[test]
    fn a_swipe_speeding_up_is_one_push() {
        let mut overview = Overview::new(window::Id::unique(), Vec::new());
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let pixels = |y: f32| ScrollDelta::Pixels { x: 0.0, y };
        // Starts slow, gets fast, stays fast for 300 ms, short of 400 px.
        let mut steps = overview.scrolled(pixels(-1.0), ms(0));
        let mut at = 0;
        for y in [2.0, 3.0, 5.0, 8.0, 12.0, 16.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0] {
            at += 20;
            steps += overview.scrolled(pixels(-y), ms(at));
        }
        assert_eq!(steps, 1);
    }

    #[test]
    fn a_fresh_push_mid_scroll_steps() {
        let mut overview = Overview::new(window::Id::unique(), Vec::new());
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let pixels = |y: f32| ScrollDelta::Pixels { x: 0.0, y };
        assert_eq!(overview.scrolled(pixels(-10.0), ms(0)), 1);
        // Steady at 1000 px/s, then easing off to 100 px/s: under 400 px,
        // no step.
        let mut at = 0;
        let mut steps = 0;
        for _ in 0..8 {
            at += 20;
            steps += overview.scrolled(pixels(-20.0), ms(at));
        }
        for _ in 0..10 {
            at += 20;
            steps += overview.scrolled(pixels(-2.0), ms(at));
        }
        assert_eq!(steps, 0);
        // A fresh push, well past 250 ms since the step: steps at once.
        at += 20;
        assert_eq!(overview.scrolled(pixels(-60.0), ms(at)), 1);
        // Keeping that speed up is no new push.
        for _ in 0..5 {
            at += 20;
            assert_eq!(overview.scrolled(pixels(-60.0), ms(at)), 0);
        }
    }

    #[test]
    fn a_scroll_steps_at_once_then_with_friction() {
        let mut overview = Overview::new(window::Id::unique(), Vec::new());
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let line = |y: f32| ScrollDelta::Lines { x: 0.0, y };
        let pixels = |y: f32| ScrollDelta::Pixels { x: 0.0, y };
        // Any bit of a new scroll steps.
        assert_eq!(overview.scrolled(pixels(-2.0), ms(0)), 1);
        // A fast swipe: far enough in 128 ms, but too soon.
        let mut steps = 0;
        for i in 1..=8 {
            steps += overview.scrolled(pixels(-60.0), ms(16 * i));
        }
        assert_eq!(steps, 0);
        // Carrying on past 250 ms since the step: the next event steps.
        assert_eq!(overview.scrolled(pixels(-10.0), ms(260)), 1);
        // A touchpad's jitter the other way is nothing...
        assert_eq!(overview.scrolled(pixels(4.0), ms(280)), 0);
        // ...but turning back steps back at once, however soon.
        assert_eq!(overview.scrolled(pixels(8.0), ms(296)), -1);
        // And turning again steps on at once.
        assert_eq!(overview.scrolled(pixels(-12.0), ms(312)), 1);
        // A pause ends the scroll: the next bit steps at once.
        assert_eq!(overview.scrolled(pixels(-3.0), ms(800)), 1);
        // The wheel: a notch steps; carrying on, two more step again once
        // 250 ms have passed; a notch back steps back at once.
        assert_eq!(overview.scrolled(line(-1.0), ms(5000)), 1);
        assert_eq!(overview.scrolled(line(-1.0), ms(5150)), 0);
        assert_eq!(overview.scrolled(line(-1.0), ms(5300)), 1);
        assert_eq!(overview.scrolled(line(1.0), ms(5350)), -1);
        // Nothing scrolled is no scroll.
        assert_eq!(overview.scrolled(pixels(0.0), ms(9000)), 0);
    }

    #[test]
    fn cards_fit_and_are_centred() {
        assert!(cards(&[], ROOM, MIDDLE).is_empty());
        for n in 1..=12 {
            let windows: Vec<Size> = (0..n)
                .map(|i| Size::new(800.0 + 150.0 * (i % 4) as f32, 600.0 + 90.0 * (i % 3) as f32))
                .collect();
            let laid = cards(&windows, ROOM, MIDDLE);
            let left = laid.iter().map(|c| c.at.x).fold(f32::MAX, f32::min);
            let right = laid.iter().map(|c| c.at.x + c.width).fold(0.0, f32::max);
            let top = laid.iter().map(|c| c.at.y).fold(f32::MAX, f32::min);
            let bottom = laid
                .iter()
                .map(|c| c.at.y + card_height(c.picture))
                .fold(0.0, f32::max);
            assert!(left >= 0.0 && right <= ROOM.width + 1.0, "{n}: {left}..{right}");
            assert!(top >= 0.0 && bottom <= ROOM.height + 1.0, "{n}: {top}..{bottom}");
            assert!((top - (ROOM.height - bottom)).abs() <= 1.5, "{n} down");
            // Rows (cards sharing a line through their middles) centred.
            let mut rows: Vec<Vec<&Card>> = Vec::new();
            for card in &laid {
                let middle = card.at.y + card_height(card.picture) / 2.0;
                match rows.iter_mut().find(|r| {
                    let m = r[0].at.y + card_height(r[0].picture) / 2.0;
                    (m - middle).abs() <= 1.0
                }) {
                    Some(row) => row.push(card),
                    None => rows.push(vec![card]),
                }
            }
            for row in rows {
                let l = row.iter().map(|c| c.at.x).fold(f32::MAX, f32::min);
                let r = row.iter().map(|c| c.at.x + c.width).fold(0.0, f32::max);
                assert!((l - (ROOM.width - r)).abs() <= 1.5, "{n}: row of {}", row.len());
            }
        }
    }
}
