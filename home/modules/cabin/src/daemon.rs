// SPDX-License-Identifier: GPL-3.0-only

//! cabind: the one process that holds the state and draws every panel.
//!
//! The panels (controls, audio, date menu, System) live on one see-through
//! layer surface over the work area, in the overlay layer, so they sit
//! above every window and each other as they're stacked. Each floats where
//! it was dragged to, by its grip; pinned, it stays up while the others
//! come and go. While an unpinned one shows, the surface takes the whole
//! area's input and the keyboard, as a popup would: a press on nothing
//! closes, Escape too. With only pinned ones up, it takes input on them
//! alone and lets the keyboard go, so the windows under work as ever.
//!
//! The volume's on-screen bar is a surface of its own, near the bottom,
//! and so is the window switcher, over the whole screen while it shows.

use std::time::Duration;

use cosmic::{
    Element, Task, app,
    iced::{
        self, Alignment, Background, Color, Length, Limits, Point, Rectangle, Size, Subscription,
        event,
        platform_specific::shell::commands::{
            blur::blur,
            keyboard_shortcuts_inhibit::inhibit_shortcuts,
            layer_surface::{
                Anchor, KeyboardInteractivity, Layer, destroy_layer_surface, get_layer_surface,
                set_input_zone, set_keyboard_interactivity,
            },
        },
        runtime::platform_specific::wayland::layer_surface::{
            IcedMargin, IcedOutput, SctkLayerSurfaceSettings,
        },
        window,
    },
    theme, widget,
};

use cosmic::cctk::sctk::reexports::calloop;

use crate::{
    app::{self as controls, App, Card},
    backend::{self, windows},
    bus::{self, Owner, Request},
    capture,
    clock::{self, Clock},
    switcher::{self, Switcher},
    ui::{self, GAP, PANE_WIDTH, palette, popup::Placed},
};

/// A panel: what floats on the overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Controls,
    Audio,
    Date,
    System,
}

impl Panel {
    const ALL: [Panel; 4] = [Panel::Controls, Panel::Audio, Panel::Date, Panel::System];

    fn owner(self) -> Owner {
        match self {
            Panel::Controls | Panel::Audio => Owner::Controls,
            Panel::Date => Owner::Date,
            Panel::System => Owner::System,
        }
    }

