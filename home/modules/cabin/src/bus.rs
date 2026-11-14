// SPDX-License-Identifier: GPL-3.0-only

//! The daemon and the panel buttons talk over the session bus.
//!
//! `cabind` owns `dev.troy.Cabin` and every bit of state; the buttons on
//! the panel ask it to open their panel, and draw what it tells them (the
//! status icons, the clock's dot, the System bars) from small JSON models
//! it signals whenever they change. `cabinctl` makes the same calls.
//!
//! Two jobs go the other way, to the buttons, because only something the
//! panel started can do them: the clock button holds the notification
//! daemon's socket, so it forwards notifications here and invokes their
//! actions when asked; the status button can raise windows (the toplevel
//! protocols are for privileged clients), so it opens Settings when asked.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use cosmic::iced::{Subscription, stream};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

pub const NAME: &str = "dev.troy.Cabin";
pub const PATH: &str = "/dev/troy/Cabin";

/// What a panel button opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Owner {
    /// The controls and the audio panels, side by side.
    Controls,
    /// The notifications, calendar, weather and clocks.
    Date,
    System,
}

impl Owner {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "controls" | "quick-settings" | "status" => Some(Self::Controls),
            "date" | "clock" | "calendar" | "notifications" => Some(Self::Date),
            "system" => Some(Self::System),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Controls => "controls",
            Self::Date => "date",
            Self::System => "system",
        }
    }
}

/// What a call asks of the daemon.
#[derive(Clone, Debug)]
pub enum Request {
    /// Open the owner's panels, or close them if they are what's open.
    Toggle(Owner),
    /// Open the owner's panels, with a menu in them open (a card's name).
    Open(Owner, Option<String>),
    /// Close what's open; pinned panels stay.
    Close,
    /// The volume up (or down, negative) by this many steps of 5 %.
    Volume(f64),
    /// The window switcher up, or its pick moved on (back, negative);
    /// with pictures of the windows if it's coming up.
    Switch(i32, bool),
    /// The workspace overview up, or down if it's up.
    Overview,
    /// The launcher up, or down if it's up; the app library (every app
    /// listed) if asked.
    Launcher(bool),
    /// A notification, as the clock button had it from the panel.
    Notified(Box<crate::backend::notifications::Notification>),
    NotificationClosed(u32),
}

// ---- what the panel buttons draw

/// The status icons: what's worth a glance, each with its tooltip.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StatusBar {
    /// The controls are open: the button shows pressed.
    pub open: bool,
    /// Icon name and tooltip, left to right.
    pub icons: Vec<(String, String)>,
    /// The Bluetooth output's battery, when sound goes to one that tells.
    pub headphones: Option<u8>,
    /// Icon, percent and tooltip.
    pub battery: Option<(String, f32, String)>,
    /// The battery running out: its icon red.
    #[serde(default)]
    pub battery_low: bool,
    pub privacy: Option<PrivacyPill>,
}

/// The pill while something watches or listens.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PrivacyPill {
    pub screen: bool,
    pub camera: bool,
    pub mic: bool,
    pub mic_muted: bool,
    pub tip: String,
}

/// The clock's marks: a dot for notifications, the bell crossed out for Do
/// not disturb. The time it keeps itself.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClockBar {
    pub open: bool,
    pub dnd: bool,
    pub unread: bool,
}

/// The System button's two bars, 0 to 1, and the CPU in words.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemBar {
    pub open: bool,
    pub cpu: f32,
    pub memory: f32,
    pub label: String,
    pub tip: String,
}

// ---- the daemon's side

/// The bars as last signalled, for buttons that start later and ask.
static BARS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
static CONNECTION: OnceLock<zbus::Connection> = OnceLock::new();

struct Server {
    requests: futures::channel::mpsc::UnboundedSender<Request>,
}

impl Server {
    fn send(&self, request: Request) -> zbus::fdo::Result<()> {
        self.requests
            .unbounded_send(request)
            .map_err(|_| zbus::fdo::Error::Failed("the daemon is stopping".to_owned()))
    }
}

fn owner(name: &str) -> zbus::fdo::Result<Owner> {
    Owner::parse(name).ok_or_else(|| {
        zbus::fdo::Error::InvalidArgs(format!("no such panel: {name} (controls, date, system)"))
    })
}

#[zbus::interface(name = "dev.troy.Cabin")]
impl Server {
    fn toggle(&self, panel: &str) -> zbus::fdo::Result<()> {
        self.send(Request::Toggle(owner(panel)?))
    }

    fn open(&self, panel: &str, menu: &str) -> zbus::fdo::Result<()> {
        self.send(Request::Open(
            owner(panel)?,
            (!menu.is_empty()).then(|| menu.to_owned()),
        ))
    }

