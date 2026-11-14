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
//! when it has no windows. The wheel steps through the workspaces, round
//! and round.
//!
//! The dock at the bottom holds the pinned apps, then the others running:
//! a press raises the app's last window, or starts it; a right press opens
//! its menu (its windows, a new one, its own actions, quitting). The grid
//! at its end shows every app. Typing searches the apps, at the top: their
//! icons stand in for the cards, Enter starts the pick. Escape closes the
//! menu, clears the search, leaves the apps, then closes the overview; so
//! do Super again and a press on nothing.
//!
//! The search does more than apps (`backend::search`: sums, files, commands,
//! `?` lists it all); for those it lists its finds in rows, not tiles. The
//! launcher (Super+/) is the same search on its own, lighter: a card at the
//! top over the windows, which stay, listing them first. With every app
//! (Super+A) it's the app library: their grid in the card, no windows.

use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use cosmic::{
    Element,
    iced::{
        self, Background, Color, Length, Point, Rectangle, Size, Vector,
        keyboard::{Key, Modifiers},
        mouse::ScrollDelta,
        platform_specific::shell::subsurface_widget::{Subsurface, SubsurfaceBuffer},
        window,
    },
    theme, widget,
};

use crate::{
    backend::{
        apps::{self, App},
        search::{self, Help, Hit, Mode},
        web::{self, Engine, Search},
        windows::{Window, Workspace},
    },
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
/// The search at the top.
const SEARCH_WIDTH: f32 = 520.0;
const SEARCH_HEIGHT: f32 = 46.0;
/// The dock at the bottom: an app's cell and icon, the gap between cells,
/// the running dot, the divider after the pinned apps, the padding; its
/// height.
const DOCK_CELL: f32 = 72.0;
const DOCK_ICON: f32 = 52.0;
const DOCK_GAP: f32 = 6.0;
const DOT: f32 = 5.0;
const DIVIDER: f32 = 1.0;
const DOCK_PAD: f32 = 10.0;
const DOCK_HEIGHT: f32 = DOCK_CELL + 2.0 * DOCK_PAD;
/// Under the dock, to the screen's edge.
const DOCK_BOTTOM: f32 = 12.0;
/// A dock menu: its width, a row's height, its padding, a separator's
/// room above and below, its gap above the dock; the dock's night, opaque.
const MENU_WIDTH: f32 = 280.0;
const MENU_ROW: f32 = 34.0;
const MENU_PAD: f32 = 6.0;
const MENU_SEP_PAD: f32 = 4.0;
const MENU_GAP: f32 = 8.0;
const MENU_BG: Color = palette::rgba(16, 21, 38, 0.98);
/// A window's row's app icon, in its colours.
const MENU_ICON: f32 = 18.0;
/// The search's results: a tile's size and icon, the gap between, the
/// panel's padding and its gap under the search, how many at most.
const RESULT: f32 = 128.0;
const RESULT_HEIGHT: f32 = 132.0;
const RESULT_ICON: f32 = 72.0;
const RESULT_GAP: f32 = 8.0;
const RESULTS_PAD: f32 = 16.0;
const RESULTS_GAP: f32 = 20.0;
const MAX_RESULTS: usize = 24;
/// At most this many apps a row in the panel.
const APPS_COLUMNS: usize = 8;
/// Results as rows: a row's height, its icon, the gap between, how many
/// at most (each has its Ctrl+number), the panel's width in the overview.
const ROW: f32 = 56.0;
const ROW_ICON: f32 = 36.0;
const ROW_GAP: f32 = 2.0;
const ROWS: usize = 9;
const ROWS_WIDTH: f32 = 680.0;
/// The launcher's card: its width, its gap under the panel, its padding.
const LAUNCHER_WIDTH: f32 = 760.0;
const LAUNCHER_TOP: f32 = 16.0;
const LAUNCHER_PAD: f32 = 14.0;
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
    /// The search's text, as edited in it.
    Query(String),
    /// Typed while the search didn't have the keys (a press elsewhere took
    /// them): added, and the search takes them again.
    Typed(String),
    /// The search takes the keys again.
    FocusSearch,
    /// The search's × pressed: cleared.
    Clear,
    /// Enter: the picked search result started.
    Enter,
    /// Ctrl and a digit: that result started (from 0).
    Choose(usize),
    /// Tab (back, true): a folder's pick completes the search; else the
    /// pick moves on.
    Tab(bool),
    /// The query's finds that take a look (`search::lookup`): due for this
    /// query, if it's still the one; found for it.
    Lookup(String),
    Hits(String, Vec<Hit>),
    /// A sum's answer copied; a file opened; a command run (in a terminal).
    Copy(String),
    Open(PathBuf),
    Run(String, bool),
    /// The search's pick moved on (back, negative).
    Pick(i32),
    /// The search's pick moved a row up (negative) or down.
    PickRow(i32),
    /// An app pressed, in the dock or the search (its id, or a window's
    /// app id when it has no entry): its last window raised, or started.
    App(String),
    /// A web search pressed (its URL): opened in the browser.
    Web(String),
    /// An engine picked: the search becomes its shortcut, ready for a query.
    Shortcut(String),
    /// The dock's grid pressed: every app shown, or the windows again.
    ToggleApps,
    /// An app's menu opened from the dock (its key), above its cell; from
    /// the search or every app, at the pointer; closed.
    DockMenu(String),
    ResultMenu(String),
    CloseMenu,
    /// From an app's menu: one of its windows raised (by id); one of its
    /// own actions (by place); all its windows closed.
    RaiseWindow(String),
    AppAction(String, usize),
    Quit(String),
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

/// An app's menu: whose (its key), and where the pointer was when it was
/// asked for from the search or every app; from the dock, none: it sits
/// above the app's cell.
pub struct Menu {
    pub key: String,
    pub at: Option<Point>,
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
    /// What's typed, and which of the apps it finds is picked.
    pub query: String,
    pub picked: usize,
    /// Every app shown (the dock's grid), and the app whose menu is open.
    pub all_apps: bool,
    pub menu: Option<Menu>,
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
    /// The launcher: the search alone, over the windows.
    pub light: bool,
    /// What looking the query up found (a sum's answer, files), for it or
    /// one of its kind before it.
    pub hits: Vec<Hit>,
}

impl Overview {
    /// Open on the windows in `order` (their ids, the one used last first);
    /// as the launcher if `light`.
    pub fn new(id: window::Id, order: Vec<String>, light: bool) -> Self {
        Self {
            id,
            order,
            light,
            hits: Vec::new(),
            cursor: Point::ORIGIN,
            hover: None,
            drag: None,
            query: String::new(),
            picked: 0,
            all_apps: false,
            menu: None,
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
        apps: &'a [App],
        favorites: &'a [String],
    ) -> Element<'a, Message> {
        widget::responsive(move |size| {
            if self.light {
                self.launcher(size, windows, apps)
            } else {
                self.laid_out(size, windows, groups, pictures, apps, favorites)
            }
        })
        .into()
    }

    /// The launcher: the search and what it lists in one card at the top
    /// middle, over the windows; as the app library, as wide as a full row
    /// of apps, their grid scrolling below the search. An app's menu by the
    /// pointer. A press off it closes it.
    fn launcher<'a>(
        &'a self,
        size: Size,
        windows: &'a [Window],
        apps: &'a [App],
    ) -> Element<'a, Message> {
        let listed = self.found(apps, windows);
        let grid = APPS_COLUMNS as f32 * RESULT + (APPS_COLUMNS as f32 - 1.0) * RESULT_GAP;
        let width = if self.all_apps {
            grid + 2.0 * LAUNCHER_PAD
        } else {
            LAUNCHER_WIDTH
        }
        .min(size.width - 2.0 * MARGIN);
        let mut parts = vec![self.search_bar(width - 2.0 * LAUNCHER_PAD)];
        if !listed.is_empty() {
            let picked = self.picked.min(listed.len() - 1);
            parts.push(if self.rows(&listed) {
                results_list(&listed, picked, apps)
            } else {
                let room = size.height - LAUNCHER_TOP - MARGIN - SEARCH_HEIGHT - 3.0 * LAUNCHER_PAD;
                results_grid(&listed, picked, room, apps).0
            });
        }
        let card = widget::container(widget::column::with_children(parts).spacing(LAUNCHER_PAD))
            .padding(LAUNCHER_PAD)
            .width(Length::Fixed(width))
            .class(panel_style());
        let at = Point::new(((size.width - width) / 2.0).round(), LAUNCHER_TOP);
        let mut placed = Placed::new(size).occluding().push(card, at, width);
        if let Some(asked) = &self.menu {
            let ordered: Vec<&Window> = windows.iter().collect();
            let item = entry(&asked.key, apps, &ordered);
            placed = with_menu(
                placed,
                size,
                &item,
                asked.at,
                size.width / 2.0,
                size.height - MARGIN,
            );
        }
        placed.on_press_outside(Message::Close).into()
    }

    /// What the search lists, in order: by the query's prefix (see
    /// `search`); else the windows it names (in the launcher, every one
    /// before anything's typed), a sum's answer, the web searches its
    /// shortcut asks for or the engines it names, then the apps (those it
    /// finds, or every one). As rows, no more than have a Ctrl+number.
    pub fn found<'a>(&'a self, apps: &'a [App], windows: &'a [Window]) -> Vec<Found<'a>> {
        let hits = |sums: bool| {
            self.hits
                .iter()
                .filter(move |h| matches!(h, Hit::Calc(..)) == sums)
                .cloned()
                .map(Found::Hit)
        };
        let mut listed: Vec<Found<'a>> = match search::mode(&self.query) {
            Mode::Plain => {
                let word = self.query.trim().to_lowercase();
                let windows = windows
                    .iter()
                    .filter(|w| {
                        self.light
                            && !self.all_apps
                            && (w.title.to_lowercase().contains(&word)
                                || owner(apps, &w.app_id)
                                    .is_some_and(|a| a.name.to_lowercase().contains(&word)))
                    })
                    .map(Found::Window);
                windows
                    .chain(hits(true).filter(|_| search::sum_like(&self.query)))
                    .chain(web::searches(&self.query).into_iter().map(Found::Web))
                    .chain(web::engines(&self.query).into_iter().map(Found::Engine))
                    .chain(
                        shown_apps(apps, &self.query, self.all_apps)
                            .into_iter()
                            .map(Found::App),
                    )
                    .collect()
            }
            Mode::Calc(_) => hits(true).collect(),
            Mode::Files(_) | Mode::Find(_) | Mode::Recent(_) => hits(false).collect(),
            Mode::Run(command, terminal) => (!command.is_empty())
                .then(|| Found::Run(command.to_owned(), terminal))
                .into_iter()
                .collect(),
            Mode::Help(word) => search::help(word).map(Found::Help).collect(),
        };
        if self.rows(&listed) {
            listed.truncate(ROWS);
        }
        listed
    }

    /// Whether the finds are listed as rows: always in the launcher (not
    /// the app library); else once there's more than apps and web searches.
    pub fn rows(&self, listed: &[Found]) -> bool {
        (self.light && !self.all_apps)
            || listed
                .iter()
                .any(|f| !matches!(f, Found::App(_) | Found::Web(_) | Found::Engine(_)))
    }

    /// How many finds a row of the list holds.
    pub fn columns(&self, listed: &[Found]) -> usize {
        if self.rows(listed) {
            1
        } else {
            columns(listed.len())
        }
    }

    fn laid_out<'a>(
        &'a self,
        size: Size,
        windows: &'a [Window],
        groups: &'a [Vec<Workspace>],
        pictures: &'a HashMap<String, SubsurfaceBuffer>,
        apps: &'a [App],
        favorites: &'a [String],
    ) -> Element<'a, Message> {
        let spaces = group(groups, windows);
        let mut shown = shown(windows, spaces);
        shown.sort_by_key(|w| {
            self.order
                .iter()
                .position(|id| *id == w.id)
                .unwrap_or(usize::MAX)
        });

        // Searching, or every app: the windows and workspaces step aside.
        let listed = self.found(apps, windows);
        let searching = self.all_apps || !self.query.trim().is_empty();

        // The workspaces down the left, straight on the desktop like the
        // cards, each as large as the column allows; not while searching.
        let n = spaces.len().max(1) as f32;
        let tall = ((size.height - 2.0 * MARGIN - (n - 1.0) * SPACE_GAP) / n).min(SPACE_HEIGHT);
        let side = SPACE;
        // Nothing hovers through what lies over it (a menu over the cards).
        let mut placed = Placed::new(size).occluding();
        // The search and the dock first, always: a child's state goes by its
        // place, and the search's (its text, its focus) must not move when
        // the workspaces step aside for a search.
        // The search, centred on the screen.
        let search_at = Point::new(((size.width - SEARCH_WIDTH) / 2.0).round(), MARGIN);
        placed = placed.push(self.search_bar(SEARCH_WIDTH), search_at, SEARCH_WIDTH);

        // The dock, centred on the screen at the bottom; in the order the
        // cards keep, so it doesn't shuffle either.
        let mut ordered: Vec<&Window> = windows.iter().collect();
        ordered.sort_by_key(|w| {
            self.order
                .iter()
                .position(|id| *id == w.id)
                .unwrap_or(usize::MAX)
        });
        let items = dock(apps, favorites, &ordered);
        let (cells, dock_width) = dock_cells(&items);
        let dock_at = Point::new(
            ((size.width - dock_width) / 2.0).round(),
            size.height - DOCK_BOTTOM - DOCK_HEIGHT,
        );
        placed = placed.push(dock_bar(&items, self.all_apps), dock_at, dock_width);

        let mut y = MARGIN;
        for space in spaces.iter().filter(|_| !searching) {
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
        // column and between the search and the dock, each row centred on
        // the screen unless that would put it over the column.
        let left = MARGIN + side + GAP;
        let top = MARGIN + SEARCH_HEIGHT + GAP;
        let room = Size::new(
            size.width - left - MARGIN,
            size.height - top - DOCK_BOTTOM - DOCK_HEIGHT - GAP,
        );
        let centre = size.width / 2.0 - left;

        // Searching, as GNOME does: the windows and workspaces step aside,
        // and the apps found sit in a row under the search.
        if searching {
            if !listed.is_empty() {
                let picked = self.picked.min(listed.len() - 1);
                let panel_top = MARGIN + SEARCH_HEIGHT + RESULTS_GAP;
                let room = dock_at.y - GAP - panel_top;
                let (panel, width) = if self.rows(&listed) {
                    let list = widget::container(results_list(&listed, picked, apps))
                        .padding(RESULTS_PAD / 2.0)
                        .width(Length::Fixed(ROWS_WIDTH))
                        .class(panel_style());
                    (list.into(), ROWS_WIDTH)
                } else {
                    results_panel(&listed, picked, room, apps)
                };
                placed = placed.push(
                    panel,
                    Point::new(((size.width - width) / 2.0).round(), panel_top),
                    width,
                );
            }
        } else {
            // Each window's size, from its picture: the cards keep the
            // windows' shapes, and their sizes against each other.
            let sizes: Vec<Size> = shown
                .iter()
                .map(|w| {
                    pictures
                        .get(&w.id)
                        .map_or(UNKNOWN, |b| Size::new(b.width() as f32, b.height() as f32))
                })
                .collect();
            for card in cards(&sizes, room, centre) {
                let at = card.at + Vector::new(left, top);
                placed = placed.push(
                    self.card(shown[card.index], pictures, card.width, card.picture),
                    at,
                    card.width,
                );
            }
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
            // The pointer passes through it to the workspace it's over.
            placed = placed.push_passive(
                widget::container(ghost)
                    .center_x(Length::Fixed(GHOST))
                    .center_y(Length::Fixed(tall)),
                self.cursor - Vector::new(GHOST / 2.0, tall / 2.0),
                GHOST,
            );
        }

        // An app's menu over everything: above its dock cell, or by the
        // pointer (below it, unless there's no room). A press anywhere else
        // (on the see-through sheet under it) just closes it.
        if let Some(asked) = &self.menu {
            let item = entry(&asked.key, apps, &ordered);
            let cell = items
                .iter()
                .position(|i| i.key == asked.key)
                .map_or(size.width / 2.0, |i| dock_at.x + cells[i] + DOCK_CELL / 2.0);
            placed = with_menu(placed, size, &item, asked.at, cell, dock_at.y);
        }

        // See-through: cosmic-comp draws no windows while it shows, so
        // what's behind is the desktop.
        widget::mouse_area(placed.on_press_outside(Message::Close))
            .on_scroll(Message::Scroll)
            .into()
    }

    /// The search: a text input like any other (selecting, pasting,
    /// Ctrl+Backspace), a × to clear it once something's in it.
    fn search_bar<'a>(&self, width: f32) -> Element<'a, Message> {
        let icon = widget::container(ui::icon("search", 18))
            .padding([0, 4, 0, 14])
            .class(theme::Container::custom(|_| widget::container::Style {
                icon_color: Some(palette::MUTED),
                ..Default::default()
            }));
        let input = widget::text_input("Type to search, or ? for more", self.query.clone())
            .id(SEARCH.clone())
            .on_input(Message::Query)
            .on_submit(|_| Message::Enter)
            .leading_icon(icon.into())
            .size(15)
            .padding([13, 14])
            .width(Length::Fixed(width))
            .style(search_style());
        // Always there, so the input's insides keep their shape (libcosmic's
        // input panicked when its trailing icon went): unseen and inert
        // while there's nothing to clear.
        // Drawn, not an icon: inside the input an icon's colour came out
        // near black, and it showed with nothing to clear.
        let empty = self.query.is_empty();
        let x = widget::canvas(Cross {
            color: (!empty).then_some(palette::MUTED),
        })
        .width(Length::Fixed(24.0))
        .height(Length::Fixed(24.0));
        let clear = widget::button::custom(x)
            .width(Length::Fixed(24.0))
            .height(Length::Fixed(24.0))
            .padding(0)
            .class(hover_tile(false))
            .on_press_maybe((!empty).then_some(Message::Clear));
        let input = input.trailing_icon(widget::container(clear).padding([0, 12, 0, 0]).into());
        widget::container(input)
            .center_y(Length::Fixed(SEARCH_HEIGHT))
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
        let label = switcher::label(w, &title, Some(close_button(&w.id)));
        let bar = widget::container(label)
            .padding([0.0, TITLE_PAD])
            .width(Length::Fixed(width))
            .center_y(Length::Fixed(TITLE))
            .class(fill(if hovered { TITLE_HOVER } else { TITLE_BG }, None));
        let size = picture;
        let picture: Element<'a, Message> = match pictures.get(&w.id) {
            // Filling a box of its own shape: nothing round it.
            // Below this surface, as the workspaces' are: a dock menu can lie
            // over it.
            Some(buffer) => Subsurface::new(buffer.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .alpha(if dragged { 0.4 } else { 1.0 })
                .z(-1)
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
        .class(theme::Container::custom(move |_| {
            widget::container::Style {
                icon_color: Some(color),
                ..Default::default()
            }
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

/// The search's ×, in `color`; nothing without one.
struct Cross {
    color: Option<Color>,
}

impl<M> widget::canvas::Program<M, cosmic::Theme, cosmic::Renderer> for Cross {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<widget::canvas::Geometry<cosmic::Renderer>> {
        use widget::canvas::{Frame, LineCap, Path, Stroke};
        let Some(color) = self.color else {
            return Vec::new();
        };
        let mut frame = Frame::new(renderer, bounds.size());
        let (centre, arm) = (frame.center(), 4.5);
        let cross = Path::new(|path| {
            path.move_to(centre + Vector::new(-arm, -arm));
            path.line_to(centre + Vector::new(arm, arm));
            path.move_to(centre + Vector::new(arm, -arm));
            path.line_to(centre + Vector::new(-arm, arm));
        });
        frame.stroke(
            &cross,
            Stroke::default()
                .with_color(color)
                .with_width(1.6)
                .with_line_cap(LineCap::Round),
        );
        vec![frame.into_geometry()]
    }
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

/// The search's text input, to focus it. Unique, not named: libcosmic's
/// iced lifts a named widget's state out on every rebuild and hands it back
/// only where it finds the same name, so when the overview changed shape
/// round the search (the workspaces coming back on Backspace) the input
/// was laid out with no state and the daemon panicked.
pub static SEARCH: std::sync::LazyLock<widget::Id> = std::sync::LazyLock::new(widget::Id::unique);

/// The keys the search didn't take, as the overview's messages: Up and
/// Down move the pick a row, Tab one (and Left and Right when the search
/// isn't focused); Enter starts it; typing while the search isn't focused
/// goes to it.
pub fn key(key: &Key, modifiers: Modifiers) -> Option<Message> {
    use iced::keyboard::key::Named;
    if modifiers.control() && !modifiers.alt() && !modifiers.logo() {
        return chosen(key).map(Message::Choose);
    }
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    match key {
        Key::Character(c) => Some(Message::Typed(c.to_string())),
        Key::Named(Named::Backspace | Named::Delete) => Some(Message::FocusSearch),
        Key::Named(Named::Enter) => Some(Message::Enter),
        Key::Named(Named::ArrowRight) => Some(Message::Pick(1)),
        Key::Named(Named::ArrowLeft) => Some(Message::Pick(-1)),
        Key::Named(Named::ArrowDown) => Some(Message::PickRow(1)),
        Key::Named(Named::ArrowUp) => Some(Message::PickRow(-1)),
        Key::Named(Named::Tab) => Some(Message::Tab(modifiers.shift())),
        _ => None,
    }
}

/// The result a digit picks with Ctrl (1 the first), if it's 1 to 9.
pub fn chosen(key: &Key) -> Option<usize> {
    match key {
        Key::Character(c) => c
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=ROWS).contains(n))
            .map(|n| n - 1),
        _ => None,
    }
}

/// The search's look: the title bars' night glass as a pill, the accent for
/// what's selected.
fn search_style() -> theme::TextInput {
    let appearance = |edge: f32| widget::text_input::Appearance {
        background: Background::Color(TITLE_BG),
        border_radius: (SEARCH_HEIGHT / 2.0).into(),
        border_offset: None,
        border_width: 1.0,
        border_color: palette::alpha(palette::TEXT, edge),
        label_color: palette::TEXT,
        placeholder_color: palette::FAINT,
        selected_text_color: palette::TEXT,
        icon_color: Some(palette::MUTED),
        text_color: Some(palette::TEXT),
        selected_fill: palette::alpha(palette::ACCENT, 0.35),
    };
    theme::TextInput::Custom {
        active: Box::new(move |_| appearance(0.12)),
        error: Box::new(move |_| appearance(0.12)),
        hovered: Box::new(move |_| appearance(0.2)),
        focused: Box::new(move |_| appearance(0.28)),
        disabled: Box::new(move |_| appearance(0.12)),
    }
}

/// The apps the search shows for `query`, best first, as many as fit.
pub fn search_results<'a>(apps: &'a [App], query: &str) -> Vec<&'a App> {
    let mut found = apps::search(apps, query);
    found.truncate(MAX_RESULTS);
    found
}

/// The apps the panel shows: those the search finds, else every app (by
/// name) when asked for, else none.
pub fn shown_apps<'a>(apps: &'a [App], query: &str, all: bool) -> Vec<&'a App> {
    if !query.trim().is_empty() {
        search_results(apps, query)
    } else if all {
        let mut every: Vec<&App> = apps.iter().collect();
        every.sort_by_key(|a| a.name.to_lowercase());
        every
    } else {
        Vec::new()
    }
}