    fn width(self) -> f32 {
        match self {
            Panel::Date => clock::WIDTH,
            Panel::System => ui::system_pane::WIDTH,
            Panel::Controls | Panel::Audio => PANE_WIDTH,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Where a panel is and whether it stays up.
#[derive(Clone, Copy, Debug, Default)]
struct Place {
    /// Where it was dragged to; at home until then.
    at: Option<Point>,
    pinned: bool,
}

/// A panel being dragged by its grip.
#[derive(Clone, Copy, Debug)]
struct Drag {
    panel: Panel,
    /// Where the pointer and the panel were when it was grabbed.
    from: Point,
    start: Point,
}

/// From the panels' edge to the work area's right edge, and to its top.
const EDGE: f32 = 14.0;
const TOP: f32 = 4.0;
/// How long the volume bar stays after the last change.
const OSD_FOR: Duration = Duration::from_millis(1300);
const OSD_SIZE: Size = Size::new(340.0, 54.0);
/// How soon after the switcher opens Alt or Super should be seen held; if
/// the keyboard came and they weren't, the shortcut was let go already.
const SWITCHER_LATE: Duration = Duration::from_millis(300);
/// How long Alt or Super must be held before the switcher shows: a quick
/// Alt+Tab switches without it flashing up.
const SWITCHER_REVEAL: Duration = Duration::from_millis(120);

#[derive(Clone, Debug)]
pub enum Message {
    Controls(controls::Message),
    Clock(clock::Message),
    Bus(Request),
    /// A surface's size, when the compositor sets it.
    Configured(window::Id, Size),
    Closed(window::Id),
    Cursor(window::Id, Point),
    Released,
    Escape,
    /// A press on a panel: it comes to the front.
    Front(Panel),
    Grab(Panel),
    SnapBack(Panel),
    TogglePin(Panel),
    /// A press on none of the panels.
    PressOutside,
    /// The panels' rectangles as laid out, to frost and take input there.
    Bounds(Vec<Rectangle>),
    Refrost,
    OsdRefrost,
    /// The volume bar's time is up, unless something showed it since.
    OsdDone(u64),
    Switcher(switcher::Message),
    Windows(windows::Event),
    /// The capture thread, run while the switcher shows windows' pictures.
    Captured(capture::Event),
    Done,
}

pub struct Daemon {
    core: app::Core,
    controls: App,
    clock: Clock,
    /// What a panel button opened, if anything: its panels show, and the
    /// pinned ones besides.
    current: Option<Owner>,
    places: [Place; 4],
    /// Back to front, as last pressed.
    order: Vec<Panel>,
    drag: Option<Drag>,
    /// The overlay, while any panel shows, and its size once known.
    overlay: Option<window::Id>,
    size: Option<Size>,
    /// Every child's rectangle on the overlay as last laid out.
    rects: Vec<Rectangle>,
    cursor: Point,
    /// What the overlay was last told: input everywhere (None) or here;
    /// the keyboard taken or not.
    zone: Option<Option<Vec<Rectangle>>>,
    keyboard: Option<KeyboardInteractivity>,
    /// The volume bar: its surface, what it shows, which showing it is.
    osd: Option<window::Id>,
    osd_level: (f32, bool),
    osd_serial: u64,
    /// The bars as last told to the buttons.
    bars: [String; 3],
    /// The open windows, the one used last first, and the way to raise one.
    windows: Vec<windows::Window>,
    window_commands: Option<calloop::channel::SyncSender<windows::Command>>,
    switcher: Option<Switcher>,
    /// The capture thread's ear, while it takes the switcher's pictures,
    /// and the pictures as last taken, by window id.
    capture: Option<calloop::channel::SyncSender<capture::Command>>,
    thumbs: std::collections::HashMap<String, widget::image::Handle>,
}

/// A task of a part's, as the daemon's.
fn lift<A: Send + 'static>(task: app::Task<A>, f: fn(A) -> Message) -> app::Task<Message> {
    task.map(move |action| match action {
        cosmic::Action::App(message) => cosmic::Action::App(f(message)),
        cosmic::Action::Cosmic(action) => cosmic::Action::Cosmic(action),
        cosmic::Action::None => cosmic::Action::None,
    })
}

fn done() -> cosmic::Action<Message> {
    cosmic::action::app(Message::Done)
}

impl Daemon {
    fn shown(&self, panel: Panel) -> bool {
        self.current == Some(panel.owner()) || self.places[panel.index()].pinned
    }

    /// The panels on screen, in a fixed order: the overlay's children, whose
    /// state goes by position, so coming to the front must not move them.
    fn laid(&self) -> Vec<Panel> {
        Panel::ALL.into_iter().filter(|p| self.shown(*p)).collect()
    }

    /// A panel's height in the stack: pinned ones above the rest, then the
    /// last pressed on top.
    fn z(&self, panel: Panel) -> u32 {
        let pressed = self.order.iter().position(|p| *p == panel).unwrap_or(0) as u32;
        if self.places[panel.index()].pinned {
            100 + pressed
        } else {
            pressed
        }
    }

    /// Something shows that a press elsewhere closes: the overlay takes all.
    fn modal(&self) -> bool {
        self.drag.is_some()
            || Panel::ALL
                .iter()
                .any(|p| self.shown(*p) && !self.places[p.index()].pinned)
            || (self.controls.shown && self.controls.menu.is_some())
    }

    /// Where a panel sits when it hasn't been dragged.
    fn home(panel: Panel, size: Size) -> Point {
        let right = size.width - PANE_WIDTH - EDGE;
        match panel {
            Panel::Controls | Panel::System => Point::new(right, TOP),
            Panel::Audio => Point::new(right - GAP - PANE_WIDTH, TOP),
            Panel::Date => Point::new(((size.width - clock::WIDTH) / 2.0).max(GAP), TOP),
        }
    }

    fn position(&self, panel: Panel, size: Size) -> Point {
        self.places[panel.index()]
            .at
            .unwrap_or_else(|| Self::home(panel, size))
    }

    fn front(&mut self, panel: Panel) {
        self.order.retain(|p| *p != panel);
        self.order.push(panel);
    }

    /// The panel under the pointer, the topmost.
    fn hovered(&self) -> Option<Panel> {
        self.laid()
            .into_iter()
            .zip(&self.rects)
            .filter(|(_, rect)| rect.contains(self.cursor))
            .map(|(panel, _)| panel)
            .max_by_key(|panel| self.z(*panel))
    }

    fn open(&mut self, owner: Owner) {
        self.current = Some(owner);
        for panel in Panel::ALL.into_iter().filter(|p| p.owner() == owner) {
            self.front(panel);
        }
    }

    /// The parts told what shows; the overlay made, unmade or set to take
    /// input as it should; the buttons told their bars.
    fn sync(&mut self) -> app::Task<Message> {
        if std::mem::take(&mut self.controls.wants_close) && self.current == Some(Owner::Controls) {
            self.current = None;
        }
        if std::mem::take(&mut self.controls.wants_open) {
            self.open(Owner::Controls);
        }
        if std::mem::take(&mut self.clock.wants_close) && self.current == Some(Owner::Date) {
            self.current = None;
        }
        let controls = self.shown(Panel::Controls) || self.shown(Panel::Audio);
        if self.controls.shown && !controls {
            self.controls.forget();
        }
        self.controls.shown = controls;
        self.controls.system_shown = self.shown(Panel::System);
        self.controls.sync_fast();
        let date = self.shown(Panel::Date);
        if date && !self.clock.shown {
            self.clock.opened();
        }
        self.clock.shown = date;
        // The system's own notices, pinned over the notifications.
        self.clock.notices = self
            .controls
            .system
            .as_ref()
            .and_then(|s| s.nixos.as_ref())
            .map(backend::nixos::notices)
            .unwrap_or_default();

        let mut tasks = vec![self.publish()];
        let any = Panel::ALL.iter().any(|p| self.shown(*p));
        match (any, self.overlay) {
            (true, None) => tasks.push(self.make_overlay()),
            (false, Some(id)) => {
                self.overlay = None;
                self.size = None;
                self.rects.clear();
                self.drag = None;
                tasks.push(destroy_layer_surface(id));
            }
            (true, Some(id)) => tasks.push(self.set_input(id)),
            (false, None) => {}
        }
        Task::batch(tasks)
    }

    fn make_overlay(&mut self) -> app::Task<Message> {
        let id = window::Id::unique();
        self.overlay = Some(id);
        self.size = None;
        self.rects.clear();
        let modal = self.modal();
        let keyboard = if modal {
            KeyboardInteractivity::Exclusive
        } else {
            KeyboardInteractivity::None
        };
        self.keyboard = Some(keyboard);
        self.zone = Some(None);
        get_layer_surface(SctkLayerSurfaceSettings {
            id,
            layer: Layer::Overlay,
            keyboard_interactivity: keyboard,
            input_zone: None,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            output: IcedOutput::Active,
            namespace: "cabin".into(),
            // Inside the work area: under the panel, which keeps its clicks.
            exclusive_zone: 0,
            size: Some((None, None)),
            size_limits: Limits::NONE.min_width(1.0).min_height(1.0),
            ..Default::default()
        })
    }

    /// Input and keyboard for the overlay: all of it while something a
    /// press elsewhere closes shows, else just the panels.
    fn set_input(&mut self, id: window::Id) -> app::Task<Message> {
        let modal = self.modal();
        let zone = if modal {
            None
        } else {
            Some(self.rects.clone())
        };
        let keyboard = if modal {
            KeyboardInteractivity::Exclusive
        } else {
            KeyboardInteractivity::None
        };
        let mut tasks = Vec::new();
        if self.zone.as_ref() != Some(&zone) {
            self.zone = Some(zone.clone());
            tasks.push(set_input_zone(id, zone));
        }
        if self.keyboard != Some(keyboard) {
            self.keyboard = Some(keyboard);
            tasks.push(set_keyboard_interactivity(id, keyboard));
        }
        Task::batch(tasks)
    }

    /// The compositor asked to blur behind the panels, where they are now.
    fn frost(&self) -> app::Task<Message> {
        match self.overlay {
            Some(id) if !self.rects.is_empty() => blur(
                id,
                Some(ui::popup::rounded_region(&self.rects, ui::PANE_RADIUS)),
            )
            .map(|()| done()),
            _ => Task::none(),
        }
    }

    /// The buttons' bars, each told when it changed.
    fn publish(&mut self) -> app::Task<Message> {
        let bars = [
            (
                "status",
                serde_json::to_string(
                    &self
                        .controls
                        .status_bar(self.current == Some(Owner::Controls)),
                ),
            ),
            (
                "clock",
                serde_json::to_string(&self.clock.bar(self.current == Some(Owner::Date))),
            ),
            (
                "system",
                serde_json::to_string(
                    &self
                        .controls
                        .system_bar(self.current == Some(Owner::System)),
                ),
            ),
        ];
        let mut tasks = Vec::new();
        for (i, (kind, bar)) in bars.into_iter().enumerate() {
            let Ok(bar) = bar else {
                continue;
            };
            if self.bars[i] != bar {
                self.bars[i] = bar.clone();
                tasks.push(Task::future(async move {
                    bus::publish_bar(kind, bar).await;
                    done()
                }));
            }
        }
        Task::batch(tasks)
    }

    /// The volume bar shown, or kept up, with the level now.
    fn show_osd(&mut self) -> app::Task<Message> {
        let Some(level) = self.controls.output_shown() else {
            return Task::none();
        };
        self.osd_level = level;
        self.osd_serial += 1;
        let serial = self.osd_serial;
        let hide = Task::future(async move {
            tokio::time::sleep(OSD_FOR).await;
            cosmic::action::app(Message::OsdDone(serial))
        });
        if self.osd.is_some() {
            return hide;
        }
        let id = window::Id::unique();
        self.osd = Some(id);
        let make = get_layer_surface(SctkLayerSurfaceSettings {
            id,
            layer: Layer::Overlay,
            keyboard_interactivity: KeyboardInteractivity::None,
            // Nothing takes input: it's only to look at.
            input_zone: Some(Vec::new()),
            anchor: Anchor::BOTTOM,
            output: IcedOutput::Active,
            namespace: "cabin-osd".into(),
            margin: IcedMargin {
                bottom: 110,
                ..Default::default()
            },
            exclusive_zone: -1,
            size: Some((Some(OSD_SIZE.width as u32), Some(OSD_SIZE.height as u32))),
            size_limits: Limits::NONE
                .min_width(OSD_SIZE.width)
                .max_width(OSD_SIZE.width)
                .min_height(OSD_SIZE.height)
                .max_height(OSD_SIZE.height),
            ..Default::default()
        });
        // Asked a few times: an ask before the surface is up is lost.
        let frost = [60, 200, 500].map(|ms| {
            Task::future(async move {
                tokio::time::sleep(Duration::from_millis(ms)).await;
                cosmic::action::app(Message::OsdRefrost)
            })
        });
        Task::batch([make, hide].into_iter().chain(frost))
    }

    fn osd_frost(&self) -> app::Task<Message> {
        match self.osd {
            Some(id) => {
                let rect = Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: OSD_SIZE.width,
                    height: OSD_SIZE.height,
                };
                blur(
                    id,
                    Some(ui::popup::rounded_region(&[rect], OSD_SIZE.height / 2.0)),
                )
                .map(|()| done())
            }
            None => Task::none(),
        }
    }

    /// The switcher up over the whole screen, taking the keyboard so it
    /// hears Alt or Super let go; what was open goes, as for any popup. It
    /// takes the keyboard on demand, not exclusively: cosmic-comp lets no
    /// window have the focus while an exclusive surface is up, and the raise
    /// must take it. Seen only once held a moment.
    fn open_switcher(&mut self, step: i32, previews: bool) -> app::Task<Message> {
        if let Some(switcher) = &mut self.switcher {
            switcher.step(step);
            return Task::none();
        }
        // Those on the workspace that shows, the one used last first.
        let here: Vec<windows::Window> = self.windows.iter().filter(|w| w.here).cloned().collect();
        if here.is_empty() {
            return Task::none();
        }
        self.current = None;
        let id = window::Id::unique();
        let mut switcher = Switcher::new(id, here, step, previews);
        // Last time's pictures, until this time's come.
        switcher.thumbs = self.thumbs.clone();
        self.switcher = Some(switcher);
        let make = get_layer_surface(SctkLayerSurfaceSettings {
            id,
            layer: Layer::Overlay,
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            input_zone: None,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            output: IcedOutput::Active,
            namespace: "cabin-switcher".into(),
            // The whole screen, panel and all: centred on it.
            exclusive_zone: -1,
            size: Some((None, None)),
            size_limits: Limits::NONE.min_width(1.0).min_height(1.0),
            ..Default::default()
        });
        let reveal = Task::future(async move {
            tokio::time::sleep(SWITCHER_REVEAL).await;
            cosmic::action::app(Message::Switcher(switcher::Message::Reveal(id)))
        });
        Task::batch([make, switcher_late(id), reveal])
    }

    /// The switcher down, its pick raised if `raise`.
    fn close_switcher(&mut self, raise: bool) -> app::Task<Message> {
        let Some(switcher) = self.switcher.take() else {
            return Task::none();
        };
        // The capture thread ends with its ear gone.
        self.capture = None;
        // The compositor's shortcuts back, then the surface gone.
        let destroy = inhibit_shortcuts(false)
            .map(|()| done())
            .chain(destroy_layer_surface(switcher.id));
        let (Some(window), Some(commands), true) =
            (switcher.picked(), self.window_commands.clone(), raise)
        else {
            return destroy;
        };
        let handle = window.handle.clone();
        // First in the list at once: the next Alt+Tab may come before the
        // compositor says it's active.
        if let Some(i) = self.windows.iter().position(|w| w.handle == handle) {
            let raised = self.windows.remove(i);
            self.windows.insert(0, raised);
        }
        if commands
            .try_send(windows::Command::Activate(handle))
            .is_err()
        {
            tracing::warn!("the window thread is gone; nothing raised");
        }
        destroy
    }

    fn switcher_frost(&self) -> app::Task<Message> {
        match self
            .switcher
            .as_ref()
            .and_then(|s| Some((s.id, s.frost_region()?)))
        {
            Some((id, region)) => blur(id, Some(region)).map(|()| done()),
            None => Task::none(),
        }
    }

    fn switcher_update(&mut self, message: switcher::Message) -> app::Task<Message> {
        use switcher::Message as M;
        let Some(switcher) = self.switcher.as_mut() else {
            return Task::none();
        };
        match message {
            M::Pick(i) => {
                switcher.selected = i;
                self.close_switcher(true)
            }
            M::Step(by) => {
                switcher.step(by);
                Task::none()
            }
            M::Commit => self.close_switcher(true),
            M::Cancel => self.close_switcher(false),
            M::Modifiers(id, modifiers) if id == switcher.id => {
                switcher.heard += 1;
                // It has the keyboard: from now on Tab comes here, not to the
                // compositor's shortcut, so holding it repeats.
                let inhibit = if switcher.heard == 1 {
                    inhibit_shortcuts(true).map(|()| done())
                } else {
                    Task::none()
                };
                let next = if switcher::holding(modifiers) {
                    switcher.held = true;
                    Task::none()
                } else if switcher.held || switcher.heard >= 2 {
                    // Let go, or let go before it came up (a quick tap):
                    // the compositor's own word, after iced's guess, says so.
                    self.close_switcher(true)
                } else {
                    Task::none()
                };
                Task::batch([inhibit, next])
            }
            M::Late(id) if id == switcher.id => {
                if switcher.held {
                    Task::none()
                } else if switcher.heard > 0 {
                    self.close_switcher(true)
                } else {
                    // No keyboard yet: look again.
                    switcher_late(id)
                }
            }
            M::Reveal(id) if id == switcher.id => {
                switcher.shown = true;
                let again = Task::future(async {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    cosmic::action::app(Message::Switcher(switcher::Message::Refrost))
                });
                Task::batch([self.switcher_frost(), again])
            }
            M::Modifiers(..) | M::Late(_) | M::Reveal(_) => Task::none(),
            M::Refrost => self.switcher_frost(),
        }
    }

    fn handle(&mut self, request: Request) -> app::Task<Message> {
        match request {
            Request::Toggle(owner) => {
                if self.current == Some(owner) {
                    self.current = None;
                } else {
                    self.open(owner);
                }
            }
            Request::Open(owner, menu) => {
                self.open(owner);
                if let (Owner::Controls, Some(card)) =
                    (owner, menu.as_deref().and_then(Card::parse))
                {
                    // Shown first, so the card's data is read as on a press.
                    self.controls.shown = true;
                    return lift(
                        self.controls.update(controls::Message::OpenCard(card)),
                        Message::Controls,
                    );
                }
            }
            Request::Close => self.current = None,
            Request::Switch(step, previews) => return self.open_switcher(step, previews),
            Request::Volume(steps) => {
                let step = lift(
                    self.controls.update(controls::Message::ScrollVolume(
                        iced::mouse::ScrollDelta::Lines {
                            x: 0.0,
                            y: steps as f32,
                        },
                    )),
                    Message::Controls,
                );
                return Task::batch([step, self.show_osd()]);
            }
            Request::Notified(notification) => {
                return lift(
                    self.clock.update(clock::Message::Notification(
                        backend::notifications::Event::New(*notification),
                    )),
                    Message::Clock,
                );
            }
            Request::NotificationClosed(id) => {
                return lift(
                    self.clock.update(clock::Message::Notification(
                        backend::notifications::Event::Closed(id),
                    )),
                    Message::Clock,
                );
            }
        }
        Task::none()
    }

    fn panel_view(&self, panel: Panel, hovered: Option<Panel>) -> Element<'_, Message> {
        let place = self.places[panel.index()];
        let dragging = self.drag.is_some_and(|d| d.panel == panel);
        let grip = ui::grip(
            place.pinned,
            hovered == Some(panel),
            dragging,
            Message::Grab(panel),
            Message::SnapBack(panel),
            Message::TogglePin(panel),
        );
        let content: Element<'_, Message> = match panel {
            Panel::Controls => self.controls.main_pane().map(Message::Controls),
            Panel::Audio => self.controls.audio_pane().map(Message::Controls),
            Panel::Date => self.clock.view().map(Message::Clock),
            Panel::System => self.controls.system_pane().map(Message::Controls),
        };
        ui::panel(grip, content, panel.width(), place.pinned)
    }

    fn overlay_view(&self) -> Element<'_, Message> {
        let Some(size) = self.size else {
            return widget::space::horizontal().into();
        };
        let hovered = self.hovered();
        let mut placed = Placed::new(size);
        for panel in self.laid() {
            placed = placed.push_stacked(
                self.panel_view(panel, hovered),
                self.position(panel, size),
                panel.width(),
                self.z(panel),
                Message::Front(panel),
            );
        }
        // A right-click menu in the controls, over everything.
        if let (true, Some((what, at))) = (self.controls.shown, &self.controls.menu) {
            let width = 220.0;
            let at = Point::new(
                at.x.min(size.width - width - GAP).max(0.0),
                (at.y - 8.0).max(0.0),
            );
            placed = placed.push(ui::menu(what).map(Message::Controls), at, width);
        }
        placed
            .on_press_outside(Message::PressOutside)
            .on_bounds(self.rects.clone(), Message::Bounds)
            .into()
    }

    /// The volume bar: the speaker, the level as a bar (red past 100 %,
    /// with a mark there), the number.
    fn osd_view(&self) -> Element<'_, Message> {
        let (level, muted) = self.osd_level;
        let max = backend::audio::MAX;
        let fill = (level / max).clamp(0.0, 1.0);
        let over = level > 1.0 + 0.001;
        let name = if muted || level <= 0.001 {
            "volume-off"
        } else if level < 0.5 {
            "volume-2"
        } else {
            "volume"
        };
        let rail = OsdRail {
            fill,
            mark: (max > 1.0).then_some(1.0 / max),
            over,
        };
        let percent = ui::text::sized_bold(format!("{:.0} %", level * 100.0), 13.0);
        let row = widget::row::with_children(vec![
            ui::icon(name, 20).into(),
            rail.view(),
            widget::container(percent)
                .width(Length::Fixed(44.0))
                .align_x(Alignment::End)
                .into(),
        ])
        .spacing(14)
        .align_y(Alignment::Center);
        widget::container(row)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding([0, 18])
            .center_y(Length::Fill)
            .class(theme::Container::custom(|_| widget::container::Style {
                background: Some(Background::Color(palette::rgba(34, 46, 84, 0.86))),
                text_color: Some(palette::TEXT),
                icon_color: Some(palette::TEXT),
                border: iced::Border {
                    radius: (OSD_SIZE.height / 2.0).into(),
                    width: 1.0,
                    color: palette::PANE_BORDER,
                },
                ..Default::default()
            }))
            .into()
    }
}

