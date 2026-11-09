// SPDX-License-Identifier: GPL-3.0-only

//! Sound in and out, through WirePlumber's `wpctl` for the levels and
//! PulseAudio's `pactl` for the devices and for word of changes.

use std::{process::Stdio, time::Duration};

use cosmic::iced::Subscription;
use futures::StreamExt;
use tokio::io::AsyncBufReadExt;

use super::{Changes, latest::Latest};

/// How far a level goes: PipeWire boosts past 100 %, up to 150 %.
pub const MAX: f32 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Volume {
    /// 0 to `MAX`.
    pub level: f32,
    pub muted: bool,
}

impl Volume {
    pub fn output_icon(&self) -> &'static str {
        match self.level {
            _ if self.muted => "audio-volume-muted-symbolic",
            l if l <= 0.0 => "audio-volume-muted-symbolic",
            l if l < 0.34 => "audio-volume-low-symbolic",
            l if l < 0.67 => "audio-volume-medium-symbolic",
            _ => "audio-volume-high-symbolic",
        }
    }

    pub fn input_icon(&self) -> &'static str {
        if self.muted || self.level <= 0.0 { "microphone-sensitivity-muted-symbolic" } else { "audio-input-microphone-symbolic" }
    }
}

/// An output or an input, by its PulseAudio name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub description: String,
    /// PulseAudio's number for it, which streams refer to.
    pub index: u64,
}

/// An app's sound: its streams together, its name and icon.
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    pub indices: Vec<u32>,
    pub app: String,
    pub icon: Option<String>,
    /// 0 to `MAX`.
    pub level: f32,
    pub muted: bool,
    /// Paused: the stream is there but makes no sound.
    pub corked: bool,
    /// The output it plays through, by name.
    pub sink: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Audio {
    pub streams: Vec<Stream>,
    pub output: Option<Volume>,
    pub input: Option<Volume>,
    pub sinks: Vec<Device>,
    pub default_sink: String,
    pub sources: Vec<Device>,
    pub default_source: String,
}

const SINK: &str = "@DEFAULT_AUDIO_SINK@";
const SOURCE: &str = "@DEFAULT_AUDIO_SOURCE@";

fn pactl() -> tokio::process::Command {
    let mut command = tokio::process::Command::new("pactl");
    // Its JSON breaks on translated messages.
    command.env("LC_ALL", "C");
    command
}

fn parse_volume(output: &str) -> Option<Volume> {
    let rest = output.trim().strip_prefix("Volume:")?.trim();
    let level = rest.split_whitespace().next()?.parse().ok()?;
    Some(Volume { level, muted: rest.contains("[MUTED]") })
}

async fn volume(node: &str) -> Option<Volume> {
    let output = tokio::process::Command::new("wpctl").args(["get-volume", node]).output().await.ok()?;
    parse_volume(&String::from_utf8_lossy(&output.stdout))
}

async fn default(what: &str) -> String {
    match pactl().arg(what).output().await {
        Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        Err(_) => String::new(),
    }
}

/// Outputs, by the port in use where there is one ("Speaker", "HDMI 1"),
/// leaving out ports with nothing plugged in.
async fn sinks(default: &str) -> Vec<Device> {
    let Ok(list) = pactl().args(["-f", "json", "list", "sinks"]).output().await else {
        return Vec::new();
    };
    let list: Vec<serde_json::Value> = serde_json::from_slice(&list.stdout).unwrap_or_default();
    list.iter()
        .filter_map(|sink| {
            let name = sink["name"].as_str()?;
            let active = sink["active_port"].as_str();
            let port = sink["ports"].as_array().into_iter().flatten().find(|port| port["name"].as_str() == active);
            let unplugged = port.is_some_and(|p| p["availability"] == "not available");
            if unplugged && name != default {
                return None;
            }
            let description = port
                .filter(|_| !name.starts_with("bluez"))
                .and_then(|p| p["description"].as_str())
                .or(sink["description"].as_str())?;
            Some(Device { name: name.to_owned(), description: description.to_owned(), index: sink["index"].as_u64().unwrap_or(0) })
        })
        .collect()
}

/// Inputs, leaving out the monitors of outputs.
async fn sources() -> Vec<Device> {
    let Ok(list) = pactl().args(["-f", "json", "list", "sources"]).output().await else {
        return Vec::new();
    };
    let list: Vec<serde_json::Value> = serde_json::from_slice(&list.stdout).unwrap_or_default();
    list.iter()
        .filter_map(|source| {
            let name = source["name"].as_str()?;
            if name.ends_with(".monitor") {
                return None;
            }
            Some(Device { name: name.to_owned(), description: source["description"].as_str()?.to_owned(), index: source["index"].as_u64().unwrap_or(0) })
        })
        .collect()
}

/// The channels' mean, 1.0 being 100 %.
fn level(volume: &serde_json::Value) -> f32 {
    let levels: Vec<f32> = volume
        .as_object()
        .into_iter()
        .flat_map(|channels| channels.values())
        .filter_map(|channel| channel["value"].as_f64())
        .map(|value| value as f32 / 65536.0)
        .collect();
    if levels.is_empty() { 0.0 } else { levels.iter().sum::<f32>() / levels.len() as f32 }
}