/// Something the search lists: a window (in the launcher), an app, a web
/// search a shortcut asks for or an engine a word names, a sum's answer or
/// a file, a command, or what a prefix does.
pub enum Found<'a> {
    Window(&'a Window),
    App(&'a App),
    Web(Search),
    Engine(Engine),
    Hit(Hit),
    Run(String, bool),
    Help(&'static Help),
}

impl<'a> Found<'a> {
    /// What a press, or Enter on it, does.
    pub fn press(&self) -> Message {
        match self {
            Found::Window(w) => Message::RaiseWindow(w.id.clone()),
            Found::App(app) => Message::App(app.id.clone()),
            Found::Web(search) => Message::Web(search.url.clone()),
            Found::Engine(engine) => Message::Shortcut(engine.shortcut.clone()),
            Found::Hit(Hit::Calc(_, answer)) => Message::Copy(answer.clone()),
            Found::Hit(Hit::File(path, _)) => Message::Open(path.clone()),
            Found::Run(command, terminal) => Message::Run(command.clone(), *terminal),
            Found::Help(help) => Message::Shortcut(help.prefix.to_owned()),
        }
    }

    /// Its icon at `size`, its name, and what it is.
    fn shown(&self, apps: &'a [App], size: f32) -> (Element<'a, Message>, String, String) {
        match self {
            Found::Window(w) => {
                let app = owner(apps, &w.app_id);
                let name = app.map_or_else(|| switcher::name(&w.app_id), |a| a.name.clone());
                let title = if w.title.is_empty() {
                    name.clone()
                } else {
                    w.title.clone()
                };
                (
                    app_icon(app, Some(&w.app_id), size),
                    title,
                    format!("{name} window"),
                )
            }
            Found::App(app) => {
                let about = if app.about.is_empty() {
                    "App".into()
                } else {
                    app.about.clone()
                };
                (app_icon(Some(app), None, size), app.name.clone(), about)
            }
            Found::Web(search) => (
                web_icon(search.icon.as_deref(), size),
                search.label.clone(),
                search.url.trim_start_matches("https://").to_owned(),
            ),
            Found::Engine(engine) => (
                web_icon(engine.icon.as_deref(), size),
                engine.name.clone(),
                format!("{}<keywords>", engine.shortcut),
            ),
            Found::Hit(Hit::Calc(sum, answer)) => (
                named_icon("accessories-calculator", size),
                answer.clone(),
                format!("{sum}   ·   Enter copies it"),
            ),
            Found::Hit(Hit::File(path, dir)) => (
                named_icon(if *dir { "folder" } else { "text-x-generic" }, size),
                path.file_name()
                    .map_or_else(|| search::tilde(path), |n| n.to_string_lossy().into_owned()),
                path.parent().map(search::tilde).unwrap_or_default(),
            ),
            Found::Run(command, terminal) => (
                named_icon(
                    if *terminal {
                        "utilities-terminal"
                    } else {
                        "system-run"
                    },
                    size,
                ),
                command.clone(),
                if *terminal {
                    "Run in a terminal"
                } else {
                    "Run in the background"
                }
                .into(),
            ),
            Found::Help(help) => (
                named_icon(help.icon, size),
                help.name.into(),
                format!("{}   ·   {}", help.syntax, help.example),
            ),
        }
    }
}

/// The app a window with this app id is, if it has an entry.
fn owner<'a>(apps: &'a [App], app_id: &str) -> Option<&'a App> {
    apps.iter().find(|a| a.owns(app_id))
}

