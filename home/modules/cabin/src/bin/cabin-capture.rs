// SPDX-License-Identifier: GPL-3.0-only

//! Super+Shift+S: a screenshot or a recording of a region, a window or a
//! screen. The screens freeze under a shade with a toolbar (Screenshot or
//! Record; Region, Window or Screen): drag a region, click a screen, or
//! pick a window from cards of them all (covered ones and those on other
//! workspaces too: a window is taken on its own). A screenshot is saved
//! and copied. A recording shades everything
//! it doesn't take in until it's stopped, by its pill or by the shortcut
//! again (the second instance tells the first over the session bus). The
//! frames come from the compositor directly (`cabin::capture`), never
//! through the portal, so nothing asks for permission.

use std::{
    collections::HashMap,
    io::Write,
    os::unix::{ffi::OsStrExt, process::CommandExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use cabin::{
    capture::{self, BUS_NAME, BUS_PATH, Command, Rect, RunningProxy, Sound, Target},
    switcher,
    ui::{icon, palette, registry, text},
};
use cosmic::{
    Element, Task, app,
    cctk::{sctk::reexports::calloop, wayland_client::protocol::wl_output::WlOutput},
    iced::{
        self, Alignment, Background, Color, ContentFit, Length, Limits, Padding, Point, Rectangle,
        Size, Subscription,
        event::{
            self,
            wayland::{Event as WaylandEvent, OutputEvent},
        },
        keyboard::{Key, key::Named},
        mouse,
        platform_specific::shell::commands::layer_surface::{
            Anchor, KeyboardInteractivity, Layer, destroy_layer_surface, get_layer_surface,
            set_input_zone, set_keyboard_interactivity,
        },
        runtime::platform_specific::wayland::layer_surface::{
            IcedOutput, SctkLayerSurfaceSettings,
        },
        stream, time,
        widget::{canvas, image, mouse_area, stack},
        window,
    },
    theme, widget,
};
use futures::{SinkExt, StreamExt, channel::mpsc};

/// What's being recorded, for cabind's privacy pill (`Recording` on the bus).
static RECORDING: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
/// The bus connection holding the name, once it's held.
static CONNECTION: std::sync::OnceLock<zbus::Connection> = std::sync::OnceLock::new();
/// Over what isn't taken.
const SHADE: Color = palette::rgba(5, 8, 18, 0.5);
/// The stop pill, and its gap from what's recorded.
const PILL: Size = Size::new(172.0, 44.0);
const GAP: f32 = 12.0;
/// A drag smaller than this is a slip, not a region.
const MIN_SIDE: f32 = 8.0;
/// A window card's picture, and the room above the cards for the toolbar.
const THUMB: Size = Size::new(240.0, 150.0);
const TOOLBAR_ROOM: f32 = 120.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Shot,
    Record,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Region,
    Window,
    Screen,
}

/// What a recording hears besides the microphone: one of these.
#[derive(Clone, Debug, PartialEq)]
enum Heard {
    Off,
    Desktop,
    /// An app by its registry name.
    App(String),
}

/// What was picked: logical pixels, on an output by name.
#[derive(Clone, Debug)]
enum Choice {
    Region { output: String, area: Rectangle },
    Window(String),
    Screen(String),
}

#[derive(Clone, Debug)]
enum Message {
    Capture(capture::Event),
    Output(OutputEvent, WlOutput),
    /// This is the only instance.
    Claimed,
    /// The shortcut again.
    Toggle,
    Kind(Kind),
    Mode(Mode),
    Hear(Heard),
    Mic,
    /// The apps with sound streams open.
    AudioApps(Vec<String>),
    Moved(window::Id, Point),
    Pressed(window::Id),
    Released(window::Id),
    /// A window's card, by window id.
    Pick(String),
    Cancel,
    Stop,
    Tick,
    Done,
}

struct Screen {
    output: WlOutput,
    name: String,
    size: Size,
    surface: Option<window::Id>,
}

struct Still {
    image: Arc<::image::RgbaImage>,
    handle: image::Handle,
}

struct Recording {
    choice: Choice,
    since: Instant,
    stopping: bool,
}

struct App {
    core: app::Core,
    commands: Option<calloop::channel::SyncSender<Command>>,
    claimed: bool,
    screens: Vec<Screen>,
    stills: HashMap<String, Still>,
    windows: Vec<capture::Window>,
    /// The cards' pictures, by window id; `None` until Window mode asks.
    thumbs: Option<HashMap<String, image::Handle>>,
    kind: Kind,
    mode: Mode,
    heard: Heard,
    mic: bool,
    audio_apps: Vec<String>,
    pointer: Option<(window::Id, Point)>,
    drag: Option<(window::Id, Point)>,
    recording: Option<Recording>,
    /// Each overlay's input zone while recording, as last set.
    zones: HashMap<window::Id, Vec<Rectangle>>,
}

impl App {
    /// An overlay on each screen whose still is in, once this is the only instance.
    fn open(&mut self) -> app::Task<Message> {
        if !self.claimed || self.recording.is_some() {
            return Task::none();
        }
        let mut tasks = Vec::new();
        for screen in &mut self.screens {
            if screen.surface.is_some() || !self.stills.contains_key(&screen.name) {
                continue;
            }
            let id = window::Id::unique();
            screen.surface = Some(id);
            tasks.push(get_layer_surface(SctkLayerSurfaceSettings {
                id,
                layer: Layer::Overlay,
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                output: IcedOutput::Output(screen.output.clone()),
                namespace: "cabin-capture".into(),
                // Over the panel too.
                exclusive_zone: -1,
                size: Some((None, None)),
                size_limits: Limits::NONE.min_width(1.0).min_height(1.0),
                ..Default::default()
            }));
        }
        Task::batch(tasks)
    }

    fn close(&mut self) -> app::Task<Message> {
        Task::batch(
            self.screens
                .iter_mut()
                .filter_map(|s| s.surface.take())
                .map(destroy_layer_surface)
                .collect::<Vec<_>>(),
        )
    }

    fn output(&mut self, event: OutputEvent, output: WlOutput) -> app::Task<Message> {
        match event {
            OutputEvent::Created(Some(info)) | OutputEvent::InfoUpdate(info) => {
                let (Some(name), Some((width, height))) = (info.name, info.logical_size) else {
                    return Task::none();
                };
                let size = Size::new(width as f32, height as f32);
                match self.screens.iter_mut().find(|s| s.output == output) {
                    Some(screen) => {
                        screen.name = name;
                        screen.size = size;
                    }
                    None => self.screens.push(Screen {
                        output,
                        name,
                        size,
                        surface: None,
                    }),
                }
                self.open()
            }
            OutputEvent::Removed => {
                let gone = self
                    .screens
                    .iter()
                    .position(|s| s.output == output)
                    .map(|i| self.screens.remove(i));
                match gone.and_then(|s| s.surface) {
                    Some(id) => destroy_layer_surface(id),
                    None => Task::none(),
                }
            }
            _ => Task::none(),
        }
    }

    fn captured(&mut self, event: capture::Event) -> app::Task<Message> {
        match event {
            capture::Event::Ready(commands) => self.commands = Some(commands),
            capture::Event::Shot(shot) => {
                let handle = handle(&shot.image);
                self.stills.insert(
                    shot.name,
                    Still {
                        image: shot.image,
                        handle,
                    },
                );
                return self.open();
            }
            capture::Event::Thumb(shot) => {
                if let Some(thumbs) = &mut self.thumbs {
                    thumbs.insert(shot.name, handle(&shot.image));
                }
            }
            capture::Event::Window(shot) => {
                let (width, height) = shot.image.dimensions();
                return save(
                    shot.image,
                    Rect {
                        x: 0,
                        y: 0,
                        width: width as i32,
                        height: height as i32,
                    },
                );
            }
            capture::Event::Windows(windows) => {
                self.windows = windows;
                return self.zones();
            }
            capture::Event::Recorded(path) => {
                match copy_file(&path) {
                    Ok(()) => tell(
                        "Recording copied",
                        &format!("Saved to {}", path.display()),
                        Some(&path),
                        false,
                    ),
                    Err(err) => tell(
                        "Recording saved, not copied",
                        &format!("wl-copy: {err}"),
                        Some(&path),
                        false,
                    ),
                }
                std::process::exit(0)
            }
            capture::Event::Failed(why) => return fail(why),
            capture::Event::Unavailable => return fail("Cannot reach the compositor".into()),
        }
        Task::none()
    }

    /// How a screen is shaded: around what's recorded, or what a press would take.
    fn cover(&self, screen: &Screen) -> Cover {
        if let Some(recording) = &self.recording {
            return match &recording.choice {
                Choice::Region { output, area } if output == &screen.name => Cover::Around(*area),
                // Followed as it moves, while it's focused and so on top;
                // when it's behind others, away or minimized, a shade or a
                // bright patch would only be in the way.
                Choice::Window(id) => match self
                    .windows
                    .iter()
                    .find(|w| &w.id == id)
                    .filter(|w| w.active)
                {
                    Some(window) => window
                        .shown
                        .iter()
                        .find(|(output, _)| output == &screen.name)
                        .map_or(Cover::All, |(_, r)| Cover::Around(rect(*r))),
                    None => Cover::Nothing,
                },
                _ => Cover::All,
            };
        }
        let Some((on, at)) = self.pointer.filter(|(on, _)| Some(*on) == screen.surface) else {
            return Cover::All;
        };
        match self.mode {
            Mode::Region => self
                .drag
                .filter(|(from_on, _)| *from_on == on)
                .map_or(Cover::All, |(_, from)| Cover::Around(span(from, at))),
            Mode::Window => Cover::All,
            Mode::Screen => Cover::Around(Rectangle::with_size(screen.size)),
        }
    }

    /// Where a region or a screen lies: its screen, and the area there.
    fn area(&self, choice: &Choice) -> Option<(&Screen, Rectangle)> {
        let (output, area) = match choice {
            Choice::Region { output, area } => (output, Some(*area)),
            Choice::Screen(output) => (output, None),
            Choice::Window(_) => return None,
        };
        let screen = self.screens.iter().find(|s| &s.name == output)?;
        Some((screen, area.unwrap_or(Rectangle::with_size(screen.size))))
    }

    /// A window's card pressed: taken whole by the capture thread, or recorded.
    fn pick(&mut self, id: String) -> app::Task<Message> {
        if self.recording.is_some() {
            return Task::none();
        }
        if self.kind == Kind::Record {
            return self.record(Choice::Window(id));
        }
        if !self
            .commands
            .as_ref()
            .is_some_and(|commands| commands.send(Command::Shoot(id)).is_ok())
        {
            return fail("The capture thread is gone".into());
        }
        // The picture comes back as Event::Window.
        self.close()
    }

    fn released(&mut self, id: window::Id) -> app::Task<Message> {
        if self.recording.is_some() {
            return Task::none();
        }
        let drag = self.drag.take();
        let Some((_, at)) = self.pointer.filter(|(on, _)| *on == id) else {
            return Task::none();
        };
        let Some(output) = self
            .screens
            .iter()
            .find(|s| s.surface == Some(id))
            .map(|s| s.name.clone())
        else {
            return Task::none();
        };
        let choice = match self.mode {
            Mode::Region => {
                let Some((_, from)) = drag.filter(|(on, _)| *on == id) else {
                    return Task::none();
                };
                let area = span(from, at);
                if area.width < MIN_SIDE || area.height < MIN_SIDE {
                    return Task::none();
                }
                Choice::Region { output, area }
            }
            // The cards take those.
            Mode::Window => return Task::none(),
            Mode::Screen => Choice::Screen(output),
        };
        match self.kind {
            Kind::Shot => self.shoot(choice),
            Kind::Record => self.record(choice),
        }
    }

    /// The picked part of the still, saved and copied.
    fn shoot(&mut self, choice: Choice) -> app::Task<Message> {
        let Some((screen, area)) = self.area(&choice) else {
            return Task::none();
        };
        let Some(still) = self.stills.get(&screen.name) else {
            return Task::none();
        };
        let crop = pixels(area, screen.size, still.image.dimensions());
        let image = still.image.clone();
        Task::batch([self.close(), save(image, crop)])
    }

    fn record(&mut self, choice: Choice) -> app::Task<Message> {
        let (target, crop) = match &choice {
            Choice::Region { output, .. } => {
                let Some((screen, area)) = self.area(&choice) else {
                    return Task::none();
                };
                let Some(still) = self.stills.get(&screen.name) else {
                    return Task::none();
                };
                (
                    Target::Output(output.clone()),
                    Some(pixels(area, screen.size, still.image.dimensions())),
                )
            }
            Choice::Window(id) => (Target::Window(id.clone()), None),
            Choice::Screen(output) => (Target::Output(output.clone()), None),
        };
        let path = match file_in("Videos/Screencasts", "Screencast", "mp4") {
            Ok(path) => path,
            Err(err) => return fail(format!("No place for the recording: {err}")),
        };
        let sound = Sound {
            desktop: self.heard == Heard::Desktop,
            app: match &self.heard {
                Heard::App(app) => Some(app.clone()),
                _ => None,
            },
            mic: self.mic,
        };
        let sent = self.commands.as_ref().is_some_and(|commands| {
            commands
                .send(Command::Record {
                    target,
                    crop,
                    sound,
                    path,
                })
                .is_ok()
        });
        if !sent {
            return fail("The capture thread is gone".into());
        }
        let mut tasks = Vec::new();
        for screen in &mut self.screens {
            let Some(id) = screen.surface else {
                continue;
            };
            // A whole screen is recorded with whatever is on it, so nothing
            // of ours stays there.
            if matches!(&choice, Choice::Screen(output) if output == &screen.name) {
                screen.surface = None;
                tasks.push(destroy_layer_surface(id));
            } else {
                tasks.push(set_keyboard_interactivity(id, KeyboardInteractivity::None));
            }
        }
        let what = match &choice {
            Choice::Region { output, .. } => format!("Part of {output}"),
            Choice::Window(id) => match self.windows.iter().find(|w| &w.id == id) {
                Some(w) if !w.title.is_empty() => format!("Window · {}", w.title),
                Some(w) => format!("Window · {}", switcher::name(&w.app_id)),
                None => "A window".to_owned(),
            },
            Choice::Screen(output) => format!("Entire screen · {output}"),
        };
        tasks.push(announce(what));
        self.recording = Some(Recording {
            choice,
            since: Instant::now(),
            stopping: false,
        });
        tasks.push(self.zones());
        Task::batch(tasks)
    }

    fn stop(&mut self) -> app::Task<Message> {
        let Some(recording) = &mut self.recording else {
            return Task::none();
        };
        if recording.stopping {
            return Task::none();
        }
        recording.stopping = true;
        if let Some(commands) = &self.commands {
            let _ = commands.send(Command::Stop);
        }
        // The encoder reports the file, and then this ends.
        Task::batch([announce(String::new()), self.close()])
    }

    /// Where a screen's stop pill goes: under what's recorded, else over it,
    /// else inside its foot when that's a window (whose recording never
    /// shows the overlay); none on a region that leaves no room, as the
    /// recording would show it.
    fn pill(&self, screen: &Screen) -> Option<Rectangle> {
        let recording = self.recording.as_ref().filter(|r| !r.stopping)?;
        let size = screen.size;
        let at = |center: f32, y: f32| {
            Rectangle::new(
                Point::new(
                    (center - PILL.width / 2.0)
                        .clamp(GAP, (size.width - PILL.width - GAP).max(GAP)),
                    y,
                ),
                PILL,
            )
        };
        let Cover::Around(hole) = self.cover(screen) else {
            return Some(at(size.width / 2.0, GAP * 2.0));
        };
        let center = hole.x + hole.width / 2.0;
        let below = hole.y + hole.height + GAP;
        let above = hole.y - GAP - PILL.height;
        if below + PILL.height <= size.height - GAP {
            Some(at(center, below))
        } else if above >= GAP {
            Some(at(center, above))
        } else if matches!(recording.choice, Choice::Window(_)) {
            Some(at(center, hole.y + hole.height - PILL.height - GAP * 2.0))
        } else {
            None
        }
    }

    /// While recording, each overlay takes input only on its pill, so the
    /// rest clicks through.
    fn zones(&mut self) -> app::Task<Message> {
        if self.recording.is_none() {
            return Task::none();
        }
        let wanted: Vec<(window::Id, Vec<Rectangle>)> = self
            .screens
            .iter()
            .filter_map(|s| Some((s.surface?, self.pill(s).into_iter().collect())))
            .collect();
        let mut tasks = Vec::new();
        for (id, zone) in wanted {
            if self.zones.get(&id) != Some(&zone) {
                self.zones.insert(id, zone.clone());
                tasks.push(set_input_zone(id, Some(zone)));
            }
        }
        Task::batch(tasks)
    }

    fn toolbar(&self) -> Element<'_, Message> {
        let row = widget::row::with_children(vec![
            chip(
                "camera",
                Some("Screenshot"),
                self.kind == Kind::Shot,
                Message::Kind(Kind::Shot),
            ),
            chip(
                "player-record",
                Some("Record"),
                self.kind == Kind::Record,
                Message::Kind(Kind::Record),
            ),
            divider(),
            chip(
                "marquee-2",
                Some("Region"),
                self.mode == Mode::Region,
                Message::Mode(Mode::Region),
            ),
            chip(
                "app-window",
                Some("Window"),
                self.mode == Mode::Window,
                Message::Mode(Mode::Window),
            ),
            chip(
                "device-desktop",
                Some("Screen"),
                self.mode == Mode::Screen,
                Message::Mode(Mode::Screen),
            ),
            divider(),
            chip("x", None, false, Message::Cancel),
        ])
        .spacing(4)
        .align_y(Alignment::Center);
        let hint = match (self.kind, self.mode) {
            (Kind::Record, _) => "Stop with the pill, or Super+Shift+S again",
            (_, Mode::Region) => "Drag across what to take · Esc cancels",
            (_, Mode::Window) => "Pick a window, wherever it is · Esc cancels",
            (_, Mode::Screen) => "Click a screen · Esc cancels",
        };
        let mut rows: Vec<Element<'_, Message>> = vec![row.into()];
        if self.kind == Kind::Record {
            rows.push(self.sound_row());
        }
        rows.push(
            text::sized(hint, 12.0)
                .class(theme::Text::Color(palette::MUTED))
                .into(),
        );
        let column = widget::column::with_children(rows)
            .spacing(6)
            .align_x(Alignment::Center);
        let card = widget::container(column)
            .padding(8)
            .class(theme::Container::custom(|_| widget::container::Style {
                background: Some(Background::Color(palette::CTX)),
                text_color: Some(palette::TEXT),
                icon_color: Some(palette::TEXT),
                border: iced::Border {
                    radius: 16.0.into(),
                    width: 1.0,
                    color: palette::CTX_BORDER,
                },
                ..Default::default()
            }));
        widget::container(card)
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .padding([28, 0, 0, 0])
            .into()
    }

    /// What a recording hears: nothing, everything playing, or one app's
    /// sound (any with a stream open, playing or paused); and the microphone.
    fn sound_row(&self) -> Element<'_, Message> {
        let mut chips = vec![
            text::sized("Sound", 12.0)
                .class(theme::Text::Color(palette::MUTED))
                .into(),
            chip(
                "volume-off",
                Some("Off"),
                self.heard == Heard::Off,
                Message::Hear(Heard::Off),
            ),
            chip(
                "volume",
                Some("Desktop"),
                self.heard == Heard::Desktop,
                Message::Hear(Heard::Desktop),
            ),
        ];
        for app in &self.audio_apps {
            let on = self.heard == Heard::App(app.clone());
            chips.push(chip(
                registry::app_info(app).icon,
                Some(app.as_str()),
                on,
                Message::Hear(Heard::App(app.clone())),
            ));
        }
        chips.push(divider());
        chips.push(chip("microphone", Some("Mic"), self.mic, Message::Mic));
        widget::row::with_children(chips)
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
    }

    /// A card per window, in rows that fit the screen, under the toolbar.
    fn cards(&self, screen: &Screen) -> Element<'_, Message> {
        if self.windows.is_empty() {
            return widget::container(text::sized("No windows", 15.0))
                .center(Length::Fill)
                .into();
        }
        let card = THUMB.width + 20.0;
        let per_row = (((screen.size.width - 96.0) / (card + 12.0)).floor() as usize).clamp(1, 6);
        let rows: Vec<Element<'_, Message>> = self
            .windows
            .chunks(per_row)
            .map(|chunk| {
                widget::row::with_children(chunk.iter().map(|w| self.card(w)).collect::<Vec<_>>())
                    .spacing(12)
                    .into()
            })
            .collect();
        let grid = widget::container(widget::column::with_children(rows).spacing(12))
            .center_x(Length::Fill);
        widget::container(widget::scrollable(grid))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding {
                top: TOOLBAR_ROOM,
                bottom: 48.0,
                left: 48.0,
                right: 48.0,
            })
            .align_y(Alignment::Center)
            .into()
    }

    fn card<'a>(&'a self, window: &'a capture::Window) -> Element<'a, Message> {
        let picture: Element<'a, Message> = match self
            .thumbs
            .as_ref()
            .and_then(|thumbs| thumbs.get(&window.id))
        {
            Some(handle) => image(handle.clone())
                .width(Length::Fixed(THUMB.width))
                .height(Length::Fixed(THUMB.height))
                .content_fit(ContentFit::Contain)
                .into(),
            None => widget::container(switcher::mark(&window.app_id, 64.0))
                .center_x(Length::Fixed(THUMB.width))
                .center_y(Length::Fixed(THUMB.height))
                .into(),
        };
        let title = if window.title.is_empty() {
            switcher::name(&window.app_id)
        } else {
            window.title.clone()
        };
        let caption = widget::row::with_children(vec![
            switcher::mark(&window.app_id, 24.0),
            text::sized(switcher::ellipsize(&title, 30), 12.5).into(),
        ])
        .spacing(6)
        .align_y(Alignment::Center);
        let style = |hovered: bool| widget::button::Style {
            background: Some(Background::Color(if hovered {
                palette::rgba(48, 62, 104, 0.97)
            } else {
                palette::CTX
            })),
            text_color: Some(palette::TEXT),
            icon_color: Some(palette::TEXT),
            border_radius: 14.0.into(),
            border_width: if hovered { 2.0 } else { 1.0 },
            border_color: if hovered {
                palette::ACCENT
            } else {
                palette::CTX_BORDER
            },
            ..widget::button::Style::new()
        };
        widget::button::custom(
            widget::column::with_children(vec![picture, caption.into()]).spacing(8),
        )
        .padding(10)
        .width(Length::Fixed(THUMB.width + 20.0))
        .class(theme::Button::Custom {
            active: Box::new(move |_, _| style(false)),
            disabled: Box::new(move |_| style(false)),
            hovered: Box::new(move |_, _| style(true)),
            pressed: Box::new(move |_, _| style(true)),
        })
        .on_press(Message::Pick(window.id.clone()))
        .into()
    }

    fn stop_pill(&self) -> Element<'_, Message> {
        let seconds = self
            .recording
            .as_ref()
            .map_or(0, |r| r.since.elapsed().as_secs());
        let dot = widget::container(widget::space::horizontal())
            .width(Length::Fixed(10.0))
            .height(Length::Fixed(10.0))
            .class(theme::Container::custom(|_| widget::container::Style {
                background: Some(Background::Color(palette::OVER)),
                border: iced::Border {
                    radius: 5.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }));
        let content = widget::row::with_children(vec![
            dot.into(),
            text::sized_bold(format!("{}:{:02}", seconds / 60, seconds % 60), 13.5).into(),
            widget::space::horizontal().into(),
            icon("player-stop", 16).into(),
            text::sized_bold("Stop", 13.5).into(),
        ])
        .spacing(8)
        .align_y(Alignment::Center);
        let style = |hovered: bool| widget::button::Style {
            background: Some(Background::Color(if hovered {
                palette::rgba(48, 62, 104, 0.97)
            } else {
                palette::CTX
            })),
            text_color: Some(palette::TEXT),
            icon_color: Some(palette::TEXT),
            border_radius: (PILL.height / 2.0).into(),
            border_width: 1.0,
            border_color: palette::OVER,
            ..widget::button::Style::new()
        };
        widget::button::custom(
            widget::container(content)
                .height(Length::Fill)
                .align_y(Alignment::Center),
        )
        .padding([0, 16])
        .width(Length::Fixed(PILL.width))
        .height(Length::Fixed(PILL.height))
        .class(theme::Button::Custom {
            active: Box::new(move |_, _| style(false)),
            disabled: Box::new(move |_| style(false)),
            hovered: Box::new(move |_, _| style(true)),
            pressed: Box::new(move |_, _| style(true)),
        })
        .on_press(Message::Stop)
        .into()
    }
}