fn property<'a>(object: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    object["properties"][key].as_str().filter(|v| !v.is_empty())
}

/// Apps playing sound, one row each however many streams they have; each
/// with the output it's on, by the sinks' numbers.
async fn streams(sinks: &[Device]) -> Vec<Stream> {
    let Ok(list) = pactl().args(["-f", "json", "list", "sink-inputs"]).output().await else {
        return Vec::new();
    };
    let list: Vec<serde_json::Value> = serde_json::from_slice(&list.stdout).unwrap_or_default();
    let mut streams: Vec<Stream> = Vec::new();
    for input in &list {
        let Some(index) = input["index"].as_u64() else {
            continue;
        };
        let corked = input["corked"].as_bool().unwrap_or(false);
        let Some(app) = property(input, "application.name").or(property(input, "node.name")) else {
            continue;
        };
        let muted = input["mute"].as_bool().unwrap_or(false);
        let level = level(&input["volume"]);
        let sink = input["sink"].as_u64().and_then(|i| sinks.iter().find(|s| s.index == i)).map(|s| s.name.clone());
        if let Some(stream) = streams.iter_mut().find(|s| s.app == app) {
            stream.indices.push(index as u32);
            stream.level = stream.level.max(level);
            stream.muted &= muted;
            stream.corked &= corked;
            continue;
        }
        let icon = property(input, "application.icon_name")
            .map(str::to_owned)
            .or_else(|| super::apps::icon(app, property(input, "application.process.binary")));
        streams.push(Stream {
            indices: vec![index as u32],
            app: app.to_owned(),
            icon,
            level,
            muted,
            corked,
            sink,
        });
    }
    streams
}

/// An app's streams moved to another output.
pub async fn move_streams(indices: Vec<u32>, sink: String) {
    for index in indices {
        _ = pactl().args(["move-sink-input", &index.to_string(), &sink]).status().await;
    }
}

fn percent(level: f32) -> String {
    format!("{:.0}%", level.clamp(0.0, MAX) * 100.0)
}

pub async fn set_stream_volume(indices: Vec<u32>, level: f32) {
    // A slider sends far faster than pactl runs: only the latest level goes.
    static LATEST: Latest<(Vec<u32>, f32)> = Latest::new();
    LATEST
        .send((indices, level), |(indices, level)| async move {
            for index in indices {
                _ = pactl().args(["set-sink-input-volume", &index.to_string(), &percent(level)]).status().await;
            }
        })
        .await;
}

pub async fn set_stream_mute(indices: Vec<u32>, muted: bool) {
    for index in indices {
        _ = pactl().args(["set-sink-input-mute", &index.to_string(), if muted { "1" } else { "0" }]).status().await;
    }
}

pub async fn now() -> Option<Audio> {
    let default_sink = default("get-default-sink").await;
    let sinks = sinks(&default_sink).await;
    Some(Audio {
        streams: streams(&sinks).await,
        output: volume(SINK).await,
        input: volume(SOURCE).await,
        sinks,
        default_sink,
        sources: sources().await,
        default_source: default("get-default-source").await,
    })
}

/// `pactl subscribe`'s lines about sinks, sources and the server, as
/// changes. The process goes with the applet: left behind, each would hold
/// one of the few connections PipeWire allows.
fn changes() -> Changes {
    async_stream()
}

fn async_stream() -> Changes {
    use futures::stream;
    Box::pin(
        stream::unfold(None::<tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStdout>>>, |mut lines| async move {
            loop {
                if lines.is_none() {
                    let mut command = pactl();
                    command.arg("subscribe").stdout(Stdio::piped()).kill_on_drop(true);
                    // SAFETY: prctl is async-signal-safe.
                    unsafe {
                        command.pre_exec(|| {
                            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                            Ok(())
                        });
                    }
                    match command.spawn().ok().and_then(|mut child| child.stdout.take().map(|out| (child, out))) {
                        Some((child, stdout)) => {
                            // Kept alive by the reader's lifetime below.
                            std::mem::forget(child);
                            lines = Some(tokio::io::BufReader::new(stdout).lines());
                        }
                        None => {
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                    }
                }
                match lines.as_mut().unwrap().next_line().await {
                    Ok(Some(line)) => {
                        if line.contains("sink") || line.contains("source") || line.contains("server") || line.contains("sink-input") {
                            return Some(((), lines));
                        }
                    }
                    // pactl went away, say PipeWire restarted.
                    _ => {
                        lines = None;
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        return Some(((), lines));
                    }
                }
            }
        })
        .boxed(),
    )
}

pub fn subscription() -> Subscription<Audio> {
    Subscription::run(|| super::watch(Duration::from_secs(60), changes, now))
}

async fn wpctl(args: &[&str]) {
    _ = tokio::process::Command::new("wpctl").args(args).status().await;
}

pub async fn set_output(level: f32) {
    static LATEST: Latest<f32> = Latest::new();
    LATEST.send(level, |level| async move { wpctl(&["set-volume", SINK, &format!("{:.2}", level.clamp(0.0, MAX))]).await }).await;
}

pub async fn set_input(level: f32) {
    static LATEST: Latest<f32> = Latest::new();
    LATEST.send(level, |level| async move { wpctl(&["set-volume", SOURCE, &format!("{:.2}", level.clamp(0.0, MAX))]).await }).await;
}

pub async fn set_output_mute(muted: bool) {
    wpctl(&["set-mute", SINK, if muted { "1" } else { "0" }]).await;
}

pub async fn set_input_mute(muted: bool) {
    wpctl(&["set-mute", SOURCE, if muted { "1" } else { "0" }]).await;
}

pub async fn toggle_output_mute() {
    wpctl(&["set-mute", SINK, "toggle"]).await;
}

pub async fn toggle_input_mute() {
    wpctl(&["set-mute", SOURCE, "toggle"]).await;
}

pub async fn set_sink(name: String) {
    _ = pactl().args(["set-default-sink", &name]).status().await;
}

pub async fn set_source(name: String) {
    _ = pactl().args(["set-default-source", &name]).status().await;
}

fn hidden_file(what: &str) -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))?;
    Some(dir.join(crate::APP_ID).join(format!("hidden_{what}.json")))
}