/// The on-screen bar's rail: the level filled, a mark at 100 %.
struct OsdRail {
    fill: f32,
    mark: Option<f32>,
    over: bool,
}

impl OsdRail {
    fn view<'a>(self) -> Element<'a, Message> {
        let color = if self.over {
            palette::OVER
        } else {
            palette::ACCENT
        };
        let filled = (self.fill * 1000.0).round() as u16;
        let rest = 1000 - filled.min(1000);
        let piece = |portion: u16, background: Color| -> Element<'a, Message> {
            widget::container(widget::space::horizontal())
                .width(Length::FillPortion(portion.max(1)))
                .height(Length::Fixed(6.0))
                .class(theme::Container::custom(move |_| {
                    widget::container::Style {
                        background: Some(Background::Color(background)),
                        border: iced::Border {
                            radius: 3.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }))
                .into()
        };
        let mut pieces = Vec::new();
        if filled > 0 {
            pieces.push(piece(filled, color));
        }
        if rest > 0 {
            pieces.push(piece(rest, palette::rgba(200, 220, 255, 0.2)));
        }
        let rail: Element<'a, Message> = widget::row::with_children(pieces)
            .width(Length::Fill)
            .into();
        match self.mark {
            Some(at) => {
                let at = (at * 1000.0).round() as u16;
                let tick = widget::container(widget::space::horizontal())
                    .width(Length::Fixed(2.0))
                    .height(Length::Fixed(14.0))
                    .class(theme::Container::custom(|_| widget::container::Style {
                        background: Some(Background::Color(palette::rgba(232, 236, 246, 0.6))),
                        ..Default::default()
                    }));
                let marks = widget::row::with_children(vec![
                    widget::space::horizontal()
                        .width(Length::FillPortion(at.max(1)))
                        .into(),
                    tick.into(),
                    widget::space::horizontal()
                        .width(Length::FillPortion((1000 - at.min(1000)).max(1)))
                        .into(),
                ])
                .width(Length::Fill)
                .align_y(Alignment::Center);
                iced::widget::stack([
                    widget::container(rail).center_y(Length::Fixed(14.0)).into(),
                    marks.into(),
                ])
                .width(Length::Fill)
                .into()
            }
            None => rail,
        }
    }
}

/// The switcher told when Alt or Super should have been seen held.
fn switcher_late(id: window::Id) -> app::Task<Message> {
    Task::future(async move {
        tokio::time::sleep(SWITCHER_LATE).await;
        cosmic::action::app(Message::Switcher(switcher::Message::Late(id)))
    })
}

/// With `QS_DEBUG_OPEN`, a panel opens a moment after start: "system",
/// "date", "osd", or a controls menu's name.
fn debug_open() -> app::Task<Message> {
    let request = match std::env::var("QS_DEBUG_OPEN").as_deref() {
        Ok("system") => Request::Open(Owner::System, None),
        Ok("date") => Request::Open(Owner::Date, None),
        Ok("osd") => Request::Volume(0.0),
        Ok("controls") => Request::Open(Owner::Controls, None),
        Ok(_) => return lift(controls::debug_open(), Message::Controls),
        Err(_) => return Task::none(),
    };
    Task::future(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        cosmic::action::app(Message::Bus(request))
    })
}

