// SPDX-License-Identifier: GPL-3.0-only

//! Who is listening or watching: apps recording from a microphone, through
//! PulseAudio's view of PipeWire, and apps holding a camera or receiving a
//! screencast, through PipeWire's own graph.

use std::{collections::HashSet, time::Duration};

use cosmic::iced::Subscription;
use serde_json::Value;

/// A screencast from the portal to an app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Share {
    /// The app receiving the stream, e.g. "Chromium", "OBS Studio".
    pub app: String,
    /// "Entire screen · eDP-1", "A window", or "Screen" when unknown.
    pub what: String,
    /// The PipeWire node id of the portal's source stream, for stopping.
    pub node: u32,
}

/// An app with a camera stream open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraUse {
    pub app: String,
    /// "/dev/video0" when PipeWire says which device, else None.
    pub node: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Privacy {
    /// App names recording from a real microphone.
    pub microphone: Vec<String>,
    pub camera: Vec<CameraUse>,
    pub screen: Vec<Share>,
}

impl Privacy {
    pub fn is_empty(&self) -> bool {
        self.microphone.is_empty() && self.camera.is_empty() && self.screen.is_empty()
    }

    pub fn sharing_screen(&self) -> bool {
        !self.screen.is_empty()
    }
}

async fn pactl(what: &str) -> Vec<Value> {
    let output = tokio::process::Command::new("pactl").args(["-f", "json", "list", what]).output().await;
    match output {
        Ok(output) => serde_json::from_slice::<Vec<Value>>(&output.stdout).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn property<'a>(object: &'a Value, key: &str) -> Option<&'a str> {
    object["properties"][key].as_str()
}

/// Apps recording from a microphone: not from a monitor of an output, which
/// is sound capture, and not level meters.
async fn recording() -> Vec<String> {
    let monitors: HashSet<u64> = pactl("sources")
        .await
        .iter()
        .filter(|s| s["monitor_source"].as_str().is_some_and(|m| !m.is_empty() && m != "n/a") || s["name"].as_str().is_some_and(|n| n.ends_with(".monitor")))
        .filter_map(|s| s["index"].as_u64())
        .collect();
    let mut apps = Vec::new();
    for output in pactl("source-outputs").await {
        let from_monitor = output["source"].as_u64().is_some_and(|s| monitors.contains(&s));
        let meter = property(&output, "stream.monitor") == Some("true") || property(&output, "media.name") == Some("Peak detect");
        if from_monitor || meter || output["corked"].as_bool().unwrap_or(false) {
            continue;
        }
        let name = property(&output, "application.name")
            .or_else(|| property(&output, "application.process.binary"))
            .unwrap_or("An app")
            .to_owned();
        if !apps.contains(&name) {
            apps.push(name);
        }
    }
    apps
}

// --- The PipeWire graph, from one `pw-dump` -------------------------------
//
// Props relied on (confirmed in xdg-desktop-portal-cosmic
// `src/screencast_thread.rs`, which builds the stream with
// `media.class => "Video/Source"` and `node.name => "cosmic-screencast"` and
// sets nothing else, and in a local `pw-dump`):
// - a node is `type` "PipeWire:Interface:Node", its props under `info.props`;
// - a link is `type` "PipeWire:Interface:Link" with `info.output-node-id`
//   (the producer) and `info.input-node-id` (the consumer);
// - the portal's screencast node: `media.class` "Video/Source",
//   `node.name` "cosmic-screencast", no `api.v4l2.path`;
// - a camera device: `media.class` "Video/Source", `api.v4l2.path`
//   "/dev/video0", `object.path` "v4l2:/dev/video0", `media.role` "Camera";
// - a consuming app: `media.class` "Stream/Input/Video", `application.name`.

struct Node<'a> {
    id: u32,
    props: &'a Value,
}

fn str_of<'a>(props: &'a Value, key: &str) -> Option<&'a str> {
    props[key].as_str()
}