/// How many apps a row of the panel holds, for `n` of them.
pub fn columns(n: usize) -> usize {
    APPS_COLUMNS.min(n).max(1)
}

/// A dock entry: an app pinned or running, with its windows.
pub struct DockItem<'a> {
    /// What a press sends: the app's id, or a window's app id if no desktop
    /// entry is known for it.
    pub key: String,
    pub app: Option<&'a App>,
    pub windows: Vec<&'a Window>,
    pub pinned: bool,
}

/// The dock's apps: the pinned ones in their order, then the others with
/// windows, in the order their windows come in `windows`.
pub fn dock<'a>(
    apps: &'a [App],
    favorites: &[String],
    windows: &[&'a Window],
) -> Vec<DockItem<'a>> {
    let mut items: Vec<DockItem<'a>> = favorites
        .iter()
        .map(|id| {
            let app = apps.iter().find(|a| &a.id == id);
            let windows = windows
                .iter()
                .copied()
                .filter(|w| app.map_or(w.app_id.eq_ignore_ascii_case(id), |a| a.owns(&w.app_id)))
                .collect();
            DockItem {
                key: id.clone(),
                app,
                windows,
                pinned: true,
            }
        })
        .collect();
    for &w in windows {
        if items.iter().any(|i| i.windows.iter().any(|x| x.id == w.id)) {
            continue;
        }
        let app = apps.iter().find(|a| a.owns(&w.app_id));
        let key = app.map_or_else(|| w.app_id.to_lowercase(), |a| a.id.clone());
        match items.iter_mut().find(|i| i.key == key) {
            Some(item) => item.windows.push(w),
            None => items.push(DockItem {
                key,
                app,
                windows: vec![w],
                pinned: false,
            }),
        }
    }
    items
}

