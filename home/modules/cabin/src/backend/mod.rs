// SPDX-License-Identifier: GPL-3.0-only

//! What the applet reads and sets on the system, one module per service.
//! Each one gives a snapshot type, a way to read it, and a stream of
//! snapshots as it changes.

pub mod apps;
pub mod audio;
pub mod bluetooth;
pub mod brightness;
pub mod camera;
pub mod display;
pub mod dnd;
pub mod dns;
pub mod ec;
pub mod gpu;
pub mod graphics;
pub mod ics;
pub mod idle;
pub mod latest;
pub mod live;
pub mod location;
pub mod media;
pub mod network;
pub mod nightlight;
pub mod nixos;
pub mod notifications;
pub mod power;
pub mod privacy;
pub mod session;
pub mod system;
pub mod toplevels;
pub mod weather;
pub mod windows;

use std::{pin::Pin, time::Duration};

use cosmic::iced::stream;
use futures::{FutureExt, SinkExt, Stream, StreamExt};

pub type Changes = Pin<Box<dyn Stream<Item = ()> + Send>>;

/// A stream of snapshots: one now, then one each time `changes` yields or
/// `every` passes, each read with `read`. Equal snapshots in a row are sent
/// once. Wrapped in `Subscription::run` by each service.
pub fn watch<T, R, F>(every: Duration, changes: fn() -> Changes, read: R) -> impl Stream<Item = T>
where
    T: Clone + PartialEq + Send + 'static,
    R: Fn() -> F + Send + 'static,
    F: Future<Output = Option<T>> + Send,
{
    stream::channel(
        4,
        move |mut out: futures::channel::mpsc::Sender<T>| async move {
            let mut last: Option<T> = None;
            let mut changes = changes();
            loop {
                if let Some(value) = read().await {
                    if last.as_ref() != Some(&value) {
                        last = Some(value.clone());
                        if out.send(value).await.is_err() {
                            return;
                        }
                    }
                }
                tokio::select! {
                    change = changes.next() => {
                        if change.is_none() {
                            // The source is gone: look again now and then.
                            changes = no_changes();
                        }
                        // Let a burst of changes settle.
                        tokio::time::sleep(Duration::from_millis(150)).await;
                    }
                    _ = tokio::time::sleep(every) => {}
                }
            }
        },
    )
}

/// Signals on the system or the session bus matching the rules, as a stream
/// of nothing in particular: just that something changed.
pub fn bus_changes(system: bool, rules: fn() -> Vec<zbus::MatchRule<'static>>) -> Changes {
    async move {
        let conn = if system {
            zbus::Connection::system().await
        } else {
            zbus::Connection::session().await
        };
        let Ok(conn) = conn else {
            return no_changes();
        };
        let mut streams = Vec::new();
        for rule in rules() {
            match zbus::MessageStream::for_match_rule(rule, &conn, Some(64)).await {
                Ok(stream) => streams.push(stream.map(|_| ())),
                Err(err) => tracing::warn!("cannot listen on the bus: {err}"),
            }
        }
        Box::pin(futures::stream::select_all(streams)) as Changes
    }
    .into_stream()
    .flatten()
    .boxed()
}

pub fn no_changes() -> Changes {
    Box::pin(futures::stream::pending())
}

/// A rule for one kind of signal, from a sender or at a path.
pub fn signal(
    sender: Option<&'static str>,
    path: Option<&'static str>,
    interface: Option<&'static str>,
    member: Option<&'static str>,
) -> Option<zbus::MatchRule<'static>> {
    let mut rule = zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal);
    if let Some(sender) = sender {
        rule = rule.sender(sender).ok()?;
    }
    if let Some(path) = path {
        rule = rule.path(path).ok()?;
    }
    if let Some(interface) = interface {
        rule = rule.interface(interface).ok()?;
    }
    if let Some(member) = member {
        rule = rule.member(member).ok()?;
    }
    Some(rule.build())
}

/// Started on the host and left be.
pub fn spawn(program: &str, args: &[&str], env: &[(&str, &str)]) {
    let mut command = std::process::Command::new(program);
    // Our own library path (the dev loop's, or the wrapper's) is ours alone:
    // another COSMIC program with it crashes in wgpu.
    command
        .env_remove("LD_LIBRARY_PATH")
        .envs(env.iter().copied())
        .args(args);
    match command.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(err) => tracing::error!("cannot run {program}: {err}"),
    }
}