fn has_any(haystack: &str, needles: &[&str]) -> bool {
    let lower = haystack.to_lowercase();
    needles.iter().any(|n| lower.contains(n))
}

/// The portal's screencast source, as opposed to a camera or any other video
/// producer.
fn is_portal_source(props: &Value) -> bool {
    let class = str_of(props, "media.class").unwrap_or("");
    if class != "Video/Source" && class != "Stream/Output/Video" {
        return false;
    }
    if props["api.v4l2.path"].is_string() {
        return false;
    }
    let by_app = str_of(props, "application.name").is_some_and(|a| has_any(a, &["xdg-desktop-portal", "cosmic"]));
    let by_name = ["node.name", "node.description"]
        .iter()
        .filter_map(|k| str_of(props, k))
        .any(|s| has_any(s, &["screencast", "screen-cast", "portal"]));
    by_app || by_name
}

/// A display connector name such as eDP-1, DP-3 or HDMI-A-1 in the text.
fn connector(text: &str) -> Option<&str> {
    const PREFIXES: [&str; 11] = ["edp", "dp", "hdmi-a", "hdmi-b", "dvi-d", "dvi-i", "vga", "lvds", "dsi", "virtual", "writeback"];
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).find(|token| {
        let Some((prefix, number)) = token.rsplit_once('-') else {
            return false;
        };
        !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) && PREFIXES.contains(&prefix.to_lowercase().as_str())
    })
}

/// What a portal source shows, from whatever it says about itself.
fn describe(props: &Value) -> String {
    let Some(map) = props.as_object() else {
        return "Screen".to_owned();
    };
    let text: Vec<&str> = map
        .iter()
        .filter(|(k, _)| {
            let k = k.to_lowercase();
            k == "node.description" || k == "media.name" || k.contains("output") || k.contains("screencast") || k.contains("source")
        })
        .filter_map(|(_, v)| v.as_str())
        .collect();
    let joined = text.join(" ");
    if let Some(connector) = connector(&joined) {
        return format!("Entire screen · {connector}");
    }
    if has_any(&joined, &["window"]) {
        return "A window".to_owned();
    }
    "Screen".to_owned()
}

fn app_name(props: &Value) -> String {
    str_of(props, "application.name").or_else(|| str_of(props, "node.name")).unwrap_or("An app").to_owned()
}

/// The device behind a camera node.
fn device_path(props: &Value) -> Option<String> {
    str_of(props, "api.v4l2.path")
        .or_else(|| str_of(props, "device.path"))
        .or_else(|| str_of(props, "object.path").map(|p| p.strip_prefix("v4l2:").unwrap_or(p)))
        .map(str::to_owned)
}