/// Names kept out of sight: `apps` out of the mixer, `players` out of the
/// media cards.
pub fn load_hidden(what: &str) -> Vec<String> {
    hidden_file(what).and_then(|f| std::fs::read(f).ok()).and_then(|d| serde_json::from_slice(&d).ok()).unwrap_or_default()
}

pub fn save_hidden(what: &str, hidden: &[String]) {
    let Some(file) = hidden_file(what) else {
        return;
    };
    let result = file.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| {
        let data = serde_json::to_vec(hidden).map_err(std::io::Error::other)?;
        std::fs::write(&file, data)
    });
    if let Err(err) = result {
        tracing::warn!("cannot keep the hidden apps: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumes() {
        assert_eq!(parse_volume("Volume: 0.50\n"), Some(Volume { level: 0.5, muted: false }));
        assert_eq!(parse_volume("Volume: 1.20 [MUTED]\n"), Some(Volume { level: 1.2, muted: true }));
        assert_eq!(parse_volume("nonsense"), None);
    }
}

/// How loud a source is, 0 to 1, twenty times a second: its peak over the
/// last 50 ms on a 60 dB scale, read with parec at a low rate so it costs
/// next to nothing. Named and flagged as a peak-detect monitor stream, as
/// pavucontrol's meters are, so the privacy pill doesn't count it as using
/// the microphone. Runs for as long as the stream is polled.
pub fn levels(device: &String) -> impl futures::Stream<Item = f32> + Send + 'static + use<> {
    peaks(format!("--device={device}"))
}

/// An app's live level, by one of its streams (a sink input's index),
/// tagged with the app's name: what the mixer's meters show.
pub fn stream_levels(of: &(u32, String)) -> impl futures::Stream<Item = (String, f32)> + Send + 'static + use<> {
    use futures::StreamExt;
    let app = of.1.clone();
    peaks(format!("--monitor-stream={}", of.0)).map(move |level| (app.clone(), level))
}

/// Peaks from parec recording what `target` says (a device, a stream).
fn peaks(target: String) -> impl futures::Stream<Item = f32> + Send + 'static {
    use futures::SinkExt;
    use tokio::io::AsyncReadExt;
    cosmic::iced::stream::channel(4, move |mut out: futures::channel::mpsc::Sender<f32>| async move {
        let child = tokio::process::Command::new("parec")
            .args([
                target.as_str(),
                "--format=s16le",
                "--rate=8000",
                "--channels=1",
                "--latency-msec=50",
                "--stream-name=Peak detect",
                "--property=stream.monitor=true",
            ])
            .env("LC_ALL", "C")
            .env_remove("LD_LIBRARY_PATH")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn();
        let Ok(mut child) = child else {
            tracing::warn!("cannot run parec for the level meter");
            return;
        };
        let Some(mut stdout) = child.stdout.take() else {
            return;
        };
        // 400 samples: 50 ms at 8 kHz.
        let mut buffer = vec![0u8; 800];
        while stdout.read_exact(&mut buffer).await.is_ok() {
            let peak = buffer.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs()).max().unwrap_or(0);
            if out.send(meter(peak)).await.is_err() {
                break;
            }
        }
    })
}

/// A sample's peak on a meter: -60 dBFS and below empty, 0 dBFS full.
fn meter(peak: u16) -> f32 {
    if peak == 0 {
        return 0.0;
    }
    let db = 20.0 * (f32::from(peak) / 32768.0).log10();
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod level_tests {
    #[test]
    fn meters() {
        assert_eq!(super::meter(0), 0.0);
        assert!(super::meter(32767) > 0.99);
        // -20 dBFS is two thirds up.
        assert!((super::meter(3277) - 0.667).abs() < 0.01);
    }
}
