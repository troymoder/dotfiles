// SPDX-License-Identifier: GPL-3.0-only

//! The quick settings' state: the controls and the audio panels, and the
//! System panel, which reads the same things. The daemon owns it and draws
//! it where it belongs; the panel's status button draws `status_bar`.

use std::time::{Duration, Instant};

use cosmic::{
    Task, app,
    iced::{self, Subscription},
    widget,
};
use zbus::zvariant::OwnedObjectPath;

use crate::{
    backend::{
        self,
        audio::Audio,
        bluetooth::Bluetooth,
        brightness::Brightness,
        camera::Cameras,
        dns::Choice,
        gpu::Gpus,
        graphics::Graphics,
        media::{Control, Media},
        network::{AccessPoint, Network, Networks},
        power::Power,
        privacy::Privacy,
        system::System,
    },
    bus,
    ui::system_pane::Section,
};

/// How long a change asked for is shown as under way before giving up.
const PENDING: Duration = Duration::from_secs(12);

/// A detail card, under the row of what it belongs to. One at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Card {
    Battery,
    Session,
    PowerMenu,
    Network,
    Wired,
    Bluetooth,
    PowerMode,
    Graphics,
    Dns,
    Camera,
    Privacy,
    Display,
    Keyboard,
    Output,
    Input,
}

impl Card {
    /// A menu by name, as `cabinctl open controls <menu>` gives it.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "battery" => Self::Battery,
            "session" | "lock" => Self::Session,
            "power" => Self::PowerMenu,
            "wifi" | "network" => Self::Network,
            "wired" => Self::Wired,
            "bluetooth" => Self::Bluetooth,
            "power-mode" | "profile" => Self::PowerMode,
            "graphics" => Self::Graphics,
            "dns" => Self::Dns,
            "camera" => Self::Camera,
            "privacy" => Self::Privacy,
            "display" => Self::Display,
            "keyboard" => Self::Keyboard,
            "output" | "sound" => Self::Output,
            "input" | "microphone" => Self::Input,
            _ => return None,
        })
    }
}

/// A change asked for and not yet seen: shown as under way meanwhile.
#[derive(Clone, Debug, PartialEq)]
pub enum Pending {
    Wifi(bool),
    Bluetooth(bool),
    Device(OwnedObjectPath, bool),
    Profile(String),
    Dns(Choice),
}

/// A slider being dragged: its level is what's shown, not the system's.
#[derive(Clone, Debug, PartialEq)]
pub enum Adjusting {
    Brightness(f32),
    Output(f32),
    Input(f32),
    /// An app's level, by its name.
    Stream(String, f32),
    /// A camera's image control: the device node, the control, its value.
    Camera(String, u32, i32),
}

/// What a right click offers: to hide something.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuFor {
    App(String),
    Player(String),
}

#[derive(Clone, Debug)]
pub enum Message {
    /// Where the pointer is on the overlay: a right-click menu opens there.
    CursorMoved(iced::Point),
    OpenMenu(MenuFor),
    CloseMenu,
    /// A section of the System panel folded or unfolded, or all of them.
    ToggleSection(Section),
    ToggleAllSections,
    Tick,
    Done,

    Network(Network),
    Networks(Option<Networks>),
    Bluetooth(Option<Bluetooth>),
    Power(Power),
    Graphics(Option<Graphics>),
    Brightness(Brightness),
    Privacy(Privacy),
    System(System),
    Gpus(Gpus),
    Cameras(Cameras),
    /// A picture from the camera on show in the Camera menu.
    Frame(backend::camera::Frame),
    Audio(Audio),
    MediaList(Vec<Media>),

    ToggleCard(Card),
    ToggleWifi,
    ToggleWired,
    /// A wired connection up or down, by its settings.
    SetWired(OwnedObjectPath, bool),
    JoinWifi(AccessPoint),
    Disconnect(OwnedObjectPath),
    ToggleBluetooth,
    ConnectDevice(OwnedObjectPath, bool),
    SetProfile(String),
    /// The Power tile pressed: on to the next power profile.
    CycleProfile,
    /// An app's live level, for the meter in its mixer slider.
    StreamLevel(String, f32),
    SetGraphics(u32),
    SetDns(Choice),
    SetBrightness(f32),
    /// The keyboard light off, or back to where it was.
    ToggleKeyboard,
    SelectCamera(String),
    /// The preview's size, or back to the camera's choice.
    SetPreviewSize(Option<(u32, u32)>),
    ToggleSizes,
    SetCameraControl(String, u32, i32),
    ResetCamera(String),
    /// Every network interface listed in the System menu, or just the one in use.
    ToggleNetAll,
    StopShare(u32),
    /// The panel with a menu already open: the privacy pill on the panel does this.
    OpenCard(Card),
    /// Charging to stop short of full, or not.
    SetChargeLimit(bool),
    /// A power-saving timeout's choices shown or hidden, and one picked.
    ToggleIdle(backend::idle::Timeout),
    SetIdle(backend::idle::Timeout, Option<u32>),
    /// The displays as cosmic-randr lists them.
    Displays(Vec<backend::display::Output>),
    /// Night light as cosmic-nightlight has it, switched, or its settings.
    NightLight(Option<backend::nightlight::NightLight>),
    ToggleNightLight,
    NightLightSettings,
    /// The microphone's level and the output's, for the meters under their sliders.
    MicLevel(f32),
    OutputLevel(f32),
    /// For looking: the popup open with the brightness slider held as if dragged.
    DebugBubble,
    /// For looking: the popup open with a pretend screen share.
    DebugCoral,
    /// The wheel over the panel icons: the volume, in steps.
    ScrollVolume(iced::mouse::ScrollDelta),
    SetInputMute(bool),
    AdjustDone,
    SetOutput(f32),
    SetInput(f32),
    ToggleOutputMute,
    ToggleInputMute,
    SetSink(String),
    SetSource(String),
    SetStream(String, Vec<u32>, f32),
    HideApp(String),
    HidePlayer(String),
    UnhidePlayer(String),
    ToggleStreamCard(String),
    MoveStream(Vec<u32>, String),
    UnhideApp(String),
    ShowAllWifi,
    Media(String, Control),
    Seek(String, f32),
    SeekDone(String),
    RaisePlayer(String),
    SetShuffle(String, bool),
    SetLoop(String, String),
    /// A mixer app's icon pressed: muted, or back.
    StreamIcon(String),
    /// This app on, every other muted.
    SoloStream(String),
    Escape,
    SetKeyboardLevel(i32),
    Screenshot,