/// An app (by key) with its windows, as a menu needs it, wherever it was
/// asked for.
fn entry<'a>(key: &str, apps: &'a [App], windows: &[&'a Window]) -> DockItem<'a> {
    let app = apps.iter().find(|a| a.id == key);
    DockItem {
        key: key.to_owned(),
        app,
        windows: windows
            .iter()
            .copied()
            .filter(|w| app.map_or(w.app_id.eq_ignore_ascii_case(key), |a| a.owns(&w.app_id)))
            .collect(),
        pinned: false,
    }
}

/// The dock's cells: each app's left edge from the dock's, and the dock's
/// width with the grid at its end. Dividers come after the pinned apps when
/// others run, and before the grid.
fn dock_cells(items: &[DockItem]) -> (Vec<f32>, f32) {
    let mut x = DOCK_PAD;
    let mut cells = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        if i > 0 && !item.pinned && items[i - 1].pinned {
            x += DIVIDER + DOCK_GAP;
        }
        cells.push(x);
        x += DOCK_CELL + DOCK_GAP;
    }
    if !items.is_empty() {
        x += DIVIDER + DOCK_GAP;
    }
    (cells, x + DOCK_CELL + DOCK_PAD)
}

/// The dock: each app's icon, a dot under it while it has windows, a menu
/// on a right press; the grid of every app at the end, lit while it shows.
fn dock_bar<'a>(items: &[DockItem<'a>], all_apps: bool) -> Element<'a, Message> {
    let divider = || -> Element<'a, Message> {
        widget::container(widget::space::horizontal())
            .width(Length::Fixed(DIVIDER))
            .height(Length::Fixed(DOCK_CELL * 0.6))
            .class(fill_round(palette::SEP, 0.0))
            .into()
    };
    let mut cells: Vec<Element<'a, Message>> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 && !item.pinned && items[i - 1].pinned {
            cells.push(divider());
        }
        let running = if item.windows.is_empty() {
            Color::TRANSPARENT
        } else {
            palette::TEXT
        };
        let dot = widget::container(widget::space::horizontal())
            .width(Length::Fixed(DOT))
            .height(Length::Fixed(DOT))
            .class(fill_round(running, DOT / 2.0));
        let icon = app_icon(
            item.app,
            item.windows.first().map(|w| w.app_id.as_str()),
            DOCK_ICON,
        );
        let inside = widget::column::with_children(vec![
            widget::container(icon)
                .center_x(Length::Fill)
                .center_y(Length::Fixed(DOCK_CELL - DOT - 8.0))
                .into(),
            dot.into(),
        ])
        .spacing(2)
        .align_x(iced::Alignment::Center);
        let button = widget::button::custom(inside)
            .width(Length::Fixed(DOCK_CELL))
            .height(Length::Fixed(DOCK_CELL))
            .padding(0)
            .class(hover_tile(false))
            .on_press(Message::App(item.key.clone()));
        cells.push(
            widget::mouse_area(button)
                .on_right_press(Message::DockMenu(item.key.clone()))
                .into(),
        );
    }
    if !items.is_empty() {
        cells.push(divider());
    }
    cells.push(
        widget::button::custom(widget::container(ui::icon("grid-dots", 30)).center(Length::Fill))
            .width(Length::Fixed(DOCK_CELL))
            .height(Length::Fixed(DOCK_CELL))
            .padding(0)
            .class(hover_tile(all_apps))
            .on_press(Message::ToggleApps)
            .into(),
    );
    let row = widget::row::with_children(cells)
        .spacing(DOCK_GAP)
        .align_y(iced::Alignment::Center);
    widget::container(row)
        .padding(DOCK_PAD)
        .center_y(Length::Fixed(DOCK_HEIGHT))
        .class(theme::Container::custom(|_| widget::container::Style {
            background: Some(Background::Color(TITLE_BG)),
            border: iced::Border {
                radius: 22.0.into(),
                width: 1.0,
                color: palette::alpha(palette::TEXT, 0.12),
            },
            ..Default::default()
        }))
        .into()
}