/// Screencasts and camera streams in a parsed `pw-dump`.
fn parse(objects: &[Value]) -> (Vec<Share>, Vec<CameraUse>) {
    let mut nodes: Vec<Node> = Vec::new();
    // (producer, consumer)
    let mut links: Vec<(u32, u32)> = Vec::new();
    for object in objects {
        let Some(kind) = object["type"].as_str() else { continue };
        let Some(id) = object["id"].as_u64() else { continue };
        if kind.ends_with("Interface:Node") {
            nodes.push(Node { id: id as u32, props: &object["info"]["props"] });
        } else if kind.ends_with("Interface:Link") {
            let info = &object["info"];
            if let (Some(out), Some(inp)) = (info["output-node-id"].as_u64(), info["input-node-id"].as_u64()) {
                links.push((out as u32, inp as u32));
            }
        }
    }
    let node = |id: u32| nodes.iter().find(|n| n.id == id);
    let portals: Vec<&Node> = nodes.iter().filter(|n| is_portal_source(n.props)).collect();
    let portal_ids: HashSet<u32> = portals.iter().map(|n| n.id).collect();
    // A consumer not linked yet may still name its target node.
    let portal_serials: HashSet<u64> = portals.iter().filter_map(|n| n.props["object.serial"].as_u64()).collect();
    let targets_portal = |props: &Value| {
        ["target.object", "node.target"].iter().any(|k| {
            let v = &props[k];
            let n = v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()));
            n.is_some_and(|n| portal_ids.contains(&(n as u32)) || portal_serials.contains(&n))
        })
    };

    let mut screen: Vec<Share> = Vec::new();
    for portal in &portals {
        let what = describe(portal.props);
        let consumers: Vec<String> = links
            .iter()
            .filter(|(out, _)| *out == portal.id)
            .filter_map(|(_, inp)| node(*inp))
            .map(|n| app_name(n.props))
            .chain(nodes.iter().filter(|n| targets_portal(n.props) && str_of(n.props, "media.class") == Some("Stream/Input/Video")).map(|n| app_name(n.props)))
            .collect();
        let apps = if consumers.is_empty() { vec![str_of(portal.props, "application.name").unwrap_or("An app").to_owned()] } else { consumers };
        for app in apps {
            let share = Share { app, what: what.clone(), node: portal.id };
            if !screen.contains(&share) {
                screen.push(share);
            }
        }
    }

    let mut camera: Vec<CameraUse> = Vec::new();
    for consumer in nodes.iter().filter(|n| str_of(n.props, "media.class") == Some("Stream/Input/Video")) {
        let producers: Vec<u32> = links.iter().filter(|(_, inp)| *inp == consumer.id).map(|(out, _)| *out).collect();
        if producers.iter().any(|p| portal_ids.contains(p)) || targets_portal(consumer.props) {
            continue;
        }
        let app = app_name(consumer.props);
        let path = producers.iter().filter_map(|p| node(*p)).find_map(|n| device_path(n.props));
        let usage = CameraUse { app, node: path };
        if !camera.contains(&usage) {
            camera.push(usage);
        }
    }
    (screen, camera)
}

/// Screencasts and camera streams open in PipeWire right now.
async fn video() -> (Vec<Share>, Vec<CameraUse>) {
    let Ok(output) = tokio::process::Command::new("pw-dump").output().await else {
        return Default::default();
    };
    let Ok(objects) = serde_json::from_slice::<Vec<Value>>(&output.stdout) else {
        return Default::default();
    };
    parse(&objects)
}

pub async fn now() -> Option<Privacy> {
    let (screen, mut camera) = video().await;
    // Apps that open the device themselves, past PipeWire: Electron, say.
    for held in tokio::task::spawn_blocking(holders).await.unwrap_or_default() {
        if !camera.iter().any(|c| c.app.eq_ignore_ascii_case(&held.app)) {
            camera.push(held);
        }
    }
    Some(Privacy { microphone: recording().await, camera, screen })
}

/// Processes holding a camera device node open, named for the app they are.
fn holders() -> Vec<CameraUse> {
    let me = std::process::id();
    let mut found: Vec<(u32, String)> = Vec::new();
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    for proc in procs.flatten() {
        let Some(pid) = proc.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        if pid == me {
            continue;
        }
        let Ok(fds) = std::fs::read_dir(proc.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            let Ok(target) = std::fs::read_link(fd.path()) else {
                continue;
            };
            let target = target.to_string_lossy();
            if target.starts_with("/dev/video") && !found.iter().any(|(p, _)| *p == pid) {
                found.push((pid, target.into_owned()));
            }
        }
    }
    let mut users: Vec<CameraUse> = Vec::new();
    for (pid, node) in found {
        let app = process_name(pid);
        if !users.iter().any(|u| u.app == app && u.node.as_deref() == Some(node.as_str())) {
            users.push(CameraUse { app, node: Some(node) });
        }
    }
    users
}

/// Runtimes that say nothing about which app this is.
const RUNTIMES: &[&str] = &["electron", "electron-unwrapped", "chromium-unwrapped", "chrome", "node", "nodejs", "python3", "python", "java", "sh", "bash", "wine", "wine64", "mono", "ruby", "perl"];

