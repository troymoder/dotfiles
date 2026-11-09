// SPDX-License-Identifier: GPL-3.0-only

//! Notifications, as cosmic-notifications hands them to panel applets.
//!
//! The panel gives applets marked `X-NotificationsApplet` a socket to the
//! daemon in `COSMIC_NOTIFICATIONS`; new notifications come through it. They
//! are closed through the usual `org.freedesktop.Notifications`, and their
//! actions invoked back through the socket. The list is kept across runs.

use std::{
    collections::{HashMap, HashSet},
    os::unix::io::{FromRawFd, RawFd},
    time::SystemTime,
};

use cosmic::iced::{Subscription, stream};
use futures::{SinkExt, StreamExt};
use tokio::sync::OnceCell;
use zbus::{connection::Builder, zvariant::Value};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Notification {
    pub id: u32,
    pub app: String,
    /// An icon name, or a path.
    pub icon: String,
    /// A picture that came with it: a sender's face, a screenshot.
    #[serde(default)]
    pub image: Option<String>,
    pub summary: String,
    pub body: String,
    /// Pairs of action key and label.
    pub actions: Vec<(String, String)>,
    pub time: SystemTime,
    /// From before the applet started: the daemon has forgotten it, and
    /// its id may be another's now.
    #[serde(default)]
    pub restored: bool,
}

fn history_file() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))?;
    Some(dir.join(crate::APP_ID).join("notifications.json"))
}

/// The notifications kept from the last run.
pub fn load() -> Vec<Notification> {
    let Some(file) = history_file() else {
        return Vec::new();
    };
    let Ok(data) = std::fs::read(file) else {
        return Vec::new();
    };
    let mut notifications: Vec<Notification> = serde_json::from_slice(&data).unwrap_or_default();
    for notification in &mut notifications {
        notification.restored = true;
        notification.actions.clear();
    }
    notifications
}

pub fn save(notifications: &[Notification]) {
    let Some(file) = history_file() else {
        return;
    };
    let result = file.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| {
        let data = serde_json::to_vec(notifications).map_err(std::io::Error::other)?;
        std::fs::write(&file, data)
    });
    if let Err(err) = result {
        tracing::warn!("cannot keep the notifications: {err}");
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    New(Notification),
    /// Gone: opened, or closed by its app or from here.
    Closed(u32),
    /// Its popup closed by hand: it stays in the list, but the daemon has
    /// let go of it.
    Dismissed,
}

/// `NotificationClosed`'s reasons when its popup timed out, and when the
/// user closed the popup: either way it stays in the list.
const EXPIRED: u32 = 1;
const DISMISSED_BY_USER: u32 = 2;