/// A toolbar button: the accent when it's the one chosen.
fn chip<'a>(
    name: &str,
    label: Option<&'a str>,
    on: bool,
    message: Message,
) -> Element<'a, Message> {
    let style = move |hovered: bool| widget::button::Style {
        background: Some(Background::Color(match (on, hovered) {
            (true, _) => palette::ACCENT,
            (false, true) => palette::ROW_HOVER,
            (false, false) => Color::TRANSPARENT,
        })),
        text_color: Some(if on {
            palette::ON_ACCENT
        } else {
            palette::TEXT
        }),
        icon_color: Some(if on {
            palette::ON_ACCENT
        } else {
            palette::TEXT
        }),
        border_radius: 10.0.into(),
        ..widget::button::Style::new()
    };
    let mut content = vec![icon(name, 16).into()];
    if let Some(label) = label {
        content.push(text::sized_bold(label, 13.0).into());
    }
    widget::button::custom(
        widget::row::with_children(content)
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding([7, 12])
    .class(theme::Button::Custom {
        active: Box::new(move |_, _| style(false)),
        disabled: Box::new(move |_| style(false)),
        hovered: Box::new(move |_, _| style(true)),
        pressed: Box::new(move |_, _| style(true)),
    })
    .on_press(message)
    .into()
}