    fn close(&self) -> zbus::fdo::Result<()> {
        self.send(Request::Close)
    }

    fn volume(&self, steps: f64) -> zbus::fdo::Result<()> {
        self.send(Request::Volume(steps))
    }

    fn switch(&self, step: i32, previews: bool) -> zbus::fdo::Result<()> {
        self.send(Request::Switch(step, previews))
    }

    fn overview(&self) -> zbus::fdo::Result<()> {
        self.send(Request::Overview)
    }

    fn launcher(&self, apps: bool) -> zbus::fdo::Result<()> {
        self.send(Request::Launcher(apps))
    }

    /// The bar a button draws (`status`, `clock`, `system`), as JSON.
    fn bar(&self, kind: &str) -> String {
        BARS.lock()
            .ok()
            .and_then(|bars| bars.as_ref()?.get(kind).cloned())
            .unwrap_or_default()
    }

    fn notified(&self, notification: &str) -> zbus::fdo::Result<()> {
        let notification = serde_json::from_str(notification)
            .map_err(|err| zbus::fdo::Error::InvalidArgs(err.to_string()))?;
        self.send(Request::Notified(Box::new(notification)))
    }

    fn notification_closed(&self, id: u32) -> zbus::fdo::Result<()> {
        self.send(Request::NotificationClosed(id))
    }

    #[zbus(signal)]
    async fn bar_changed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        kind: &str,
        bar: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn invoke_action(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        id: u32,
        key: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn open_settings(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        page: &str,
    ) -> zbus::Result<()>;
}

/// The daemon's end: takes the name and passes on what's asked. Another
/// daemon holding the name already is fatal: two would draw every panel.
pub fn serve() -> Subscription<Request> {
    Subscription::run(|| {
        stream::channel(
            32,
            |mut output: futures::channel::mpsc::Sender<Request>| async move {
                let (requests, mut incoming) = futures::channel::mpsc::unbounded();
                let built = async {
                    zbus::connection::Builder::session()?
                        .serve_at(PATH, Server { requests })?
                        .name(zbus::names::WellKnownName::try_from(NAME)?)?
                        .build()
                        .await
                };
                match built.await {
                    Ok(connection) => {
                        _ = CONNECTION.set(connection);
                    }
                    Err(err) => {
                        tracing::error!(
                            "cannot take {NAME} on the session bus (another cabind running?): {err}"
                        );
                        std::process::exit(1);
                    }
                }
                while let Some(request) = incoming.next().await {
                    if output.send(request).await.is_err() {
                        return;
                    }
                }
                futures::future::pending().await
            },
        )
    })
}

async fn emit(member: &'static str, body: &(impl Serialize + zbus::zvariant::DynamicType + Sync)) {
    let Some(connection) = CONNECTION.get() else {
        return;
    };
    if let Err(err) = connection
        .emit_signal(None::<()>, PATH, NAME, member, body)
        .await
    {
        tracing::warn!("cannot signal {member}: {err}");
    }
}

/// A bar told to the buttons, and kept for those that ask.
pub async fn publish_bar(kind: &'static str, bar: String) {
    if let Ok(mut bars) = BARS.lock() {
        bars.get_or_insert_with(HashMap::new)
            .insert(kind.to_owned(), bar.clone());
    }
    emit("BarChanged", &(kind, bar.as_str())).await;
}

/// The clock button asked to take a notification's action.
pub async fn ask_invoke(id: u32, key: String) {
    emit("InvokeAction", &(id, key.as_str())).await;
}

/// The status button asked to open Settings, at a page or where it was.
pub async fn ask_settings(page: Option<&'static str>) {
    emit("OpenSettings", &(page.unwrap_or_default(),)).await;
}

// ---- the buttons' side, and cabinctl's

#[zbus::proxy(
    interface = "dev.troy.Cabin",
    default_service = "dev.troy.Cabin",
    default_path = "/dev/troy/Cabin"
)]
pub trait Cabin {
    fn toggle(&self, panel: &str) -> zbus::Result<()>;
    fn open(&self, panel: &str, menu: &str) -> zbus::Result<()>;
    fn close(&self) -> zbus::Result<()>;
    fn volume(&self, steps: f64) -> zbus::Result<()>;
    fn switch(&self, step: i32, previews: bool) -> zbus::Result<()>;
    fn overview(&self) -> zbus::Result<()>;
    fn launcher(&self, apps: bool) -> zbus::Result<()>;
    fn bar(&self, kind: &str) -> zbus::Result<String>;
    fn notified(&self, notification: &str) -> zbus::Result<()>;
    fn notification_closed(&self, id: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    fn bar_changed(&self, kind: String, bar: String) -> zbus::Result<()>;
    #[zbus(signal)]
    fn invoke_action(&self, id: u32, key: String) -> zbus::Result<()>;
    #[zbus(signal)]
    fn open_settings(&self, page: String) -> zbus::Result<()>;
}

/// The daemon, through one session connection for the process.
pub async fn daemon() -> Option<CabinProxy<'static>> {
    static PROXY: OnceCell<Option<CabinProxy<'static>>> = OnceCell::const_new();
    PROXY
        .get_or_init(|| async {
            let connection = zbus::Connection::session()
                .await
                .inspect_err(|err| tracing::warn!("no session bus: {err}"))
                .ok()?;
            CabinProxy::new(&connection)
                .await
                .inspect_err(|err| tracing::warn!("cannot reach {NAME}: {err}"))
                .ok()
        })
        .await
        .clone()
}