/// An app's menu laid over everything: by the pointer (`at`: below it,
/// unless there's no room) or, from the dock, centred on `middle` just
/// above `above`. A press anywhere else (on the see-through sheet under
/// it) just closes it.
fn with_menu<'a>(
    placed: Placed<'a, Message>,
    size: Size,
    item: &DockItem<'a>,
    at: Option<Point>,
    middle: f32,
    above: f32,
) -> Placed<'a, Message> {
    let (menu, height) = dock_menu(item);
    let (middle, y) = match at {
        Some(at) if at.y + MENU_GAP + height > size.height - MARGIN => {
            (at.x, at.y - MENU_GAP - height)
        }
        Some(at) => (at.x, at.y + MENU_GAP),
        None => (middle, above - MENU_GAP - height),
    };
    let x = (middle - MENU_WIDTH / 2.0).clamp(MARGIN, size.width - MARGIN - MENU_WIDTH);
    let sheet = widget::mouse_area(
        widget::container(widget::space::horizontal())
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .on_press(Message::CloseMenu)
    .on_right_press(Message::CloseMenu);
    placed.push(sheet, Point::ORIGIN, size.width).push(
        menu,
        Point::new(x.round(), y.max(MARGIN).round()),
        MENU_WIDTH,
    )
}

/// An app's menu, and its height: its windows (to go to one); its own
/// actions from its desktop entry; quitting while it runs. Thin lines
/// between those.
fn dock_menu<'a>(item: &DockItem<'a>) -> (Element<'a, Message>, f32) {
    let name = item
        .app
        .map(|a| a.name.clone())
        .unwrap_or_else(|| switcher::name(&item.key));
    let windows: Vec<Element<'a, Message>> = item
        .windows
        .iter()
        .map(|w| {
            let title = if w.title.is_empty() {
                name.clone()
            } else {
                w.title.clone()
            };
            let icon = app_icon(item.app, Some(w.app_id.as_str()), MENU_ICON);
            menu_item(Some(icon), title, Message::RaiseWindow(w.id.clone()))
        })
        .collect();
    let launch: Vec<Element<'a, Message>> = item
        .app
        .into_iter()
        .flat_map(|app| app.actions.iter().enumerate())
        .map(|(i, (action, _))| {
            menu_item(
                None,
                action.clone(),
                Message::AppAction(item.key.clone(), i),
            )
        })
        .collect();
    let quit: Vec<Element<'a, Message>> = if item.windows.is_empty() {
        Vec::new()
    } else {
        vec![menu_item(
            None,
            "Quit".into(),
            Message::Quit(item.key.clone()),
        )]
    };
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    let mut height = 2.0 * MENU_PAD;
    for group in [windows, launch, quit]
        .into_iter()
        .filter(|g| !g.is_empty())
    {
        if !rows.is_empty() {
            rows.push(
                widget::container(
                    widget::container(widget::space::horizontal())
                        .width(Length::Fill)
                        .height(Length::Fixed(1.0))
                        .class(fill_round(palette::alpha(palette::TEXT, 0.1), 0.0)),
                )
                .padding([MENU_SEP_PAD, 8.0])
                .into(),
            );
            height += 1.0 + 2.0 * MENU_SEP_PAD;
        }
        height += group.len() as f32 * MENU_ROW;
        rows.extend(group);
    }
    let card = widget::container(widget::column::with_children(rows))
        .padding(MENU_PAD)
        .width(Length::Fixed(MENU_WIDTH))
        .class(theme::Container::custom(|_| widget::container::Style {
            background: Some(Background::Color(MENU_BG)),
            text_color: Some(palette::TEXT),
            icon_color: Some(palette::MUTED),
            border: iced::Border {
                radius: 14.0.into(),
                width: 1.0,
                color: palette::alpha(palette::TEXT, 0.12),
            },
            shadow: iced::Shadow {
                color: palette::alpha(Color::BLACK, 0.35),
                offset: Vector::new(0.0, 6.0),
                blur_radius: 18.0,
            },
            ..Default::default()
        }));
    (card.into(), height)
}