    Lock,
    LogOut,
    Suspend,
    Restart,
    PowerOff,
    Settings(Option<&'static str>),
}

#[derive(Default)]
pub struct App {
    /// The controls and audio panels are on screen: what only they show is
    /// read meanwhile (cameras, the level meters).
    pub(crate) shown: bool,
    /// The System panel is on screen: sampling is fast, processes are read.
    pub(crate) system_shown: bool,
    /// Something asked for the panels to close (a button that hands off to
    /// another app), or to open (a debug look); the daemon sees to it.
    pub(crate) wants_close: bool,
    pub(crate) wants_open: bool,
    /// The System panel's folded sections.
    pub(crate) sections_closed: Vec<Section>,

    pub(crate) network: Option<Network>,
    pub(crate) networks: Option<Networks>,
    pub(crate) bluetooth: Option<Bluetooth>,
    pub(crate) power: Option<Power>,
    pub(crate) graphics: Option<Graphics>,
    pub(crate) brightness: Option<Brightness>,
    pub(crate) privacy: Privacy,
    pub(crate) system: Option<System>,
    pub(crate) gpus: Option<Gpus>,
    pub(crate) cameras: Option<Cameras>,
    /// The camera whose details the Camera menu shows.
    pub(crate) camera_sel: Option<String>,
    /// The latest picture from the camera on show, while its menu is open.
    pub(crate) preview: Option<widget::image::Handle>,
    /// The size asked of the camera for the preview; the camera's own choice when none.
    pub(crate) preview_size: Option<(u32, u32)>,
    /// The list of sizes open under the Resolution row.
    pub(crate) sizes_open: bool,
    /// The power-saving timeouts, read when the Battery menu opens, and the
    /// one whose choices are open.
    pub(crate) idle: backend::idle::Idle,
    pub(crate) idle_open: Option<backend::idle::Timeout>,
    /// Every interface listed under Network in the System menu.
    pub(crate) net_all: bool,
    /// The last while of the machine's power draw, for the graph.
    pub(crate) draw_history: Vec<f32>,
    /// Where the keyboard light was before it was turned off.
    keyboard_last: i32,
    /// Wheel movement over the panel icons not yet a whole step.
    scrolled: f32,
    /// What the meters under the output and microphone sliders show, 0 to 1:
    /// a peak that falls back slowly.
    pub(crate) output_level: f32,
    pub(crate) mic_level: f32,
    /// Each playing app's meter, by name, falling back the same way.
    pub(crate) stream_levels: Vec<(String, f32)>,
    /// The displays, read when the Display menu opens.
    pub(crate) displays: Vec<backend::display::Output>,
    /// Night light, where cosmic-nightlight is installed.
    pub(crate) night: Option<backend::nightlight::NightLight>,
    pub(crate) audio: Option<Audio>,
    /// Apps that have played lately, in the order they first showed, each
    /// with when it was last heard of: gone ones stay a while.
    pub(crate) streams: Vec<(backend::audio::Stream, Instant)>,
    /// Apps kept out of the mixer.
    pub(crate) hidden_apps: Vec<String>,
    /// Players kept out of the media cards, by name.
    pub(crate) hidden_players: Vec<String>,
    /// Where the pointer is on the overlay.
    cursor: Option<iced::Point>,
    /// A right-click menu, and where it opened.
    pub(crate) menu: Option<(MenuFor, iced::Point)>,
    /// The app whose outputs are listed under its row in the mixer.
    pub(crate) stream_card: Option<String>,
    /// Every Wi-Fi network in reach shown, not just the first few.
    pub(crate) wifi_all: bool,
    pub(crate) media: Vec<Media>,
    /// A seek bar being dragged: the player, and where to.
    pub(crate) seeking: Option<(String, f32)>,
    /// When a slider was let go: its level stays shown a moment, until the
    /// system has caught up, so it doesn't jump back.
    released: Option<Instant>,