/// The app a process belongs to, by its name or its parents': the Nix store
/// package it runs from (an Electron app's own package, not Electron's), else
/// the program's name.
pub(crate) fn process_name(pid: u32) -> String {
    let read = |pid: u32, what: &str| std::fs::read_to_string(format!("/proc/{pid}/{what}")).unwrap_or_default();
    let mut fallback: Option<String> = None;
    let mut pid = pid;
    for _ in 0..5 {
        let cmdline = read(pid, "cmdline");
        let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
        let comm = read(pid, "comm").trim().to_owned();
        let program = unwrap_nix(if exe.is_empty() { comm.clone() } else { exe });
        if fallback.is_none() && !program.is_empty() {
            fallback = Some(program.clone());
        }
        if !RUNTIMES.contains(&program.as_str()) && !program.is_empty() {
            return program;
        }
        // A store path among the arguments names the app's own package. Chromium
        // and Electron rewrite their arguments into one space-separated string.
        for arg in cmdline.split(['\0', ' ']) {
            if let Some(name) = store_package(arg).filter(|n| !RUNTIMES.contains(&n.as_str())) {
                return name;
            }
        }
        let status = read(pid, "status");
        let Some(parent) = status.lines().find_map(|l| l.strip_prefix("PPid:")).and_then(|p| p.trim().parse::<u32>().ok()).filter(|p| *p > 1) else {
            break;
        };
        pid = parent;
    }
    fallback.unwrap_or_else(|| "An app".to_owned())
}

/// `.spotify-wrapped` -> `spotify`.
fn unwrap_nix(name: String) -> String {
    match name.strip_prefix('.').and_then(|n| n.strip_suffix("-wrapped")) {
        Some(inner) => inner.to_owned(),
        None => name,
    }
}

/// The package name in a Nix store path: `/nix/store/<hash>-vesktop-1.6.8/...` -> `vesktop`.
fn store_package(arg: &str) -> Option<String> {
    let rest = arg.split("/nix/store/").nth(1)?;
    let component = rest.split('/').next()?;
    let (_, named) = component.split_at_checked(33)?;
    let mut name = String::new();
    for part in named.split('-') {
        if part.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            break;
        }
        if !name.is_empty() {
            name.push('-');
        }
        name.push_str(part);
    }
    (!name.is_empty()).then_some(name)
}

pub fn subscription() -> Subscription<Privacy> {
    Subscription::run(|| super::watch(Duration::from_secs(3), super::no_changes, now))
}