/// A menu's row: the app's icon on a window's row (else its room, so the
/// labels line up), the label cut to fit, a soft shade under the pointer.
fn menu_item<'a>(
    icon: Option<Element<'a, Message>>,
    label: String,
    on_press: Message,
) -> Element<'a, Message> {
    let icon: Element<'a, Message> = widget::container(icon.unwrap_or_else(|| {
        widget::space::horizontal()
            .width(Length::Fixed(MENU_ICON))
            .into()
    }))
    .center_x(Length::Fixed(MENU_ICON))
    .into();
    // Filling the row's height, so the label and icon sit in its middle
    // (a shrunk row sat at the top of the button).
    let row = widget::row::with_children(vec![
        icon,
        ui::marquee::ticker(label, ui::text::body, false),
    ])
    .spacing(10)
    .height(Length::Fill)
    .align_y(iced::Alignment::Center);
    let style = |hovered: bool| widget::button::Style {
        background: Some(Background::Color(if hovered {
            palette::alpha(palette::TEXT, 0.08)
        } else {
            Color::TRANSPARENT
        })),
        border_radius: 8.0.into(),
        icon_color: Some(palette::MUTED),
        text_color: Some(palette::TEXT),
        ..widget::button::Style::new()
    };
    widget::button::custom(row)
        .width(Length::Fill)
        .height(Length::Fixed(MENU_ROW))
        .padding([0, 10])
        .class(theme::Button::Custom {
            active: Box::new(move |_, _| style(false)),
            disabled: Box::new(move |_| style(false)),
            hovered: Box::new(move |_, _| style(true)),
            pressed: Box::new(move |_, _| style(true)),
        })
        .on_press(on_press)
        .into()
}

/// The apps listed, as GNOME shows them: large icons with their names in
/// rows on a dark rounded panel, the pick on a lighter tile, scrolling when
/// they don't fit `room`'s height. With the panel's width.
fn results_panel<'a>(
    listed: &[Found<'a>],
    picked: usize,
    room: f32,
    apps: &'a [App],
) -> (Element<'a, Message>, f32) {
    let (grid, width) = results_grid(listed, picked, room - 2.0 * RESULTS_PAD, apps);
    let panel = widget::container(grid)
        .padding(RESULTS_PAD)
        .class(panel_style())
        .into();
    (panel, width + 2.0 * RESULTS_PAD)
}