fn divider<'a>() -> Element<'a, Message> {
    widget::container(widget::space::horizontal())
        .width(Length::Fixed(1.0))
        .height(Length::Fixed(22.0))
        .class(theme::Container::custom(|_| widget::container::Style {
            background: Some(Background::Color(palette::SEP)),
            ..Default::default()
        }))
        .into()
}

/// How much of a screen is shaded.
#[derive(Clone, Copy)]
enum Cover {
    All,
    /// All but this, which gets an edge just outside it, where a recording
    /// never looks.
    Around(Rectangle),
    Nothing,
}

struct Shade {
    cover: Cover,
    edge: Color,
}

impl<M> canvas::Program<M, cosmic::Theme, cosmic::Renderer> for Shade {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &cosmic::Renderer,
        _: &cosmic::Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry<cosmic::Renderer>> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        match self.cover {
            Cover::Nothing => {}
            Cover::All => frame.fill_rectangle(Point::ORIGIN, bounds.size(), SHADE),
            Cover::Around(hole) => {
                let (x0, y0) = (hole.x.clamp(0.0, w), hole.y.clamp(0.0, h));
                let (x1, y1) = (
                    (hole.x + hole.width).clamp(x0, w),
                    (hole.y + hole.height).clamp(y0, h),
                );
                // Above, below, left and right of the hole.
                frame.fill_rectangle(Point::ORIGIN, Size::new(w, y0), SHADE);
                frame.fill_rectangle(Point::new(0.0, y1), Size::new(w, h - y1), SHADE);
                frame.fill_rectangle(Point::new(0.0, y0), Size::new(x0, y1 - y0), SHADE);
                frame.fill_rectangle(Point::new(x1, y0), Size::new(w - x1, y1 - y0), SHADE);
                let edge = canvas::Path::rectangle(
                    Point::new(x0 - 1.0, y0 - 1.0),
                    Size::new(x1 - x0 + 2.0, y1 - y0 + 2.0),
                );
                frame.stroke(
                    &edge,
                    canvas::Stroke::default()
                        .with_color(self.edge)
                        .with_width(2.0),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

impl cosmic::Application for App {
    type Executor = iced::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "dev.troy.Cabin.Capture";

    fn core(&self) -> &app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut app::Core {
        &mut self.core
    }

    fn init(mut core: app::Core, (): ()) -> (Self, app::Task<Message>) {
        core.set_app_type(cosmic::core::AppType::System);
        let app = Self {
            core,
            commands: None,
            claimed: false,
            screens: Vec::new(),
            stills: HashMap::new(),
            windows: Vec::new(),
            thumbs: None,
            kind: Kind::Shot,
            mode: Mode::Region,
            heard: Heard::Off,
            mic: false,
            audio_apps: Vec::new(),
            pointer: None,
            drag: None,
            recording: None,
            zones: HashMap::new(),
        };
        let audio_apps = Task::future(async {
            Message::AudioApps(
                tokio::task::spawn_blocking(capture::audio_apps)
                    .await
                    .unwrap_or_default(),
            )
        })
        .map(cosmic::action::app);
        (
            app,
            Task::batch([
                text::load_font().map(|()| cosmic::action::app(Message::Done)),
                audio_apps,
            ]),
        )
    }

    fn style(&self) -> Option<iced::theme::Style> {
        Some(iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: palette::TEXT,
            icon_color: palette::TEXT,
        })
    }

    fn update(&mut self, message: Message) -> app::Task<Message> {
        match message {
            // A tick redraws the pill's clock.
            Message::Done | Message::Tick => {}
            Message::Claimed => {
                self.claimed = true;
                return self.open();
            }
            Message::Toggle if self.recording.is_some() => return self.stop(),
            Message::Toggle | Message::Cancel if self.recording.is_none() => std::process::exit(0),
            Message::Toggle | Message::Cancel => {}
            Message::Kind(kind) => self.kind = kind,
            Message::Mode(mode) => {
                self.mode = mode;
                // The cards' pictures, taken only once they're wanted.
                if mode == Mode::Window && self.thumbs.is_none() {
                    self.thumbs = Some(HashMap::new());
                    if let Some(commands) = &self.commands {
                        let _ = commands.send(Command::Thumbnails);
                    }
                }
            }
            Message::Pick(id) => return self.pick(id),
            Message::Hear(heard) => self.heard = heard,
            Message::Mic => self.mic = !self.mic,
            Message::AudioApps(apps) => self.audio_apps = apps,
            Message::Output(event, output) => return self.output(event, output),
            Message::Capture(event) => return self.captured(event),
            Message::Moved(id, at) => self.pointer = Some((id, at)),
            Message::Pressed(id) => {
                if self.recording.is_none() && self.mode == Mode::Region {
                    self.drag = self.pointer.filter(|(on, _)| *on == id);
                }
            }
            Message::Released(id) => return self.released(id),
            Message::Stop => return self.stop(),
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        widget::text("").into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        let Some(screen) = self.screens.iter().find(|s| s.surface == Some(id)) else {
            return widget::text("").into();
        };
        let recording = self.recording.is_some();
        let shade = canvas::Canvas::new(Shade {
            cover: self.cover(screen),
            edge: if recording {
                palette::OVER
            } else {
                palette::ACCENT
            },
        })
        .width(Length::Fill)
        .height(Length::Fill);
        let mut layers: Vec<Element<'_, Message>> = Vec::new();
        if recording {
            layers.push(shade.into());
            if let Some(pill) = self.pill(screen) {
                layers.push(
                    widget::container(self.stop_pill())
                        .padding(Padding {
                            top: pill.y,
                            left: pill.x,
                            right: 0.0,
                            bottom: 0.0,
                        })
                        .into(),
                );
            }
        } else {
            if let Some(still) = self.stills.get(&screen.name) {
                layers.push(
                    image(still.handle.clone())
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .content_fit(ContentFit::Fill)
                        .into(),
                );
            }
            let pointer = match self.mode {
                Mode::Region => mouse::Interaction::Crosshair,
                Mode::Window => mouse::Interaction::Idle,
                Mode::Screen => mouse::Interaction::Pointer,
            };
            layers.push(
                mouse_area(shade)
                    .on_move(move |at| Message::Moved(id, at))
                    .on_press(Message::Pressed(id))
                    .on_release(Message::Released(id))
                    .interaction(pointer)
                    .into(),
            );
            if self.mode == Mode::Window {
                layers.push(self.cards(screen));
            }
            layers.push(self.toolbar());
        }
        stack(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            capture::subscription().map(Message::Capture),
            bus(),
            event::listen_with(|event, _, _| match event {
                iced::Event::PlatformSpecific(event::PlatformSpecific::Wayland(
                    WaylandEvent::Output(event, output),
                )) => Some(Message::Output(event, output)),
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: Key::Named(Named::Escape),
                    ..
                }) => Some(Message::Cancel),
                _ => None,
            }),
        ];
        if self.recording.is_some() {
            subscriptions.push(time::every(Duration::from_secs(1)).map(|_| Message::Tick));
        }
        Subscription::batch(subscriptions)
    }
}

fn handle(image: &::image::RgbaImage) -> image::Handle {
    image::Handle::from_rgba(image.width(), image.height(), image.as_raw().clone())
}

/// The cut saved and copied, said so, and the end.
fn save(image: Arc<::image::RgbaImage>, crop: Rect) -> app::Task<Message> {
    Task::future(async move {
        match tokio::task::spawn_blocking(move || save_shot(&image, crop)).await {
            Ok(Ok((path, None))) => tell(
                "Screenshot copied",
                &format!("Saved to {}", path.display()),
                Some(&path),
                true,
            ),
            Ok(Ok((path, Some(why)))) => {
                tell("Screenshot saved, not copied", &why, Some(&path), true)
            }
            Ok(Err(why)) => tell("Screenshot failed", &why, None, false),
            Err(err) => tell("Screenshot failed", &err.to_string(), None, false),
        }
        std::process::exit(0)
    })
}

fn rect(r: Rect) -> Rectangle {
    Rectangle::new(
        Point::new(r.x as f32, r.y as f32),
        Size::new(r.width as f32, r.height as f32),
    )
}

fn span(a: Point, b: Point) -> Rectangle {
    Rectangle::new(
        Point::new(a.x.min(b.x), a.y.min(b.y)),
        Size::new((a.x - b.x).abs(), (a.y - b.y).abs()),
    )
}

/// A logical area of a screen in its still's pixels, rounded inward so a
/// recording never takes in the shade's edge.
fn pixels(area: Rectangle, logical: Size, (width, height): (u32, u32)) -> Rect {
    let (w, h) = (width as f32, height as f32);
    let (sx, sy) = (w / logical.width, h / logical.height);
    let x0 = (area.x * sx).ceil().clamp(0.0, w);
    let y0 = (area.y * sy).ceil().clamp(0.0, h);
    let x1 = ((area.x + area.width) * sx).floor().clamp(x0, w);
    let y1 = ((area.y + area.height) * sy).floor().clamp(y0, h);
    Rect {
        x: x0 as i32,
        y: y0 as i32,
        width: (x1 - x0) as i32,
        height: (y1 - y0) as i32,
    }
}

/// A new file under the home folder, named for now without spaces
/// (`Screenshot_2026-11-14_00-33-33.png`), as cosmic-screenshot names them.
fn file_in(dir: &str, what: &str, extension: &str) -> std::io::Result<PathBuf> {
    let dir = Path::new(&std::env::var_os("HOME").unwrap_or_default()).join(dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(format!(
        "{what}_{}.{extension}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    )))
}

/// The cut saved as a PNG and put on the clipboard: the file, and why it
/// isn't on the clipboard if it isn't.
fn save_shot(image: &::image::RgbaImage, crop: Rect) -> Result<(PathBuf, Option<String>), String> {
    let path = file_in("Pictures/Screenshots", "Screenshot", "png")
        .map_err(|err| format!("No place for it: {err}"))?;
    ::image::imageops::crop_imm(
        image,
        crop.x as u32,
        crop.y as u32,
        crop.width as u32,
        crop.height as u32,
    )
    .to_image()
    .save_with_format(&path, ::image::ImageFormat::Png)
    .map_err(|err| err.to_string())?;
    // wl-copy forks to serve the clipboard and returns at once.
    let copied = std::fs::File::open(&path).and_then(|file| {
        std::process::Command::new("wl-copy")
            .args(["--type", "image/png"])
            .stdin(file)
            .status()
    });
    let why = match copied {
        Ok(status) if status.success() => None,
        Ok(status) => Some(format!("wl-copy: {status}")),
        Err(err) => Some(format!("wl-copy: {err}")),
    };
    Ok((path, why))
}

/// A file on the clipboard the way a file manager puts it there, as a URI
/// list: chat apps, browsers and file managers paste it as the file.
fn copy_file(path: &Path) -> std::io::Result<()> {
    let mut wl_copy = std::process::Command::new("wl-copy")
        .args(["--type", "text/uri-list"])
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    // Dropped once written: wl-copy takes the end of its input as the cue to serve it.
    wl_copy
        .stdin
        .take()
        .expect("piped")
        .write_all(format!("{}\r\n", file_uri(path)).as_bytes())?;
    match wl_copy.wait()? {
        status if status.success() => Ok(()),
        status => Err(std::io::Error::other(status.to_string())),
    }
}

fn fail(why: String) -> app::Task<Message> {
    tracing::error!("cabin-capture: {why}");
    tell("Capture failed", &why, None, false);
    std::process::exit(1)
}

/// Said in a notification, by a helper (`cabin-capture --notify`) that
/// outlives this one, so a click on it can still show the file.
fn tell(summary: &str, body: &str, file: Option<&Path>, picture: bool) {
    let spawned = std::env::current_exe().and_then(|exe| {
        std::process::Command::new(exe)
            .arg("--notify")
            .args([summary, body])
            .arg(file.map(Path::as_os_str).unwrap_or_default())
            .arg(if picture { "picture" } else { "" })
            .process_group(0)
            .stdin(std::process::Stdio::null())
            .spawn()
    });
    if let Err(err) = spawned {
        tracing::warn!("cabin-capture: no notification: {err}");
    }
}

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, zbus::zvariant::Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    #[zbus(signal)]
    fn action_invoked(&self, id: u32, action_key: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn notification_closed(&self, id: u32, reason: u32) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.freedesktop.FileManager1",
    default_service = "org.freedesktop.FileManager1",
    default_path = "/org/freedesktop/FileManager1"
)]
trait FileManager {
    fn show_items(&self, uris: &[&str], startup_id: &str) -> zbus::Result<()>;
}

/// The helper: `--notify <summary> <body> <file or ""> <"picture" or "">`.
/// A click on the notification (from its pop-up or, later, the list) opens
/// the file's folder with it selected; it waits an hour at most.
async fn notified(
    summary: &str,
    body: &str,
    file: Option<PathBuf>,
    picture: bool,
) -> zbus::Result<()> {
    let session = zbus::Connection::session().await?;
    let notifications = NotificationsProxy::new(&session).await?;
    // Listening before it's up, so no click is missed.
    let mut invoked = notifications.receive_action_invoked().await?;
    let mut closed = notifications.receive_notification_closed().await?;
    let mut hints = HashMap::new();
    if let Some(file) = file.as_ref().filter(|_| picture) {
        hints.insert(
            "image-path",
            zbus::zvariant::Value::from(file.to_string_lossy().into_owned()),
        );
    }
    let actions: &[&str] = if file.is_some() {
        &["default", "Show in folder"]
    } else {
        &[]
    };
    let id = notifications
        .notify("Capture", 0, "", summary, body, actions, hints, 8000)
        .await?;
    let Some(file) = file else {
        return Ok(());
    };
    let give_up = tokio::time::sleep(Duration::from_secs(3600));
    tokio::pin!(give_up);
    loop {
        tokio::select! {
            Some(signal) = invoked.next() => {
                if signal.args().is_ok_and(|a| a.id == id) {
                    show(&session, &file).await;
                    return Ok(());
                }
            }
            // Expired (1) only leaves the pop-up for the list: still clickable.
            Some(signal) = closed.next() => {
                if signal.args().is_ok_and(|a| a.id == id && a.reason != 1) {
                    return Ok(());
                }
            }
            () = &mut give_up => return Ok(()),
        }
    }
}

/// The file shown in its folder by the file manager (COSMIC Files serves
/// FileManager1), else just the folder opened.
async fn show(session: &zbus::Connection, file: &Path) {
    let uri = file_uri(file);
    let shown = async {
        FileManagerProxy::new(session)
            .await?
            .show_items(&[&uri], "")
            .await
    };
    if let Err(err) = shown.await {
        tracing::warn!("cabin-capture: no file manager to show {uri}: {err}");
        let _ = std::process::Command::new("xdg-open")
            .arg(file.parent().unwrap_or(Path::new("/")))
            .spawn();
    }
}

fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

/// The running instance on the bus: the shortcut pressed again, and what
/// it records (`capture::RunningProxy` is the other end).
struct Remote(mpsc::UnboundedSender<()>);

#[zbus::interface(name = "dev.troy.Cabin.Capture")]
impl Remote {
    /// Cancels the choosing, or stops the recording.
    fn toggle(&self) {
        let _ = self.0.unbounded_send(());
    }

    fn recording(&self) -> String {
        RECORDING
            .lock()
            .map(|what| what.clone())
            .unwrap_or_default()
    }

    #[zbus(signal)]
    async fn recording_changed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::Result<()>;
}

/// What's recorded, told to cabind (its privacy pill) at once.
fn announce(what: String) -> app::Task<Message> {
    if let Ok(mut recording) = RECORDING.lock() {
        *recording = what;
    }
    Task::future(async {
        if let Some(connection) = CONNECTION.get()
            && let Ok(emitter) = zbus::object_server::SignalEmitter::new(connection, BUS_PATH)
            && let Err(err) = Remote::recording_changed(&emitter).await
        {
            tracing::warn!("cabin-capture: cannot say the recording changed: {err}");
        }
        cosmic::action::app(Message::Done)
    })
}

/// The bus name, held for life; if another instance has it, this is the
/// shortcut's second press: that one is told, and this one ends.
fn bus() -> Subscription<Message> {
    Subscription::run(|| {
        stream::channel(4, |mut output: mpsc::Sender<Message>| async move {
            let (remote, mut toggles) = mpsc::unbounded();
            let built = async {
                zbus::connection::Builder::session()?
                    .serve_at(BUS_PATH, Remote(remote))?
                    .name(BUS_NAME)?
                    .build()
                    .await
            };
            match built.await {
                Ok(connection) => _ = CONNECTION.set(connection),
                Err(zbus::Error::NameTaken) => {
                    let told = async {
                        RunningProxy::new(&zbus::Connection::session().await?)
                            .await?
                            .toggle()
                            .await
                    };
                    if let Err(err) = told.await {
                        tracing::warn!("cabin-capture: cannot reach the running one: {err}");
                    }
                    std::process::exit(0)
                }
                Err(err) => tracing::warn!(
                    "cabin-capture: no bus name, so the shortcut can't stop a recording: {err}"
                ),
            }
            let _ = output.send(Message::Claimed).await;
            while toggles.next().await.is_some() {
                if output.send(Message::Toggle).await.is_err() {
                    break;
                }
            }
            futures::future::pending().await
        })
    })
}

/// Whether another instance took the toggle: the shortcut's second press,
/// answered before any of the GUI starts (a whole app torn down takes seconds).
fn toggled_running() -> bool {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return false;
    };
    runtime
        .block_on(async {
            RunningProxy::new(&zbus::Connection::session().await?)
                .await?
                .toggle()
                .await
        })
        .is_ok()
}

fn main() -> iced::Result {
    cabin::hand_over_to_dev();
    cabin::prefer_integrated_gpu();
    cabin::init_logging();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [flag, summary, body, file, picture] = args.as_slice()
        && flag == "--notify"
    {
        let file = (!file.is_empty()).then(|| PathBuf::from(file));
        let told = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map(|runtime| runtime.block_on(notified(summary, body, file, picture == "picture")));
        if let Ok(Err(err)) = told {
            tracing::warn!("cabin-capture: no notification: {err}");
        }
        return Ok(());
    }
    if toggled_running() {
        return Ok(());
    }
    cosmic::app::run::<App>(
        app::Settings::default()
            .no_main_window(true)
            .exit_on_close(false),
        (),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_escapes() {
        assert_eq!(
            file_uri(Path::new(
                "/home/t/Pictures/Screenshot From 2026-11-14 00-33.png"
            )),
            "file:///home/t/Pictures/Screenshot%20From%202026-11-14%2000-33.png"
        );
    }

    #[test]
    fn pixels_round_inward() {
        // 125 %: 2048×1280 logical on 2560×1600.
        let logical = Size::new(2048.0, 1280.0);
        let area = Rectangle::new(Point::new(10.3, 20.0), Size::new(100.0, 50.0));
        assert_eq!(
            pixels(area, logical, (2560, 1600)),
            Rect {
                x: 13,
                y: 25,
                width: 124,
                height: 62
            }
        );
        // Hanging off the edge: clamped.
        let off = Rectangle::new(Point::new(2000.0, -10.0), Size::new(100.0, 30.0));
        assert_eq!(
            pixels(off, logical, (2560, 1600)),
            Rect {
                x: 2500,
                y: 0,
                width: 60,
                height: 25
            }
        );
    }
}