/// Ends a screencast by destroying the portal's source node.
pub async fn stop_share(node: u32) {
    match tokio::process::Command::new("pw-cli").args(["destroy", &node.to_string()]).output().await {
        Ok(output) if output.status.success() => {}
        Ok(output) => tracing::warn!("cannot stop screencast node {node}: {}", String::from_utf8_lossy(&output.stderr).trim()),
        Err(err) => tracing::warn!("cannot run pw-cli to stop screencast node {node}: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(id: u32, props: Value) -> Value {
        json!({ "id": id, "type": "PipeWire:Interface:Node", "info": { "props": props } })
    }

    fn link(id: u32, out: u32, inp: u32) -> Value {
        json!({ "id": id, "type": "PipeWire:Interface:Link", "info": { "output-node-id": out, "input-node-id": inp } })
    }

    #[test]
    fn portal_share_to_consumer() {
        let objects = vec![
            node(90, json!({ "media.class": "Video/Source", "node.name": "cosmic-screencast", "application.name": "xdg-desktop-portal-cosmic", "object.serial": 900 })),
            node(91, json!({ "media.class": "Stream/Input/Video", "application.name": "Chromium" })),
            link(92, 90, 91),
        ];
        let (screen, camera) = parse(&objects);
        assert_eq!(screen, vec![Share { app: "Chromium".into(), what: "Screen".into(), node: 90 }]);
        assert!(camera.is_empty(), "a screencast consumer is not a camera user");
    }

    #[test]
    fn portal_share_without_link_and_with_connector() {
        let objects = vec![node(90, json!({ "media.class": "Video/Source", "node.name": "cosmic-screencast", "node.description": "Screencast of eDP-1" }))];
        let (screen, _) = parse(&objects);
        assert_eq!(screen, vec![Share { app: "An app".into(), what: "Entire screen · eDP-1".into(), node: 90 }]);
    }

    #[test]
    fn window_share() {
        let objects = vec![node(7, json!({ "media.class": "Stream/Output/Video", "node.name": "portal-window", "media.name": "Window: Firefox" }))];
        let (screen, _) = parse(&objects);
        assert_eq!(screen[0].what, "A window");
    }

    #[test]
    fn camera_consumer_with_v4l2_path() {
        let objects = vec![
            node(79, json!({ "media.class": "Video/Source", "api.v4l2.path": "/dev/video0", "object.path": "v4l2:/dev/video0", "node.name": "v4l2_input.pci-0000_c3_00.4-usb-0_1_1.0", "media.role": "Camera" })),
            node(80, json!({ "media.class": "Stream/Input/Video", "application.name": "OBS Studio" })),
            node(81, json!({ "media.class": "Stream/Input/Video", "application.name": "OBS Studio" })),
            link(82, 79, 80),
            link(83, 79, 81),
        ];
        let (screen, camera) = parse(&objects);
        assert!(screen.is_empty(), "a camera is not a portal source");
        assert_eq!(camera, vec![CameraUse { app: "OBS Studio".into(), node: Some("/dev/video0".into()) }]);
    }

    #[test]
    fn camera_consumer_without_link() {
        let objects = vec![node(80, json!({ "media.class": "Stream/Input/Video", "node.name": "cheese" }))];
        let (_, camera) = parse(&objects);
        assert_eq!(camera, vec![CameraUse { app: "cheese".into(), node: None }]);
    }

    #[test]
    fn consumer_targeting_portal_is_a_share() {
        let objects = vec![
            node(90, json!({ "media.class": "Video/Source", "node.name": "cosmic-screencast" })),
            node(91, json!({ "media.class": "Stream/Input/Video", "application.name": "Zoom", "target.object": "90" })),
        ];
        let (screen, camera) = parse(&objects);
        assert_eq!(screen, vec![Share { app: "Zoom".into(), what: "Screen".into(), node: 90 }]);
        assert!(camera.is_empty());
    }

    #[test]
    fn connectors() {
        assert_eq!(connector("Entire screen on DP-3"), Some("DP-3"));
        assert_eq!(connector("HDMI-A-1 mirrored"), Some("HDMI-A-1"));
        assert_eq!(connector("cosmic-screencast"), None);
        assert_eq!(connector("v4l2_input.pci-0000_c3_00.4-usb-0_1_1.0"), None);
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;

    #[test]
    fn store_paths_name_the_package() {
        assert_eq!(store_package("/nix/store/h4iw21fgj2rklj5r3msspdp3syyr0fq6-vesktop-1.6.8/opt/Vesktop/resources/app.asar").as_deref(), Some("vesktop"));
        assert_eq!(store_package("/nix/store/rs2nyh0mmp8hc73hr0v04jsdsfjncf58-electron-unwrapped-44.9.3/libexec/electron/electron").as_deref(), Some("electron-unwrapped"));
        assert_eq!(store_package("--type=utility"), None);
        assert_eq!(unwrap_nix(".spotify-wrapped".to_owned()), "spotify");
    }
}

#[cfg(test)]
mod live_tests {
    /// `QS_PID=<pid> cargo test --release --lib privacy::live_tests::name -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn name() {
        if let Ok(pid) = std::env::var("QS_PID") {
            println!("process {pid} is {:?}", super::process_name(pid.parse().unwrap()));
            let cmdline = std::fs::read_to_string(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            for arg in cmdline.split('\0') {
                println!("  arg {:?} -> {:?}", &arg[..arg.len().min(60)], super::store_package(arg));
            }
            println!("  exe {:?}", std::fs::read_link(format!("/proc/{pid}/exe")));
        }
    }
}