    pub(crate) card: Option<Card>,
    pub(crate) pending: Vec<(Pending, Instant)>,
    pub(crate) adjusting: Option<Adjusting>,
}

/// A backend call run for its effect; what it says is logged.
fn run<F>(what: &'static str, future: F) -> app::Task<Message>
where
    F: Future<Output = zbus::Result<()>> + Send + 'static,
{
    Task::future(async move {
        if let Err(err) = future.await {
            tracing::warn!("cannot {what}: {err}");
        }
        cosmic::action::app(Message::Done)
    })
}

fn load_networks(scan: bool) -> app::Task<Message> {
    Task::future(async move {
        let networks = backend::network::networks(scan)
            .await
            .unwrap_or_else(|err| {
                tracing::warn!("cannot list Wi-Fi networks: {err}");
                None
            });
        cosmic::action::app(Message::Networks(networks))
    })
}

impl App {
    /// The panels closed: what was open in them forgotten. The daemon takes
    /// them off screen.
    pub(crate) fn close_popup(&mut self) -> app::Task<Message> {
        self.wants_close = true;
        self.forget();
        Task::none()
    }

    /// The panels asked to show (for a debug look); the daemon opens them.
    fn open_popup(&mut self) -> app::Task<Message> {
        self.wants_open = true;
        Task::none()
    }

    /// What was open in the panels, forgotten as they close.
    pub(crate) fn forget(&mut self) {
        self.card = None;
        self.menu = None;
        self.stream_card = None;
        self.preview = None;
        self.sizes_open = false;
        self.idle_open = None;
        self.mic_level = 0.0;
        self.output_level = 0.0;
        self.stream_levels.clear();
        self.wifi_all = false;
        self.networks = None;
    }

    /// Sampling fast while something shows what it samples.
    pub(crate) fn sync_fast(&self) {
        backend::system::set_fast(
            self.system_shown || (self.shown && self.card == Some(Card::Graphics)),
        );
    }

    /// The output's level and mute as shown (while dragged, the dragged
    /// level), for the on-screen bar.
    pub(crate) fn output_shown(&self) -> Option<(f32, bool)> {
        let output = self.audio.as_ref()?.output?;
        let level = match self.adjusting {
            Some(Adjusting::Output(level)) => level,
            _ => output.level,
        };
        Some((level, output.muted))
    }

    pub(crate) fn pending(&self, is: impl Fn(&Pending) -> bool) -> Option<&Pending> {
        self.pending.iter().map(|(p, _)| p).find(|p| is(p))
    }

    fn ask(&mut self, pending: Pending) {
        self.pending
            .retain(|(p, _)| std::mem::discriminant(p) != std::mem::discriminant(&pending));
        self.pending.push((pending, Instant::now()));
    }

    /// Whatever's been asked for and has since come to be, forgotten.
    fn settle(&mut self) {
        let network = self.network.as_ref();
        let bluetooth = self.bluetooth.as_ref();
        let power = self.power.as_ref();
        self.pending.retain(|(pending, since)| {
            if since.elapsed() > PENDING {
                return false;
            }
            let done = match pending {
                Pending::Wifi(on) => network.is_some_and(|n| n.wifi_enabled == *on),
                Pending::Bluetooth(on) => bluetooth.is_some_and(|b| b.powered == *on),
                Pending::Device(path, on) => bluetooth.is_some_and(|b| {
                    b.devices
                        .iter()
                        .any(|d| &d.path == path && d.connected == *on)
                }),
                Pending::Profile(profile) => power
                    .and_then(|p| p.profiles.as_ref())
                    .is_some_and(|p| &p.active == profile),
                Pending::Dns(choice) => network
                    .and_then(|n| n.dns.as_ref())
                    .is_some_and(|d| &d.choice == choice),
            };
            !done
        });
    }

    /// Apps' streams kept across snapshots: one that stops is kept a while
    /// (its slider still shows), and the oldest go once the list is long.
    fn remember_streams(&mut self, now: &[backend::audio::Stream]) {
        const KEEP: Duration = Duration::from_secs(120);
        const MOST: usize = 5;
        let at = Instant::now();
        for stream in now {
            match self.streams.iter_mut().find(|(s, _)| s.app == stream.app) {
                Some(known) => *known = (stream.clone(), at),
                None => self.streams.push((stream.clone(), at)),
            }
        }
        // Gone ones: still there, paused, until forgotten.
        for (stream, seen) in &mut self.streams {
            if !now.iter().any(|s| s.app == stream.app) {
                stream.corked = true;
                stream.indices.clear();
                let _ = seen;
            }
        }
        self.streams
            .retain(|(stream, seen)| !stream.indices.is_empty() || seen.elapsed() < KEEP);
        while self.streams.len() > MOST {
            // The one unheard of longest, among those not playing now.
            let Some(oldest) = self
                .streams
                .iter()
                .enumerate()
                .filter(|(_, (s, _))| s.indices.is_empty())
                .min_by_key(|(_, (_, seen))| *seen)
                .map(|(i, _)| i)
            else {
                break;
            };
            self.streams.remove(oldest);
        }
    }

    /// The battery of the Bluetooth device a sink is, by the address in
    /// the sink's name (`bluez_output.AA_BB_...`).
    pub(crate) fn sink_battery(&self, sink: &str) -> Option<u8> {
        let address = sink
            .strip_prefix("bluez_output.")?
            .split('.')
            .next()?
            .replace('_', ":")
            .to_uppercase();
        let bluetooth = self.bluetooth.as_ref()?;
        bluetooth
            .devices
            .iter()
            .find(|d| {
                d.path
                    .as_str()
                    .to_uppercase()
                    .ends_with(&address.replace(':', "_"))
            })
            .and_then(|d| d.battery)
    }

    fn output_device_battery(&self) -> Option<u8> {
        let audio = self.audio.as_ref()?;
        self.sink_battery(&audio.default_sink)
    }

    /// What the panel's status button draws.
    pub(crate) fn status_bar(&self, open: bool) -> bus::StatusBar {
        let icons = self
            .bar_icons()
            .into_iter()
            .map(|(icon, tip)| (icon.to_owned(), tip))
            .collect();
        let battery = self
            .power
            .as_ref()
            .and_then(|p| p.battery.as_ref())
            .map(|b| (b.icon(), b.percentage as f32, Self::battery_tip(b)));
        let battery_low = self
            .power
            .as_ref()
            .and_then(|p| p.battery.as_ref())
            .is_some_and(|b| b.low());
        bus::StatusBar {
            open,
            icons,
            headphones: self.output_device_battery(),
            battery,
            battery_low,
            privacy: self.privacy_pill(),
        }
    }

    /// What the System button draws: CPU and memory as two small bars.
    pub(crate) fn system_bar(&self, open: bool) -> bus::SystemBar {
        let Some(system) = &self.system else {
            return bus::SystemBar {
                open,
                ..bus::SystemBar::default()
            };
        };
        let memory = system.memory.used as f32 / system.memory.total.max(1) as f32;
        bus::SystemBar {
            open,
            cpu: (system.cpu.percent / 100.0).clamp(0.0, 1.0),
            memory: memory.clamp(0.0, 1.0),
            label: format!("{:.0} %", system.cpu.percent),
            tip: format!(
                "CPU {:.0} % · memory {} of {}",
                system.cpu.percent,
                backend::system::bytes(system.memory.used),
                backend::system::bytes(system.memory.total)
            ),
        }
    }

    /// The icons on the panel, each with what its tooltip says.
    fn bar_icons(&self) -> Vec<(&'static str, String)> {
        let mut icons = Vec::new();
        if let Some(graphics) = &self.graphics {
            if graphics.power < 2 {
                let state = if graphics.power == 0 {
                    "dedicated GPU active"
                } else {
                    "dedicated GPU asleep"
                };
                icons.push((graphics.icon(), format!("Graphics: {state}")));
            }
        }
        if let Some(bluetooth) = &self.bluetooth {
            if bluetooth.powered {
                icons.push((
                    "bluetooth-active-symbolic",
                    format!("Bluetooth: {}", bluetooth.status()),
                ));
            }
        }
        if let Some(profiles) = self.power.as_ref().and_then(|p| p.profiles.as_ref()) {
            let profile = &profiles.active;
            icons.push((
                crate::backend::power::Profiles::icon(profile),
                format!(
                    "Power mode: {}",
                    crate::backend::power::Profiles::name(profile)
                ),
            ));
        }
        match &self.network {
            Some(network) if network.wired.is_some() => icons.push((
                "network-wired-symbolic",
                format!("Wired: {}", network.wired_status()),
            )),
            Some(network) if network.has_wifi => icons.push((
                network.wifi_icon(),
                format!("Wi-Fi: {}", network.wifi_status()),
            )),
            Some(_) => {}
            None => icons.push(("network-wireless-offline-symbolic", "No network".to_owned())),
        }
        if let Some(output) = self.audio.as_ref().and_then(|a| a.output) {
            let level = if output.muted {
                "muted".to_owned()
            } else {
                format!("{:.0} %", output.level * 100.0)
            };
            icons.push((output.output_icon(), format!("Volume: {level}")));
        }
        icons
    }

    /// The battery's tooltip: its charge and what it's doing.
    fn battery_tip(battery: &crate::backend::power::Battery) -> String {
        let state = if battery.full {
            "fully charged".to_owned()
        } else if battery.held {
            "plugged in, not charging".to_owned()
        } else if battery.charging {
            battery.time_left.map_or_else(
                || "charging".to_owned(),
                |s| {
                    format!(
                        "charging, {} in {}",
                        battery.target_name().to_lowercase(),
                        duration(s)
                    )
                },
            )
        } else {
            battery.time_left.map_or_else(
                || "on battery".to_owned(),
                |s| format!("{} left", duration(s)),
            )
        };
        format!("Battery {:.0} %, {state}", battery.percentage)
    }

    /// The pill on the panel while something watches or listens: amber,
    /// coral when the screen is shared. Pressing it opens the Privacy menu.
    fn privacy_pill(&self) -> Option<bus::PrivacyPill> {
        let camera = !self.privacy.camera.is_empty()
            || self
                .cameras
                .as_ref()
                .is_some_and(|c| c.list.iter().any(|c| !c.users.is_empty()));
        let mic = !self.privacy.microphone.is_empty();
        let screen = self.privacy.sharing_screen();
        if !(camera || mic || screen) {
            return None;
        }
        let mic_muted = self
            .audio
            .as_ref()
            .and_then(|a| a.input)
            .is_some_and(|i| i.muted);
        let mut parts = Vec::new();
        if screen {
            parts.push("screen shared");
        }
        if camera {
            parts.push("camera in use");
        }
        if mic {
            parts.push("microphone in use");
        }
        let tip = parts.join(", ");
        let tip = tip[..1].to_uppercase() + &tip[1..];
        Some(bus::PrivacyPill {
            screen,
            camera,
            mic,
            mic_muted,
            tip,
        })
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            hidden_apps: backend::audio::load_hidden("apps"),
            hidden_players: backend::audio::load_hidden("players"),
            // The System panel starts folded: each section a line.
            sections_closed: Section::ALL.to_vec(),
            ..Self::default()
        }
    }

    pub fn update(&mut self, message: Message) -> app::Task<Message> {
        match message {
            Message::ToggleSection(section) => {
                if !self.sections_closed.contains(&section) {
                    self.sections_closed.push(section);
                } else {
                    self.sections_closed.retain(|s| *s != section);
                }
            }
            Message::ToggleAllSections => {
                // All folded: all open. Else all folded.
                if Section::ALL
                    .iter()
                    .all(|s| self.sections_closed.contains(s))
                {
                    self.sections_closed.clear();
                } else {
                    self.sections_closed = Section::ALL.to_vec();
                }
            }
            Message::DebugCoral => {
                self.privacy.screen.push(backend::privacy::Share {
                    app: "Chromium".to_owned(),
                    what: "Sharing entire screen · eDP-1".to_owned(),
                    node: 0,
                });
                self.privacy.microphone.push("Discord".to_owned());
                if !self.shown {
                    return self.open_popup();
                }
            }
            Message::DebugBubble => {
                let level = self
                    .brightness
                    .map_or(0.6, |b| b.level as f32 / b.max.max(1) as f32);
                self.adjusting = Some(Adjusting::Brightness(level));
                self.released = None;
                if !self.shown {
                    return self.open_popup();
                }
            }
            Message::Displays(displays) => self.displays = displays,
            Message::NightLight(night) => self.night = night,
            Message::ToggleNightLight => {
                // Shown switched at once; it takes a second or two on screen.
                if let Some(night) = self.night.as_mut() {
                    night.on = if night.on.is_some() {
                        None
                    } else {
                        Some(night.temperature)
                    };
                }
                return Task::future(async {
                    backend::nightlight::toggle().await;
                    cosmic::action::app(Message::NightLight(backend::nightlight::read()))
                });
            }
            Message::NightLightSettings => {
                backend::nightlight::settings();
                return self.close_popup();
            }
            // A peak shows at once and falls back over a few tenths of a second.
            Message::MicLevel(level) => self.mic_level = level.max(self.mic_level * 0.75),
            Message::OutputLevel(level) => self.output_level = level.max(self.output_level * 0.75),
            Message::StreamLevel(app, level) => {
                match self.stream_levels.iter_mut().find(|(a, _)| *a == app) {
                    Some((_, shown)) => *shown = level.max(*shown * 0.75),
                    None => self.stream_levels.push((app, level)),
                }
            }
            Message::CycleProfile => {
                let Some(profiles) = self.power.as_ref().and_then(|p| p.profiles.as_ref()) else {
                    return Task::none();
                };
                // From the one shown: a press before the last took is the next.
                let active = match self.pending(|p| matches!(p, Pending::Profile(_))) {
                    Some(Pending::Profile(profile)) => profile.clone(),
                    _ => profiles.active.clone(),
                };
                let available = &profiles.available;
                let Some(next) = available
                    .iter()
                    .position(|p| *p == active)
                    .map(|i| available[(i + 1) % available.len()].clone())
                    .or_else(|| available.first().cloned())
                else {
                    return Task::none();
                };
                return self.update(Message::SetProfile(next));
            }
            Message::SetChargeLimit(enabled) => {
                if let Some(limit) = self
                    .power
                    .as_mut()
                    .and_then(|p| p.battery.as_mut())
                    .and_then(|b| b.limit.as_mut())
                {
                    limit.enabled = enabled;
                }
                return Task::future(async move {
                    backend::power::set_charge_limit(enabled).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::ToggleIdle(timeout) => {
                self.idle_open = if self.idle_open == Some(timeout) {
                    None
                } else {
                    Some(timeout)
                }
            }
            Message::SetIdle(timeout, ms) => {
                self.idle_open = None;
                if let Some(i) = backend::idle::Timeout::ALL
                    .iter()
                    .position(|t| *t == timeout)
                {
                    self.idle[i] = ms;
                }
                backend::idle::set(timeout, ms);
            }
            Message::OpenCard(card) => {
                // Opened as a press on it would: whatever it reads is read.
                let load = if self.card != Some(card) {
                    self.update(Message::ToggleCard(card))
                } else {
                    Task::none()
                };
                if !self.shown {
                    return Task::batch([load, self.open_popup()]);
                }
                return load;
            }
            Message::ScrollVolume(delta) => {
                // A notch of the wheel is a step; the trackpad adds up to one.
                let notches = match delta {
                    iced::mouse::ScrollDelta::Lines { y, .. } => y,
                    iced::mouse::ScrollDelta::Pixels { y, .. } => y / 30.0,
                };
                self.scrolled += notches;
                let steps = self.scrolled.trunc();
                if steps == 0.0 {
                    return Task::none();
                }
                self.scrolled -= steps;
                // From the level shown: steps come faster than the system
                // reports the last one.
                let Some((shown, _)) = self.output_shown() else {
                    return Task::none();
                };
                let mut level = (shown * 100.0 + steps * 5.0)
                    .round()
                    .clamp(0.0, backend::audio::MAX * 100.0);
                if (level - 100.0).abs() < 3.0 {
                    level = 100.0;
                }
                // Let go at once: the level shown follows the system again
                // a moment after the last step.
                let task = self.update(Message::SetOutput(level / 100.0));
                self.released = Some(Instant::now());
                return task;
            }
            Message::CursorMoved(position) => self.cursor = Some(position),
            Message::OpenMenu(what) => {
                let at = self.cursor.unwrap_or(iced::Point::ORIGIN);
                self.menu = Some((what, at));
            }
            Message::CloseMenu => self.menu = None,
            Message::Tick => {
                self.settle();
                let now: Vec<backend::audio::Stream> = self
                    .audio
                    .as_ref()
                    .map(|a| a.streams.clone())
                    .unwrap_or_default();
                self.remember_streams(&now);
                if self
                    .released
                    .is_some_and(|at| at.elapsed() > Duration::from_millis(1500))
                {
                    self.released = None;
                    self.adjusting = None;
                }
            }
            Message::Done => {}

            Message::Network(network) => {
                let refresh =
                    self.card == Some(Card::Network) && self.network.as_ref() != Some(&network);
                self.network = Some(network);
                self.settle();
                if refresh {
                    return load_networks(false);
                }
            }
            Message::Networks(networks) => self.networks = networks,
            Message::Bluetooth(bluetooth) => {
                self.bluetooth = bluetooth;
                self.settle();
            }
            Message::Power(power) => {
                self.power = Some(power);
                self.settle();
            }
            Message::Graphics(graphics) => {
                self.graphics = graphics;
                self.settle();
            }
            Message::Brightness(brightness) => {
                if !matches!(self.adjusting, Some(Adjusting::Brightness(_))) {
                    self.brightness = Some(brightness);
                }
            }
            Message::Privacy(privacy) => self.privacy = privacy,
            Message::System(system) => {
                self.system = Some(system);
                if let Some(watts) = self.draw_total() {
                    self.draw_history.push(watts);
                    if self.draw_history.len() > 40 {
                        self.draw_history.remove(0);
                    }
                }
            }
            Message::Gpus(gpus) => self.gpus = Some(gpus),
            Message::Cameras(cameras) => {
                if !matches!(self.adjusting, Some(Adjusting::Camera(..))) {
                    self.cameras = Some(cameras);
                }
            }
            Message::SelectCamera(id) => {
                if self.camera_sel.as_ref() != Some(&id) {
                    self.preview = None;
                }
                self.camera_sel = Some(id);
            }
            // One handle per picture: the renderer keeps its texture between redraws.
            Message::SetPreviewSize(size) => {
                self.preview_size = size;
                self.preview = None;
                self.sizes_open = false;
            }
            Message::ToggleSizes => self.sizes_open = !self.sizes_open,
            Message::Frame(frame) => {
                self.preview = Some(widget::image::Handle::from_rgba(
                    frame.width,
                    frame.height,
                    frame.rgba.as_ref().clone(),
                ))
            }
            Message::SetCameraControl(node, id, value) => {
                self.released = None;
                self.adjusting = Some(Adjusting::Camera(node.clone(), id, value));
                return Task::future(async move {
                    backend::camera::set_control(node, id, value).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::ResetCamera(node) => {
                return Task::future(async move {
                    backend::camera::reset_controls(node).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::ToggleNetAll => self.net_all = !self.net_all,
            Message::StopShare(node) => {
                return Task::future(async move {
                    backend::privacy::stop_share(node).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::SetInputMute(muted) => {
                if let Some(input) = self.audio.as_mut().and_then(|a| a.input.as_mut()) {
                    input.muted = muted;
                }
                return Task::future(async move {
                    backend::audio::set_input_mute(muted).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::Audio(audio) => {
                // Keep a level being dragged; take the rest.
                let mut audio = audio;
                match &self.adjusting {
                    Some(Adjusting::Output(_)) => {
                        audio.output = self.audio.as_ref().and_then(|a| a.output).or(audio.output)
                    }
                    Some(Adjusting::Input(_)) => {
                        audio.input = self.audio.as_ref().and_then(|a| a.input).or(audio.input)
                    }
                    Some(Adjusting::Stream(app, level)) => {
                        if let Some(stream) = audio.streams.iter_mut().find(|s| s.app == *app) {
                            stream.level = *level;
                        }
                    }
                    _ => {}
                }
                self.remember_streams(&audio.streams);
                self.audio = Some(audio);
            }
            Message::MediaList(media) => {
                // Players keep the place they first took: pausing one doesn't
                // move it. New ones go to the end.
                let mut ordered: Vec<Media> = Vec::with_capacity(media.len());
                for known in &self.media {
                    if let Some(now) = media.iter().find(|m| m.service == known.service) {
                        ordered.push(now.clone());
                    }
                }
                for now in media {
                    if !ordered.iter().any(|m| m.service == now.service) {
                        ordered.push(now);
                    }
                }
                self.media = ordered;
            }
            Message::ToggleCard(card) => {
                if self.card == Some(card) {
                    self.card = None;
                    self.preview = None;
                    return Task::none();
                }
                self.preview = None;
                self.card = Some(card);
                self.wifi_all = false;
                if card == Card::Network {
                    return load_networks(true);
                }
                if card == Card::Battery {
                    self.idle = backend::idle::read();
                    self.idle_open = None;
                }
                if card == Card::Display {
                    self.night = backend::nightlight::read();
                    return Task::future(async {
                        cosmic::action::app(Message::Displays(backend::display::now().await))
                    });
                }
            }
            Message::ToggleWifi => {
                let on = !self.network.as_ref().is_some_and(|n| n.wifi_enabled);
                self.ask(Pending::Wifi(on));
                return run("turn Wi-Fi on or off", backend::network::set_wifi(on));
            }
            Message::JoinWifi(network) => {
                let Some(networks) = &self.networks else {
                    return Task::none();
                };
                let device = networks.device.clone();
                return Task::future(async move {
                    match backend::network::join(device, network).await {
                        Ok(backend::network::Joined::Connecting) => {
                            cosmic::action::app(Message::Done)
                        }
                        Ok(backend::network::Joined::NeedsSettings) => {
                            cosmic::action::app(Message::Settings(Some("wireless")))
                        }
                        Err(err) => {
                            tracing::warn!("cannot join the network: {err}");
                            cosmic::action::app(Message::Done)
                        }
                    }
                });
            }
            Message::Disconnect(active) => {
                return run("disconnect", backend::network::disconnect(active));
            }
            Message::ToggleWired => {
                let Some(network) = &self.network else {
                    return Task::none();
                };
                return match (&network.wired, &network.wired_device) {
                    (Some(wired), _) => run(
                        "disconnect the cable",
                        backend::network::disconnect(wired.active.clone()),
                    ),
                    (None, Some(device)) => run(
                        "connect the cable",
                        backend::network::connect_wired(device.clone(), None),
                    ),
                    (None, None) => Task::none(),
                };
            }
            Message::SetWired(settings, up) => {
                let Some(network) = &self.network else {
                    return Task::none();
                };
                let connection = network
                    .wired_connections
                    .iter()
                    .find(|c| c.settings == settings);
                return match (
                    up,
                    connection.and_then(|c| c.active.clone()),
                    &network.wired_device,
                ) {
                    (false, Some(active), _) => {
                        run("disconnect the cable", backend::network::disconnect(active))
                    }
                    (true, None, Some(device)) => run(
                        "connect the cable",
                        backend::network::connect_wired(device.clone(), Some(settings)),
                    ),
                    _ => Task::none(),
                };
            }
            Message::ToggleBluetooth => {
                let Some(bluetooth) = &self.bluetooth else {
                    return Task::none();
                };
                let on = !bluetooth.powered;
                let adapter = bluetooth.adapter.clone();
                self.ask(Pending::Bluetooth(on));
                return run(
                    "turn Bluetooth on or off",
                    backend::bluetooth::set_powered(adapter, on),
                );
            }
            Message::ConnectDevice(device, on) => {
                self.ask(Pending::Device(device.clone(), on));
                return run(
                    "connect the device",
                    backend::bluetooth::connect(device, on),
                );
            }
            Message::SetProfile(profile) => {
                self.ask(Pending::Profile(profile.clone()));
                return Task::future(async move {
                    backend::power::set_profile(profile).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::SetGraphics(mode) => {
                // A switch needs a new session: the dialog asks, then does both.
                backend::session::graphics_logout(mode, Graphics::mode_name(mode));
                return self.close_popup();
            }
            Message::SetDns(choice) => {
                let Some(dns) = self.network.as_ref().and_then(|n| n.dns.clone()) else {
                    return Task::none();
                };
                self.ask(Pending::Dns(choice.clone()));
                return run("set the DNS servers", backend::dns::set(dns, choice));
            }
            Message::SetBrightness(level) => {
                self.released = None;
                self.adjusting = Some(Adjusting::Brightness(level));
                let Some(brightness) = self.brightness else {
                    return Task::none();
                };
                let value = (level * brightness.max as f32).round().max(1.0) as i32;
                return Task::future(async move {
                    backend::brightness::set(value).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::SetKeyboardLevel(level) => {
                if let Some(keyboard) = self.power.as_mut().and_then(|p| p.keyboard.as_mut()) {
                    keyboard.level = level;
                }
                return Task::future(async move {
                    backend::power::set_keyboard(level).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::ToggleKeyboard => {
                let Some(keyboard) = self.power.as_ref().and_then(|p| p.keyboard.as_ref()) else {
                    return Task::none();
                };
                let next = if keyboard.level > 0 {
                    self.keyboard_last = keyboard.level;
                    0
                } else if self.keyboard_last > 0 {
                    self.keyboard_last
                } else {
                    keyboard.max
                };
                return self.update(Message::SetKeyboardLevel(next));
            }
            Message::AdjustDone => {
                // The level stays as set until the system says the same.
                match &self.adjusting {
                    Some(Adjusting::Brightness(level)) => {
                        if let Some(b) = self.brightness.as_mut() {
                            b.level = (level * b.max as f32).round() as i32;
                        }
                    }
                    Some(Adjusting::Output(level)) => {
                        if let Some(o) = self.audio.as_mut().and_then(|a| a.output.as_mut()) {
                            o.level = *level;
                        }
                    }
                    Some(Adjusting::Input(level)) => {
                        if let Some(i) = self.audio.as_mut().and_then(|a| a.input.as_mut()) {
                            i.level = *level;
                        }
                    }
                    Some(Adjusting::Stream(app, level)) => {
                        if let Some((s, _)) = self.streams.iter_mut().find(|(s, _)| s.app == *app) {
                            s.level = *level;
                        }
                    }
                    Some(Adjusting::Camera(node, id, value)) => {
                        if let Some(control) = self
                            .cameras
                            .as_mut()
                            .and_then(|c| c.list.iter_mut().find(|c| c.node == *node))
                            .and_then(|c| c.controls.iter_mut().find(|c| c.id == *id))
                        {
                            control.value = *value;
                        }
                    }
                    _ => {}
                }
                self.released = Some(Instant::now());
            }
            Message::SetOutput(level) => {
                self.released = None;
                self.adjusting = Some(Adjusting::Output(level));
                // Moving it unmutes.
                let muted = self
                    .audio
                    .as_mut()
                    .and_then(|a| a.output.as_mut())
                    .is_some_and(|o| std::mem::replace(&mut o.muted, false));
                return Task::future(async move {
                    if muted {
                        backend::audio::set_output_mute(false).await;
                    }
                    backend::audio::set_output(level).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::SetInput(level) => {
                self.released = None;
                self.adjusting = Some(Adjusting::Input(level));
                let muted = self
                    .audio
                    .as_mut()
                    .and_then(|a| a.input.as_mut())
                    .is_some_and(|i| std::mem::replace(&mut i.muted, false));
                return Task::future(async move {
                    if muted {
                        backend::audio::set_input_mute(false).await;
                    }
                    backend::audio::set_input(level).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::ToggleOutputMute => {
                return Task::future(async {
                    backend::audio::toggle_output_mute().await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::ToggleInputMute => {
                return Task::future(async {
                    backend::audio::toggle_input_mute().await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::SetSink(name) => {
                return Task::future(async move {
                    backend::audio::set_sink(name).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::SetSource(name) => {
                return Task::future(async move {
                    backend::audio::set_source(name).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::SetStream(app, indices, level) => {
                self.released = None;
                self.adjusting = Some(Adjusting::Stream(app, level));
                return Task::future(async move {
                    backend::audio::set_stream_volume(indices, level).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::ToggleStreamCard(app) => {
                self.stream_card = if self.stream_card.as_ref() == Some(&app) {
                    None
                } else {
                    Some(app)
                };
            }
            Message::MoveStream(indices, sink) => {
                self.stream_card = None;
                return Task::future(async move {
                    backend::audio::move_streams(indices, sink).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::HideApp(app) => {
                self.menu = None;
                if !self.hidden_apps.contains(&app) {
                    self.hidden_apps.push(app);
                    backend::audio::save_hidden("apps", &self.hidden_apps);
                }
            }
            Message::UnhideApp(app) => {
                self.hidden_apps.retain(|a| *a != app);
                backend::audio::save_hidden("apps", &self.hidden_apps);
            }
            Message::HidePlayer(app) => {
                self.menu = None;
                if !self.hidden_players.contains(&app) {
                    self.hidden_players.push(app);
                    backend::audio::save_hidden("players", &self.hidden_players);
                }
            }
            Message::UnhidePlayer(app) => {
                self.hidden_players.retain(|a| *a != app);
                backend::audio::save_hidden("players", &self.hidden_players);
            }
            Message::ShowAllWifi => self.wifi_all = true,
            Message::Media(service, control) => {
                return run(
                    "control the player",
                    backend::media::control(service, control),
                );
            }
            Message::Seek(service, position) => self.seeking = Some((service, position)),
            Message::RaisePlayer(service) => {
                let close = self.close_popup();
                return Task::batch([
                    close,
                    run("raise the player", backend::media::raise(service)),
                ]);
            }
            Message::SetShuffle(service, shuffle) => {
                if let Some(media) = self.media.iter_mut().find(|m| m.service == service) {
                    media.shuffle = Some(shuffle);
                }
                return run("set shuffle", backend::media::set_shuffle(service, shuffle));
            }
            Message::SetLoop(service, status) => {
                if let Some(media) = self.media.iter_mut().find(|m| m.service == service) {
                    media.loop_status = Some(status.clone());
                }
                return run("set repeat", backend::media::set_loop(service, status));
            }
            Message::Escape => {
                if self.menu.is_some() {
                    self.menu = None;
                } else if self.card.is_some() || self.stream_card.is_some() {
                    self.card = None;
                    self.stream_card = None;
                    self.preview = None;
                } else {
                    return self.close_popup();
                }
            }
            Message::StreamIcon(app) => {
                let found = self
                    .streams
                    .iter()
                    .find(|(s, _)| s.app == app)
                    .map(|(s, _)| (s.indices.clone(), s.muted));
                let Some((indices, muted)) = found else {
                    return Task::none();
                };
                if let Some((s, _)) = self.streams.iter_mut().find(|(s, _)| s.app == app) {
                    s.muted = !muted;
                }
                return Task::future(async move {
                    backend::audio::set_stream_mute(indices, !muted).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::SoloStream(app) => {
                self.menu = None;
                let streams: Vec<(String, Vec<u32>, bool)> = self
                    .streams
                    .iter()
                    .map(|(s, _)| (s.app.clone(), s.indices.clone(), s.muted))
                    .collect();
                let mut tasks = Vec::new();
                {
                    for (name, indices, _) in streams {
                        if indices.is_empty() {
                            continue;
                        }
                        let muted = name != app;
                        if let Some((s, _)) = self.streams.iter_mut().find(|(s, _)| s.app == name) {
                            s.muted = muted;
                        }
                        tasks.push(Task::future(async move {
                            backend::audio::set_stream_mute(indices, muted).await;
                            cosmic::action::app(Message::Done)
                        }));
                    }
                }
                return Task::batch(tasks);
            }
            Message::SeekDone(service) => {
                let Some((_, position)) = self.seeking.take().filter(|(s, _)| *s == service) else {
                    return Task::none();
                };
                let Some(media) = self.media.iter_mut().find(|m| m.service == service) else {
                    return Task::none();
                };
                media.position = position as i64;
                media.at = Instant::now();
                return run(
                    "seek",
                    backend::media::seek(service, media.track.clone(), position as i64),
                );
            }
            Message::Screenshot => {
                backend::session::screenshot();
                return self.close_popup();
            }
            Message::Lock => {
                backend::session::lock();
                return self.close_popup();
            }
            Message::LogOut => {
                backend::session::log_out();
                return self.close_popup();
            }
            Message::Suspend => {
                backend::session::suspend();
                return self.close_popup();
            }
            Message::Restart => {
                backend::session::restart();
                return self.close_popup();
            }
            Message::PowerOff => {
                backend::session::power_off();
                return self.close_popup();
            }
            Message::Settings(page) => {
                // The status button opens it: raising a window takes the
                // toplevel protocols, which only what the panel starts gets.
                let close = self.close_popup();
                let open = Task::future(async move {
                    bus::ask_settings(page).await;
                    cosmic::action::app(Message::Done)
                });
                return Task::batch([close, open]);
            }
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            backend::network::subscription().map(Message::Network),
            backend::bluetooth::subscription().map(Message::Bluetooth),
            backend::power::subscription().map(Message::Power),
            backend::graphics::subscription().map(Message::Graphics),
            backend::brightness::subscription().map(Message::Brightness),
            backend::audio::subscription().map(Message::Audio),
            backend::media::subscription().map(Message::MediaList),
            backend::privacy::subscription().map(Message::Privacy),
            backend::system::subscription().map(Message::System),
        ];
        if self.shown {
            subscriptions.push(backend::camera::subscription().map(Message::Cameras));
            // The level meters, only while they're on screen.
            subscriptions.push(
                Subscription::run_with("@DEFAULT_SOURCE@".to_owned(), backend::audio::levels)
                    .map(Message::MicLevel),
            );
            subscriptions.push(
                Subscription::run_with("@DEFAULT_MONITOR@".to_owned(), backend::audio::levels)
                    .map(Message::OutputLevel),
            );
            // A meter per app that's playing, from one of its streams.
            for (stream, _) in &self.streams {
                if stream.corked || self.hidden_apps.contains(&stream.app) {
                    continue;
                }
                if let Some(index) = stream.indices.first() {
                    subscriptions.push(
                        Subscription::run_with(
                            (*index, stream.app.clone()),
                            backend::audio::stream_levels,
                        )
                        .map(|(app, level)| Message::StreamLevel(app, level)),
                    );
                }
            }
        }
        if self.system_shown || (self.shown && self.card == Some(Card::Graphics)) {
            subscriptions.push(backend::gpu::subscription().map(Message::Gpus));
        }
        // The camera on show streams pictures only while its menu is open.
        if self.shown && self.card == Some(Card::Camera) {
            if let Some(camera) = self.preview_camera() {
                subscriptions.push(
                    Subscription::run_with(
                        (camera.node.clone(), self.preview_size),
                        backend::camera::preview,
                    )
                    .map(Message::Frame),
                );
            }
        }
        let playing = self.shown && self.media.iter().any(|m| m.playing && m.seekable());
        if !self.pending.is_empty() || self.released.is_some() || playing {
            subscriptions
                .push(iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick));
        }
        Subscription::batch(subscriptions)
    }
}

/// Seconds as "2 h 15 min" or "40 min".
pub fn duration(seconds: u64) -> String {
    let minutes = seconds / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// With `QS_DEBUG_OPEN` set to a menu's name, the popup opens on that menu a
/// moment after start: for looking at it without a pointer.
pub(crate) fn debug_open() -> app::Task<Message> {
    if std::env::var("QS_DEBUG_OPEN").as_deref() == Ok("coral") {
        return Task::future(async {
            tokio::time::sleep(Duration::from_secs(3)).await;
            cosmic::action::app(Message::DebugCoral)
        });
    }
    if std::env::var("QS_DEBUG_OPEN").as_deref() == Ok("bubble") {
        return Task::future(async {
            tokio::time::sleep(Duration::from_secs(3)).await;
            cosmic::action::app(Message::DebugBubble)
        });
    }
    if std::env::var("QS_DEBUG_OPEN").as_deref() == Ok("settings") {
        return Task::future(async {
            tokio::time::sleep(Duration::from_secs(3)).await;
            cosmic::action::app(Message::Settings(None))
        });
    }
    let card = match std::env::var("QS_DEBUG_OPEN").as_deref() {
        Ok("camera") => Card::Camera,
        Ok("graphics") => Card::Graphics,
        Ok("privacy") => Card::Privacy,
        Ok("battery") => Card::Battery,
        Ok("wired") => Card::Wired,
        Ok("display") => Card::Display,
        _ => return Task::none(),
    };
    Task::future(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        cosmic::action::app(Message::OpenCard(card))
    })
}