/// The finds' tiles in rows, scrolling when they're taller than `room`;
/// with their width.
fn results_grid<'a>(
    listed: &[Found<'a>],
    picked: usize,
    room: f32,
    apps: &'a [App],
) -> (Element<'a, Message>, f32) {
    let cols = columns(listed.len());
    let rows: Vec<Element<'a, Message>> = listed
        .chunks(cols)
        .enumerate()
        .map(|(r, chunk)| {
            let tiles: Vec<Element<'a, Message>> = chunk
                .iter()
                .enumerate()
                .map(|(c, item)| {
                    let (icon, label, _) = item.shown(apps, RESULT_ICON);
                    let menu = match item {
                        Found::App(app) => Some(Message::ResultMenu(app.id.clone())),
                        _ => None,
                    };
                    let inside = widget::column::with_children(vec![
                        icon,
                        ui::marquee::ticker(label, ui::text::body, true),
                    ])
                    .spacing(10)
                    .align_x(iced::Alignment::Center);
                    let tile =
                        widget::button::custom(widget::container(inside).center(Length::Fill))
                            .width(Length::Fixed(RESULT))
                            .height(Length::Fixed(RESULT_HEIGHT))
                            .padding(8)
                            .class(result_style(r * cols + c == picked))
                            .on_press(item.press());
                    let mut area = widget::mouse_area(tile);
                    if let Some(menu) = menu {
                        area = area.on_right_press(menu);
                    }
                    area.into()
                })
                .collect();
            widget::row::with_children(tiles).spacing(RESULT_GAP).into()
        })
        .collect();
    let n_rows = rows.len() as f32;
    let tall = n_rows * RESULT_HEIGHT + (n_rows - 1.0).max(0.0) * RESULT_GAP;
    let grid = widget::column::with_children(rows).spacing(RESULT_GAP);
    let width = cols as f32 * RESULT + (cols as f32 - 1.0) * RESULT_GAP;
    let inner: Element<'a, Message> = if tall > room {
        widget::scrollable(grid)
            .height(Length::Fixed(room.max(RESULT_HEIGHT)))
            .into()
    } else {
        grid.into()
    };
    (inner, width)
}

/// The results' panel, and the launcher's card: night glass, rounded.
fn panel_style() -> theme::Container<'static> {
    theme::Container::custom(|_| widget::container::Style {
        background: Some(Background::Color(TITLE_BG)),
        text_color: Some(palette::TEXT),
        border: iced::Border {
            radius: 24.0.into(),
            width: 1.0,
            color: palette::alpha(palette::TEXT, 0.12),
        },
        ..Default::default()
    })
}

/// The results as rows, the way COSMIC's launcher listed them: each an
/// icon, a name over what it is, and its Ctrl+number; the pick lighter.
fn results_list<'a>(listed: &[Found<'a>], picked: usize, apps: &'a [App]) -> Element<'a, Message> {
    let rows: Vec<Element<'a, Message>> = listed
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let (icon, name, about) = item.shown(apps, ROW_ICON);
            let words = widget::column::with_children(vec![
                ui::marquee::ticker(name, ui::text::row, false),
                ui::text::row_sub(about)
                    .class(theme::Text::Custom(ui::dimmed))
                    .into(),
            ])
            .spacing(3)
            .width(Length::Fill);
            let row = widget::row::with_children(vec![
                widget::container(icon)
                    .center_x(Length::Fixed(ROW_ICON))
                    .into(),
                words.into(),
                ui::text::small(format!("Ctrl + {}", i + 1))
                    .class(theme::Text::Custom(ui::dimmed))
                    .into(),
            ])
            .spacing(14)
            .height(Length::Fill)
            .align_y(iced::Alignment::Center);
            widget::button::custom(row)
                .width(Length::Fill)
                .height(Length::Fixed(ROW))
                .padding([0, 14])
                .class(result_style(i == picked))
                .on_press(item.press())
                .into()
        })
        .collect();
    widget::column::with_children(rows).spacing(ROW_GAP).into()
}

/// An icon from the theme by name, looked up at twice the size (see
/// `app_icon`).
fn named_icon<'a>(name: &str, size: f32) -> Element<'a, Message> {
    let px = size as u16;
    widget::icon(
        widget::icon::from_name(name)
            .size(px.saturating_mul(2))
            .handle(),
    )
    .size(px)
    .into()
}