#[zbus::proxy(
    default_service = "com.system76.NotificationsApplet",
    interface = "com.system76.NotificationsApplet",
    default_path = "/com/system76/NotificationsApplet"
)]
trait NotificationsApplet {
    #[zbus(signal)]
    fn notify(
        &self,
        app_name: &str,
        id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<&str>,
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<()>;

    fn invoke_action(&self, id: u32, action: String) -> zbus::Result<()>;
}

#[zbus::proxy(
    default_service = "org.freedesktop.Notifications",
    interface = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    fn close_notification(&self, id: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    fn notification_closed(&self, id: u32, reason: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    fn action_invoked(&self, id: u32, action_key: &str) -> zbus::Result<()>;
}

enum Signal {
    New(Notification),
    Invoked(u32),
    Closed(u32, u32),
}

/// The connection to the daemon, if the panel gave us one.
async fn daemon() -> Option<NotificationsAppletProxy<'static>> {
    static DAEMON: OnceCell<Option<NotificationsAppletProxy<'static>>> = OnceCell::const_new();
    DAEMON
        .get_or_init(|| async {
            let fd: RawFd = std::env::var("COSMIC_NOTIFICATIONS").ok()?.parse().ok()?;
            // SAFETY: the panel opened this descriptor for us and nothing else holds it.
            let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
            stream.set_nonblocking(true).ok()?;
            let stream = tokio::net::UnixStream::from_std(stream).ok()?;
            let conn = Builder::socket(stream).p2p().build().await.ok()?;
            NotificationsAppletProxy::new(&conn).await.ok()
        })
        .await
        .clone()
}

/// Markup stripped, entities unescaped, whitespace settled.
fn plain(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut in_tag = false;
    for c in body.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&").trim().to_owned()
}

/// The icon to show: the app's own by its desktop entry where given, else
/// what the notification brought.
fn icon_for(desktop_entry: Option<&str>, app_icon: &str, image: Option<&str>) -> String {
    if let Some(entry) = desktop_entry.filter(|e| !e.is_empty()) {
        return entry.trim_end_matches(".desktop").to_owned();
    }
    [Some(app_icon), image]
        .into_iter()
        .flatten()
        .map(|icon| icon.strip_prefix("file://").unwrap_or(icon))
        .find(|icon| !icon.is_empty() && (!icon.starts_with('/') || std::path::Path::new(icon).exists()))
        .unwrap_or("preferences-system-notifications-symbolic")
        .to_owned()
}

fn event(opened: &mut HashSet<u32>, signal: Signal) -> Option<Event> {
    Some(match signal {
        Signal::New(notification) => Event::New(notification),
        Signal::Invoked(id) => {
            opened.insert(id);
            return None;
        }
        Signal::Closed(id, EXPIRED | DISMISSED_BY_USER) if !opened.contains(&id) => Event::Dismissed,
        Signal::Closed(id, _) => {
            opened.remove(&id);
            Event::Closed(id)
        }
    })
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run(|| {
        stream::channel(50, |mut output: futures::channel::mpsc::Sender<Event>| async move {
            let Some(daemon) = daemon().await else {
                tracing::info!("no notifications socket from the panel");
                return futures::future::pending().await;
            };
            let Ok(session) = zbus::Connection::session().await else {
                return futures::future::pending().await;
            };
            let Ok(freedesktop) = NotificationsProxy::new(&session).await else {
                return futures::future::pending().await;
            };
            let (Ok(notify), Ok(closed), Ok(invoked)) =
                (daemon.receive_notify().await, freedesktop.receive_notification_closed().await, freedesktop.receive_action_invoked().await)
            else {
                return futures::future::pending().await;
            };
            let notify = notify.filter_map(|signal| async move {
                let args = signal.args().ok()?;
                let actions = args
                    .actions
                    .chunks(2)
                    .filter_map(|pair| match pair {
                        [key, label] => Some((key.to_string(), label.to_string())),
                        _ => None,
                    })
                    .collect();
                let hint = |key: &str| match args.hints.get(key) {
                    Some(Value::Str(value)) => Some(value.to_string()),
                    _ => None,
                };
                Some(Signal::New(Notification {
                    id: args.id,
                    app: args.app_name.to_owned(),
                    icon: icon_for(hint("desktop-entry").as_deref(), args.app_icon, None),
                    image: hint("image-path").map(|p| p.strip_prefix("file://").unwrap_or(&p).to_owned()).filter(|p| std::path::Path::new(p).exists()),
                    summary: args.summary.to_owned(),
                    body: plain(args.body),
                    actions,
                    time: SystemTime::now(),
                    restored: false,
                }))
            });
            let closed = closed.filter_map(|signal| async move {
                let args = signal.args().ok()?;
                Some(Signal::Closed(args.id, args.reason))
            });
            let invoked = invoked.filter_map(|signal| async move { Some(Signal::Invoked(signal.args().ok()?.id)) });
            let mut signals = futures::stream::select_all([notify.boxed(), closed.boxed(), invoked.boxed()]);
            let mut opened = HashSet::new();
            while let Some(signal) = signals.next().await {
                let Some(event) = event(&mut opened, signal) else {
                    continue;
                };
                if output.send(event).await.is_err() {
                    return;
                }
            }
            futures::future::pending().await
        })
    })
}

/// An action of a notification taken, through the daemon.
pub async fn invoke(id: u32, action: String) {
    if let Some(daemon) = daemon().await {
        if let Err(err) = daemon.invoke_action(id, action).await {
            tracing::warn!("cannot invoke the action: {err}");
        }
    }
}

/// Notifications closed with the daemon, so it forgets them too.
pub async fn dismiss(ids: Vec<u32>) {
    let Ok(conn) = zbus::Connection::session().await else {
        return;
    };
    let Ok(proxy) = NotificationsProxy::new(&conn).await else {
        return;
    };
    for id in ids {
        _ = proxy.close_notification(id).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text() {
        assert_eq!(plain("<b>Hi</b> &amp; bye "), "Hi & bye");
        assert_eq!(plain("a <a href=\"x\">link</a>"), "a link");
    }
}