/// A call to the daemon for its effect; a failure is logged.
pub async fn call<F, Fut>(what: &str, f: F)
where
    F: FnOnce(CabinProxy<'static>) -> Fut,
    Fut: Future<Output = zbus::Result<()>>,
{
    let Some(daemon) = daemon().await else {
        return;
    };
    if let Err(err) = f(daemon).await {
        tracing::warn!("cabind: cannot {what}: {err}");
    }
}

/// A button's bar, as JSON: now, whenever it changes, and again when the
/// daemon (re)starts. Until the daemon answers, it's asked every few seconds.
pub fn bar(kind: &'static str) -> Subscription<String> {
    Subscription::run_with(kind, |kind| {
        let kind = *kind;
        stream::channel(
            8,
            move |mut output: futures::channel::mpsc::Sender<String>| async move {
                let daemon = loop {
                    match daemon().await {
                        Some(daemon) => break daemon,
                        None => tokio::time::sleep(std::time::Duration::from_secs(5)).await,
                    }
                };
                let (Ok(changes), Ok(owners)) = (
                    daemon.receive_bar_changed().await,
                    daemon.inner().receive_owner_changed().await,
                ) else {
                    return futures::future::pending().await;
                };
                enum Event {
                    Bar(String),
                    Owner(bool),
                    Ask,
                }
                let changes = changes.filter_map(move |signal| async move {
                    let args = signal.args().ok()?;
                    (args.kind == kind).then(|| Event::Bar(args.bar.clone()))
                });
                let owners = owners.map(|owner| Event::Owner(owner.is_some()));
                let asks = futures::stream::once(async { Event::Ask });
                let mut events =
                    futures::stream::select_all([changes.boxed(), owners.boxed(), asks.boxed()]);
                while let Some(event) = events.next().await {
                    let bar = match event {
                        Event::Bar(bar) => Some(bar),
                        Event::Owner(false) => None,
                        Event::Owner(true) | Event::Ask => {
                            // Asked until there's an answer: the daemon may be
                            // starting, or not yet started.
                            let mut answer = None;
                            for _ in 0..30 {
                                match daemon.bar(kind).await {
                                    Ok(bar) if !bar.is_empty() => {
                                        answer = Some(bar);
                                        break;
                                    }
                                    Ok(_) => {}
                                    Err(err) => tracing::debug!("cabind: no bar yet: {err}"),
                                }
                                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            }
                            answer
                        }
                    };
                    if let Some(bar) = bar {
                        if output.send(bar).await.is_err() {
                            return;
                        }
                    }
                }
                futures::future::pending().await
            },
        )
    })
}

/// The daemon asking the clock button to take a notification's action.
pub fn invoke_requests() -> Subscription<(u32, String)> {
    Subscription::run(|| {
        stream::channel(
            8,
            |mut output: futures::channel::mpsc::Sender<(u32, String)>| async move {
                let Some(daemon) = daemon().await else {
                    return futures::future::pending().await;
                };
                let Ok(mut signals) = daemon.receive_invoke_action().await else {
                    return futures::future::pending().await;
                };
                while let Some(signal) = signals.next().await {
                    if let Ok(args) = signal.args() {
                        if output.send((args.id, args.key.clone())).await.is_err() {
                            return;
                        }
                    }
                }
                futures::future::pending().await
            },
        )
    })
}

/// The daemon asking the status button to open Settings: the page, or "".
pub fn settings_requests() -> Subscription<String> {
    Subscription::run(|| {
        stream::channel(
            8,
            |mut output: futures::channel::mpsc::Sender<String>| async move {
                let Some(daemon) = daemon().await else {
                    return futures::future::pending().await;
                };
                let Ok(mut signals) = daemon.receive_open_settings().await else {
                    return futures::future::pending().await;
                };
                while let Some(signal) = signals.next().await {
                    if let Ok(args) = signal.args() {
                        if output.send(args.page.clone()).await.is_err() {
                            return;
                        }
                    }
                }
                futures::future::pending().await
            },
        )
    })
}