impl cosmic::Application for Daemon {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "dev.troy.Cabin.Daemon";

    fn core(&self) -> &app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut app::Core {
        &mut self.core
    }

    fn init(mut core: app::Core, _flags: ()) -> (Self, app::Task<Message>) {
        core.set_app_type(cosmic::core::AppType::System);
        // The design's font, loaded by hand: fontconfig may not see the Nix store.
        let font = ui::text::load_font().map(|()| done());
        let daemon = Self {
            core,
            controls: App::new(),
            clock: Clock::new(),
            current: None,
            places: [Place::default(); 4],
            order: Panel::ALL.to_vec(),
            drag: None,
            overlay: None,
            size: None,
            rects: Vec::new(),
            cursor: Point::ORIGIN,
            zone: None,
            keyboard: None,
            osd: None,
            osd_level: (0.0, false),
            osd_serial: 0,
            bars: Default::default(),
            windows: Vec::new(),
            window_commands: None,
            switcher: None,
            capture: None,
            thumbs: Default::default(),
        };
        (daemon, Task::batch([font, debug_open()]))
    }

    fn style(&self) -> Option<iced::theme::Style> {
        Some(iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: palette::TEXT,
            icon_color: palette::TEXT,
        })
    }

    fn update(&mut self, message: Message) -> app::Task<Message> {
        let task = match message {
            Message::Controls(message) => lift(self.controls.update(message), Message::Controls),
            Message::Clock(message) => lift(self.clock.update(message), Message::Clock),
            Message::Bus(request) => self.handle(request),
            Message::Configured(id, size) => {
                if Some(id) == self.overlay {
                    self.size = Some(size);
                    Task::none()
                } else if Some(id) == self.osd {
                    self.osd_frost()
                } else if let Some(switcher) = self.switcher.as_mut().filter(|s| s.id == id) {
                    switcher.resized(size);
                    // Asked again: an ask before the surface is up is lost.
                    let again = Task::future(async {
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        cosmic::action::app(Message::Switcher(switcher::Message::Refrost))
                    });
                    Task::batch([self.switcher_frost(), again])
                } else {
                    Task::none()
                }
            }
            Message::Closed(id) => {
                if Some(id) == self.overlay {
                    // Closed under us: start over with nothing up.
                    self.overlay = None;
                    self.size = None;
                    self.rects.clear();
                    self.current = None;
                    self.drag = None;
                    self.places = [Place::default(); 4];
                }
                if Some(id) == self.osd {
                    self.osd = None;
                }
                if self.switcher.as_ref().is_some_and(|s| s.id == id) {
                    self.switcher = None;
                    self.capture = None;
                }
                Task::none()
            }
            Message::Cursor(id, at) => {
                if Some(id) != self.overlay {
                    return Task::none();
                }
                self.cursor = at;
                if let (Some(drag), Some(size)) = (self.drag, self.size) {
                    // Kept where its grip can still be reached.
                    let width = drag.panel.width();
                    let x =
                        (drag.start.x + at.x - drag.from.x).clamp(80.0 - width, size.width - 80.0);
                    let y = (drag.start.y + at.y - drag.from.y).clamp(0.0, size.height - 40.0);
                    self.places[drag.panel.index()].at = Some(Point::new(x, y));
                }
                lift(
                    self.controls.update(controls::Message::CursorMoved(at)),
                    Message::Controls,
                )
            }
            Message::Released => {
                self.drag = None;
                Task::none()
            }
            Message::Escape if self.switcher.is_some() => self.close_switcher(false),
            Message::Escape => match self.current {
                Some(Owner::Controls) => lift(
                    self.controls.update(controls::Message::Escape),
                    Message::Controls,
                ),
                Some(_) => {
                    self.current = None;
                    Task::none()
                }
                None => Task::none(),
            },
            Message::Front(panel) => {
                self.front(panel);
                Task::none()
            }
            Message::Grab(panel) => {
                let size = self.size.unwrap_or(Size::new(1280.0, 720.0));
                self.front(panel);
                self.drag = Some(Drag {
                    panel,
                    from: self.cursor,
                    start: self.position(panel, size),
                });
                Task::none()
            }
            Message::SnapBack(panel) => {
                // Home, and no longer pinned.
                self.drag = None;
                self.places[panel.index()] = Place::default();
                Task::none()
            }
            Message::TogglePin(panel) => {
                let place = &mut self.places[panel.index()];
                place.pinned = !place.pinned;
                // Unpinned while its button's panel isn't open: it goes.
                self.front(panel);
                Task::none()
            }
            Message::PressOutside => {
                self.current = None;
                if self.controls.menu.is_some() {
                    lift(
                        self.controls.update(controls::Message::CloseMenu),
                        Message::Controls,
                    )
                } else {
                    Task::none()
                }
            }
            Message::Bounds(rects) => {
                self.rects = rects;
                let again = Task::future(async {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    cosmic::action::app(Message::Refrost)
                });
                Task::batch([self.frost(), again])
            }
            Message::Refrost => self.frost(),
            Message::OsdRefrost => self.osd_frost(),
            Message::OsdDone(serial) => match self.osd {
                Some(id) if serial == self.osd_serial => {
                    self.osd = None;
                    destroy_layer_surface(id)
                }
                _ => Task::none(),
            },
            Message::Switcher(message) => self.switcher_update(message),
            Message::Windows(windows::Event::Ready(commands)) => {
                self.window_commands = Some(commands);
                Task::none()
            }
            Message::Windows(windows::Event::Windows(list)) => {
                self.thumbs.retain(|id, _| list.iter().any(|w| &w.id == id));
                self.windows = list;
                match &mut self.switcher {
                    Some(switcher) => {
                        let before = switcher.windows.len();
                        switcher.keep(&self.windows);
                        if switcher.windows.is_empty() {
                            self.close_switcher(false)
                        } else if switcher.windows.len() != before {
                            self.switcher_frost()
                        } else {
                            Task::none()
                        }
                    }
                    None => Task::none(),
                }
            }
            Message::Captured(capture::Event::Ready(commands)) => {
                // A fresh thread takes every window.
                if self.switcher.is_some()
                    && commands.try_send(capture::Command::Thumbnails).is_ok()
                {
                    self.capture = Some(commands);
                }
                Task::none()
            }
            Message::Captured(capture::Event::Thumb(shot)) => {
                let handle = widget::image::Handle::from_rgba(
                    shot.image.width(),
                    shot.image.height(),
                    shot.image.as_raw().clone(),
                );
                if let Some(switcher) = &mut self.switcher {
                    switcher.thumbs.insert(shot.name.clone(), handle.clone());
                }
                self.thumbs.insert(shot.name, handle);
                Task::none()
            }
            // Its stills of the screens and its own window list aren't wanted here.
            Message::Captured(_) => Task::none(),
            Message::Windows(windows::Event::Unavailable) => {
                tracing::warn!("no toplevel protocols: the window switcher stays down");
                self.window_commands = None;
                Task::none()
            }
            Message::Done => Task::none(),
        };
        Task::batch([task, self.sync()])
    }

    fn view(&self) -> Element<'_, Message> {
        widget::text("").into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        if Some(id) == self.overlay {
            self.overlay_view()
        } else if Some(id) == self.osd {
            self.osd_view()
        } else if let Some(switcher) = self.switcher.as_ref().filter(|s| s.id == id) {
            switcher.view().map(Message::Switcher)
        } else {
            widget::text("").into()
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            bus::serve().map(Message::Bus),
            self.controls.subscription().map(Message::Controls),
            self.clock.subscription().map(Message::Clock),
            windows::subscription().map(Message::Windows),
            match &self.switcher {
                // From the start: the pictures take a moment to come.
                Some(switcher) if switcher.previews => {
                    capture::subscription().map(Message::Captured)
                }
                _ => Subscription::none(),
            },
            window::resize_events().map(|(id, size)| Message::Configured(id, size)),
            event::listen_with(|event, _, id| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::Cursor(id, position))
                }
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )) => Some(Message::Released),
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::Escape),
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key, modifiers, ..
                }) => switcher::key(&key, modifiers).map(Message::Switcher),
                iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => Some(
                    Message::Switcher(switcher::Message::Modifiers(id, modifiers)),
                ),
                iced::Event::Window(window::Event::Opened { size, .. }) => {
                    Some(Message::Configured(id, size))
                }
                _ => None,
            }),
        ])
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::Closed(id))
    }
}

pub fn run() -> iced::Result {
    cosmic::app::run::<Daemon>(
        app::Settings::default()
            .no_main_window(true)
            .exit_on_close(false),
        (),
    )
}