/// A search result's tile: lighter when picked, faint under the pointer.
fn result_style(picked: bool) -> theme::Button {
    let style = move |hovered: bool| widget::button::Style {
        background: Some(Background::Color(if picked {
            palette::alpha(palette::TEXT, 0.16)
        } else if hovered {
            palette::alpha(palette::TEXT, 0.08)
        } else {
            Color::TRANSPARENT
        })),
        border_radius: 16.0.into(),
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

/// A web search's icon: its engine's from Papirus (`CABIN_WEB_ICONS`, set
/// by the package's wrapper), else a globe.
fn web_icon<'a>(name: Option<&str>, size: f32) -> Element<'a, Message> {
    let path = name
        .zip(std::env::var_os("CABIN_WEB_ICONS"))
        .map(|(name, dir)| std::path::Path::new(&dir).join(format!("{name}.svg")));
    match path.filter(|p| p.exists()) {
        Some(path) => widget::icon(widget::icon::from_path(path))
            .size(size as u16)
            .into(),
        None => widget::container(ui::icon("world", (size * 0.8) as u16))
            .center_x(Length::Fixed(size))
            .center_y(Length::Fixed(size))
            .into(),
    }
}

/// An app's icon: its entry's (a name in the icon theme, or a file), else
/// the mark its window's app id gets.
fn app_icon<'a>(app: Option<&App>, app_id: Option<&str>, size: f32) -> Element<'a, Message> {
    let px = size as u16;
    match app {
        Some(app) if app.icon.starts_with('/') => {
            widget::icon(widget::icon::from_path(app.icon.clone().into()))
                .size(px)
                .into()
        }
        // Looked up at twice the size: the screen is scaled, and a raster
        // found at the size it's drawn at comes out blurred.
        Some(app) => widget::icon(
            widget::icon::from_name(app.icon.clone())
                .size(px.saturating_mul(2))
                .handle(),
        )
        .size(px)
        .into(),
        // The mark is drawn at 0.6 of what it's given.
        None => switcher::mark(app_id.unwrap_or_default(), size / 0.6),
    }
}

/// A dock cell or search result: faint under the pointer, lit and edged
/// in the accent when picked.
fn hover_tile(picked: bool) -> theme::Button {
    let style = move |hovered: bool| widget::button::Style {
        background: Some(Background::Color(if picked {
            palette::ROW_SELECTED
        } else if hovered {
            palette::ROW_HOVER
        } else {
            Color::TRANSPARENT
        })),
        border_radius: 14.0.into(),
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

/// A plain fill with rounded corners: the dock's running dot, its divider.
fn fill_round(color: Color, radius: f32) -> theme::Container<'static> {
    theme::Container::custom(move |_| widget::container::Style {
        background: Some(Background::Color(color)),
        border: iced::Border {
            radius: radius.into(),
            ..Default::default()
        },
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: Size = Size::new(1400.0, 1000.0);

    const SCREEN: Size = Size::new(2048.0, 1247.0);
    const MIDDLE: f32 = ROOM.width / 2.0;

    fn item(key: &str, pinned: bool) -> DockItem<'static> {
        DockItem {
            key: key.into(),
            app: None,
            windows: Vec::new(),
            pinned,
        }
    }

    #[test]
    fn dock_cells_leave_room_for_the_dividers() {
        // Only the grid: padding, a cell, padding.
        assert_eq!(dock_cells(&[]), (vec![], DOCK_PAD + DOCK_CELL + DOCK_PAD));
        // Two pinned, one running: a divider before the running one and
        // another before the grid.
        let (cells, width) = dock_cells(&[item("a", true), item("b", true), item("c", false)]);
        let step = DOCK_CELL + DOCK_GAP;
        let divider = DIVIDER + DOCK_GAP;
        assert_eq!(
            cells,
            vec![DOCK_PAD, DOCK_PAD + step, DOCK_PAD + 2.0 * step + divider]
        );
        assert_eq!(
            width,
            DOCK_PAD + 3.0 * step + 2.0 * divider + DOCK_CELL + DOCK_PAD
        );
    }

    #[test]
    fn the_panel_lists_the_search_or_every_app() {
        let app = |name: &str| App::test(name);
        let apps = [app("Zed"), app("chromium"), app("Alacritty")];
        let names = |q: &str, all: bool| -> Vec<String> {
            shown_apps(&apps, q, all)
                .iter()
                .map(|a| a.name.clone())
                .collect()
        };
        assert!(names("", false).is_empty());
        assert_eq!(names("", true), ["Alacritty", "chromium", "Zed"]);
        // Typing filters, every app or not.
        assert_eq!(names("ze", true), ["Zed"]);
        assert_eq!(names("ze", false), ["Zed"]);
        assert_eq!(columns(3), 3);
        assert_eq!(columns(30), APPS_COLUMNS);
        assert_eq!(columns(0), 1);
    }

    #[test]
    fn prefixes_list_rows_and_the_launcher_always_does() {
        let apps = [App::test("Zed"), App::test("zathura")];
        let kinds = |light: bool, query: &str| {
            let mut o = Overview::new(window::Id::unique(), Vec::new(), light);
            o.query = query.into();
            let found = o.found(&apps, &[]);
            let rows = o.rows(&found);
            let names: Vec<String> = found
                .iter()
                .map(|f| match f {
                    Found::App(a) => a.name.clone(),
                    Found::Run(c, t) => format!("run {c} {t}"),
                    Found::Help(h) => h.prefix.into(),
                    _ => "?".into(),
                })
                .collect();
            (rows, names)
        };
        assert_eq!(
            kinds(false, "z"),
            (false, vec!["Zed".into(), "zathura".into()])
        );
        assert_eq!(kinds(true, "z").0, true);
        assert_eq!(kinds(false, ":ls"), (true, vec!["run ls false".into()]));
        assert_eq!(kinds(true, "t: htop").1, ["run htop true"]);
        assert_eq!(kinds(false, "?").1.len(), search::HELP.len());
        // Nothing found yet for a folder: nothing listed, not the apps.
        assert!(kinds(false, "~/z").1.is_empty());
        // The app library: every app, as tiles; a prefix still lists rows.
        let mut library = Overview::new(window::Id::unique(), Vec::new(), true);
        library.all_apps = true;
        let found = library.found(&apps, &[]);
        assert_eq!((found.len(), library.rows(&found)), (2, false));
        library.query = "?".into();
        assert!(library.rows(&library.found(&apps, &[])));
        assert_eq!(chosen(&Key::Character("3".into())), Some(2));
        assert_eq!(chosen(&Key::Character("0".into())), None);
    }

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
        let mut widths: Vec<(usize, f32)> =
            laid.iter().map(|c| (c.index, c.picture.width)).collect();
        widths.sort_by_key(|w| w.0);
        assert!(widths.windows(2).all(|w| w[0].1 >= w[1].1), "{widths:?}");
        assert!(
            widths[0].1 > widths[1].1 && widths[1].1 > widths[3].1,
            "{widths:?}"
        );
        // The floor: the fifth and after alike, at 70 %.
        assert_eq!(widths[4].1, widths[5].1);
        assert!((widths[5].1 / widths[0].1 - 0.7).abs() < 0.01, "{widths:?}");
    }

    #[test]
    fn a_swipe_speeding_up_is_one_push() {
        let mut overview = Overview::new(window::Id::unique(), Vec::new(), false);
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let pixels = |y: f32| ScrollDelta::Pixels { x: 0.0, y };
        // Starts slow, gets fast, stays fast for 300 ms, short of 400 px.
        let mut steps = overview.scrolled(pixels(-1.0), ms(0));
        let mut at = 0;
        for y in [
            2.0, 3.0, 5.0, 8.0, 12.0, 16.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0,
        ] {
            at += 20;
            steps += overview.scrolled(pixels(-y), ms(at));
        }
        assert_eq!(steps, 1);
    }

    #[test]
    fn a_fresh_push_mid_scroll_steps() {
        let mut overview = Overview::new(window::Id::unique(), Vec::new(), false);
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
        let mut overview = Overview::new(window::Id::unique(), Vec::new(), false);
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
                .map(|i| {
                    Size::new(
                        800.0 + 150.0 * (i % 4) as f32,
                        600.0 + 90.0 * (i % 3) as f32,
                    )
                })
                .collect();
            let laid = cards(&windows, ROOM, MIDDLE);
            let left = laid.iter().map(|c| c.at.x).fold(f32::MAX, f32::min);
            let right = laid.iter().map(|c| c.at.x + c.width).fold(0.0, f32::max);
            let top = laid.iter().map(|c| c.at.y).fold(f32::MAX, f32::min);
            let bottom = laid
                .iter()
                .map(|c| c.at.y + card_height(c.picture))
                .fold(0.0, f32::max);
            assert!(
                left >= 0.0 && right <= ROOM.width + 1.0,
                "{n}: {left}..{right}"
            );
            assert!(
                top >= 0.0 && bottom <= ROOM.height + 1.0,
                "{n}: {top}..{bottom}"
            );
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
                assert!(
                    (l - (ROOM.width - r)).abs() <= 1.5,
                    "{n}: row of {}",
                    row.len()
                );
            }
        }
    }
}
