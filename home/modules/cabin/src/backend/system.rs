// SPDX-License-Identifier: GPL-3.0-only

//! The machine's vitals: CPU, memory, disk, network, power draw, fan.
//! Everything comes from /proc and /sys, sampled on a timer; nothing here
//! talks to a daemon.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use cosmic::iced::{Subscription, stream};
use futures::{SinkExt, Stream};
use serde::{Deserialize, Serialize};

/// How many samples the graphs keep.
const HISTORY: usize = 40;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Cpu {
    pub percent: f32,
    pub model: String,
    pub threads: usize,
    pub temp_c: Option<f32>,
    /// The last 40 samples of `percent`, oldest first.
    pub history: Vec<f32>,
}

/// Bytes.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Memory {
    pub used: u64,
    pub total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
}

/// Bytes, for the root filesystem.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Disk {
    pub used: u64,
    pub total: u64,
    pub free: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Wifi,
    Ethernet,
    Vpn,
    Bridge,
    Loopback,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Interface {
    pub name: String,
    pub kind: Kind,
    pub up: bool,
    /// The first IPv4 address, like "100.84.12.7".
    pub address: Option<String>,
    /// Bytes per second since the last sample.
    pub down_rate: f64,
    pub up_rate: f64,
    /// The link speed, when the driver knows it.
    pub speed_mbps: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Network {
    /// The interface holding the default route.
    pub active: Option<String>,
    /// Bytes per second over the active interface.
    pub down_rate: f64,
    pub up_rate: f64,
    /// The last 40 samples of the rates, oldest first.
    pub down_history: Vec<f64>,
    pub up_history: Vec<f64>,
    /// Bytes moved today over the active interface.
    pub today_rx: u64,
    pub today_tx: u64,
    /// Everything but the loopback: the active one first, then the ones
    /// that are up, then the rest.
    pub interfaces: Vec<Interface>,
}

/// Watts. `total_w` is the battery's drain, only while discharging;
/// `cpu_w` is the CPU package through RAPL, when it may be read.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Draw {
    pub total_w: Option<f32>,
    pub cpu_w: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct System {
    pub cpu: Cpu,
    pub memory: Memory,
    pub disk: Disk,
    pub network: Network,
    pub draw: Draw,
    /// None without a fan sensor; Some(0) when the fan is stopped.
    pub fan_rpm: Option<u32>,
    pub gpu_temp_c: Option<f32>,
    /// The system's generation and how old its nixpkgs is.
    pub nixos: Option<super::nixos::NixOs>,
    /// The programs using the most CPU, most first: read only while the
    /// System panel shows (sampling is fast then), else empty.
    pub processes: Vec<Process>,
}

/// A program's share of the machine, its processes added up.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Process {
    pub name: String,
    /// Percent of the whole machine's CPU.
    pub cpu: f32,
    /// Resident memory, in bytes.
    pub memory: u64,
}

static FAST: AtomicBool = AtomicBool::new(false);

/// Sample every 1.5 s while the System panel or a menu needing it shows,
/// every 2 s otherwise (the panel button's bars are live too).
pub fn set_fast(fast: bool) {
    FAST.store(fast, Ordering::Relaxed);
}

fn interval() -> Duration {
    if FAST.load(Ordering::Relaxed) { Duration::from_millis(1500) } else { Duration::from_secs(2) }
}

/// "3.8 GB", "412 MB", "9 KB": decimal units, one decimal from GB up.
pub fn bytes(b: u64) -> String {
    let b = b as f64;
    if b >= 1e12 {
        format!("{:.1} TB", b / 1e12)
    } else if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{b:.0} B")
    }
}

/// "1.2 MB/s", "90 KB/s": decimal units, one decimal from MB up.
pub fn rate(bytes_per_second: f64) -> String {
    let b = bytes_per_second.max(0.0);
    if b >= 1e9 {
        format!("{:.1} GB/s", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB/s", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB/s", b / 1e3)
    } else {
        format!("{b:.0} B/s")
    }
}

fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_owned())
}

fn read_u64(path: impl AsRef<Path>) -> Option<u64> {
    read(path)?.parse().ok()
}

/// Millidegrees in a sysfs file, as degrees.
fn read_temp(path: impl AsRef<Path>) -> Option<f32> {
    read(path)?.parse::<f32>().ok().map(|m| m / 1000.0)
}

fn push_capped<T>(history: &mut Vec<T>, value: T) {
    history.push(value);
    if history.len() > HISTORY {
        let extra = history.len() - HISTORY;
        history.drain(..extra);
    }
}

// CPU

/// Idle and total jiffies from the first line of /proc/stat.
fn parse_stat(line: &str) -> Option<(u64, u64)> {
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    let values: Vec<u64> = fields.take(8).map_while(|f| f.parse().ok()).collect();
    if values.len() < 4 {
        return None;
    }
    let idle = values[3] + values.get(4).copied().unwrap_or(0);
    Some((idle, values.iter().sum()))
}

/// Busy time between two readings of /proc/stat, in percent.
fn cpu_percent(prev: (u64, u64), now: (u64, u64)) -> f32 {
    let total = now.1.saturating_sub(prev.1);
    if total == 0 {
        return 0.0;
    }
    let idle = now.0.saturating_sub(prev.0).min(total);
    ((total - idle) as f64 / total as f64 * 100.0) as f32
}

fn cpu_model() -> String {
    read("/proc/cpuinfo")
        .and_then(|info| info.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split_once(':')).map(|(_, m)| m.trim().to_owned()))
        .unwrap_or_default()
}

fn cpu_threads() -> usize {
    let counted = read("/proc/cpuinfo").map(|info| info.lines().filter(|l| l.starts_with("processor")).count()).unwrap_or(0);
    if counted > 0 { counted } else { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) }
}

// Memory and disk

fn memory() -> Memory {
    let Some(info) = read("/proc/meminfo") else {
        return Memory::default();
    };
    let mut fields: HashMap<&str, u64> = HashMap::new();
    for line in info.lines() {
        if let Some((key, rest)) = line.split_once(':') {
            if let Some(kb) = rest.split_whitespace().next().and_then(|v| v.parse::<u64>().ok()) {
                fields.insert(key, kb * 1024);
            }
        }
    }
    let get = |key: &str| fields.get(key).copied().unwrap_or(0);
    let total = get("MemTotal");
    let swap_total = get("SwapTotal");
    Memory {
        used: total.saturating_sub(get("MemAvailable")),
        total,
        swap_used: swap_total.saturating_sub(get("SwapFree")),
        swap_total,
    }
}

fn disk() -> Disk {
    // SAFETY: statvfs fills a plain C struct we own; a zeroed one is a
    // valid starting value, and the path is a NUL-terminated literal.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c"/".as_ptr(), &mut st) } != 0 {
        return Disk::default();
    }
    let frsize = st.f_frsize as u64;
    Disk {
        used: (st.f_blocks as u64).saturating_sub(st.f_bfree as u64) * frsize,
        total: st.f_blocks as u64 * frsize,
        free: st.f_bavail as u64 * frsize,
    }
}

// Network

/// The interface holding the default route with the lowest metric.
fn parse_default_route(route: &str) -> Option<String> {
    route
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 8 || fields[1] != "00000000" || fields[7] != "00000000" {
                return None;
            }
            let metric: u64 = fields[6].parse().ok()?;
            Some((metric, fields[0].to_owned()))
        })
        .min()
        .map(|(_, iface)| iface)
}

fn default_route() -> Option<String> {
    parse_default_route(&read("/proc/net/route")?)
}

fn interface_kind(name: &str, path: &Path) -> Kind {
    if path.join("wireless").is_dir() {
        Kind::Wifi
    } else if name.starts_with("lo") {
        Kind::Loopback
    } else if path.join("bridge").exists() || name.starts_with("docker") || name.starts_with("br-") {
        Kind::Bridge
    } else if ["tailscale", "wg", "tun", "tap"].iter().any(|p| name.starts_with(p)) || path.join("tun_flags").exists() {
        Kind::Vpn
    } else if path.join("device").exists() {
        Kind::Ethernet
    } else {
        Kind::Other
    }
}

/// The first IPv4 address of each interface.
fn ipv4_addresses() -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs hands us a linked list we walk read-only and free
    // once, checking each pointer before dereferencing it.
    unsafe {
        if libc::getifaddrs(&mut list) != 0 {
            return out;
        }
        let mut cur = list;
        while !cur.is_null() {
            let ifa = &*cur;
            if !ifa.ifa_addr.is_null() && i32::from((*ifa.ifa_addr).sa_family) == libc::AF_INET && !ifa.ifa_name.is_null() {
                let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                let ip = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                let name = std::ffi::CStr::from_ptr(ifa.ifa_name).to_string_lossy().into_owned();
                out.entry(name).or_insert_with(|| ip.to_string());
            }
            cur = ifa.ifa_next;
        }
        libc::freeifaddrs(list);
    }
    out
}

/// The byte counters of each interface, received and sent.
type Counters = HashMap<String, (u64, u64)>;

// Today's traffic

/// The active interface's counters at the first sample of the day.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Baseline {
    date: String,
    iface: String,
    rx: u64,
    tx: u64,
}

fn baseline_file() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(dir.join("cabin").join("net-today.json"))
}

fn load_baseline() -> Option<Baseline> {
    serde_json::from_slice(&fs::read(baseline_file()?).ok()?).ok()
}

fn save_baseline(baseline: &Baseline) {
    let Some(file) = baseline_file() else {
        return;
    };
    let write = || -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&file, serde_json::to_vec(baseline)?)
    };
    if let Err(err) = write() {
        tracing::warn!("cannot save today's network baseline: {err}");
    }
}

// Sensors

/// The sysfs files worth reading, found once.
struct Sensors {
    cpu_temp: Option<PathBuf>,
    fan: Option<PathBuf>,
    /// Every amdgpu hwmon; which is awake is checked at each sample.
    gpus: Vec<PathBuf>,
    rapl: Option<PathBuf>,
}

impl Sensors {
    fn find() -> Self {
        let hwmons: Vec<(String, PathBuf)> = fs::read_dir("/sys/class/hwmon")
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| Some((read(entry.path().join("name"))?, entry.path())))
            .collect();
        let named = |name: &str| hwmons.iter().find(|(n, _)| n == name).map(|(_, p)| p.clone());
        let cpu_temp = ["k10temp", "coretemp", "acpitz"].iter().find_map(|n| named(n)).map(|p| p.join("temp1_input")).filter(|p| p.exists());
        let fan = named("cros_ec")
            .map(|p| p.join("fan1_input"))
            .filter(|p| p.exists())
            .or_else(|| hwmons.iter().map(|(_, p)| p.join("fan1_input")).find(|p| p.exists()));
        let gpus = hwmons.iter().filter(|(n, _)| n == "amdgpu").map(|(_, p)| p.clone()).collect();
        let rapl = Some(PathBuf::from("/sys/class/powercap/intel-rapl:0/energy_uj")).filter(|p| read_u64(p).is_some());
        Self { cpu_temp, fan, gpus, rapl }
    }

    /// The temperature of an awake GPU. A suspended one is left alone:
    /// reading its sensors would wake it.
    fn gpu_temp(&self) -> Option<f32> {
        self.gpus
            .iter()
            .filter(|p| read(p.join("device/power/runtime_status")).as_deref() != Some("suspended"))
            .find_map(|p| read_temp(p.join("temp1_input")))
    }
}

/// The battery's drain while discharging, in watts.
fn battery_draw() -> Option<f32> {
    fs::read_dir("/sys/class/power_supply").ok()?.filter_map(|entry| entry.ok()).map(|entry| entry.path()).find_map(|bat| {
        if !bat.file_name()?.to_str()?.starts_with("BAT") || read(bat.join("status")).as_deref() != Some("Discharging") {
            return None;
        }
        if let Some(uw) = read_u64(bat.join("power_now")) {
            return Some(uw as f32 / 1e6);
        }
        let ua = read_u64(bat.join("current_now"))? as f64;
        let uv = read_u64(bat.join("voltage_now"))? as f64;
        Some((ua * uv / 1e12) as f32)
    })
}

// Sampling

/// What one sample needs from the one before it.
struct Sampler {
    sensors: Sensors,
    model: String,
    threads: usize,
    at: Instant,
    stat: Option<(u64, u64)>,
    counters: Counters,
    energy_uj: Option<u64>,
    baseline: Option<Baseline>,
    cpu_history: Vec<f32>,
    down_history: Vec<f64>,
    up_history: Vec<f64>,
    /// Each process's CPU time at the last reading, in clock ticks.
    ticks: HashMap<u32, u64>,
    /// Each process's program, read once.
    names: HashMap<u32, String>,
}

impl Sampler {
    fn new() -> Self {
        Self {
            sensors: Sensors::find(),
            model: cpu_model(),
            threads: cpu_threads(),
            at: Instant::now(),
            stat: None,
            counters: Counters::new(),
            energy_uj: None,
            baseline: load_baseline(),
            cpu_history: Vec::new(),
            down_history: Vec::new(),
            up_history: Vec::new(),
            ticks: HashMap::new(),
            names: HashMap::new(),
        }
    }

    /// The busiest programs since the last reading, while sampling is fast.
    fn processes(&mut self, elapsed: f64) -> Vec<Process> {
        if !FAST.load(Ordering::Relaxed) {
            self.ticks.clear();
            self.names.clear();
            return Vec::new();
        }
        // SAFETY: sysconf only reads a constant.
        let (hertz, page) = unsafe { (libc::sysconf(libc::_SC_CLK_TCK), libc::sysconf(libc::_SC_PAGESIZE)) };
        let (hertz, page) = (hertz.max(1) as f64, page.max(1) as u64);
        let whole = elapsed * hertz * self.threads.max(1) as f64;
        let seeded = !self.ticks.is_empty();
        let mut ticks = HashMap::new();
        let mut names = HashMap::new();
        let mut programs: HashMap<String, Process> = HashMap::new();
        for entry in fs::read_dir("/proc").into_iter().flatten().filter_map(Result::ok) {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let Some((comm, used, rss)) = read(entry.path().join("stat")).as_deref().and_then(parse_pid_stat) else {
                continue;
            };
            // Kernel threads hold no memory of their own.
            if rss == 0 {
                continue;
            }
            let cpu = match self.ticks.get(&pid) {
                Some(before) if seeded => used.saturating_sub(*before) as f64 / whole * 100.0,
                _ => 0.0,
            };
            ticks.insert(pid, used);
            let name = self.names.remove(&pid).unwrap_or_else(|| program_name(&entry.path(), &comm));
            names.insert(pid, name.clone());
            let program = programs.entry(name.clone()).or_insert_with(|| Process { name, ..Process::default() });
            program.cpu += cpu as f32;
            program.memory += rss * page;
        }
        self.ticks = ticks;
        self.names = names;
        let mut list: Vec<Process> = programs.into_values().collect();
        list.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.memory.cmp(&a.memory)));
        list.truncate(5);
        list
    }

    fn sample(&mut self) -> System {
        let now = Instant::now();
        let elapsed = now.duration_since(self.at).as_secs_f64().max(1e-3);
        self.at = now;

        let stat = read("/proc/stat").and_then(|s| parse_stat(s.lines().next().unwrap_or_default()));
        let percent = match (self.stat, stat) {
            (Some(prev), Some(now)) => cpu_percent(prev, now),
            _ => 0.0,
        };
        self.stat = stat;
        push_capped(&mut self.cpu_history, percent);

        let network = self.network(elapsed);

        let energy = self.sensors.rapl.as_ref().and_then(read_u64);
        let cpu_w = match (self.energy_uj, energy) {
            (Some(prev), Some(now)) if now >= prev => Some(((now - prev) as f64 / 1e6 / elapsed) as f32),
            _ => None,
        };
        self.energy_uj = energy;

        System {
            cpu: Cpu {
                percent,
                model: self.model.clone(),
                threads: self.threads,
                temp_c: self.sensors.cpu_temp.as_ref().and_then(read_temp),
                history: self.cpu_history.clone(),
            },
            memory: memory(),
            disk: disk(),
            network,
            draw: Draw { total_w: battery_draw(), cpu_w },
            fan_rpm: self.sensors.fan.as_ref().and_then(read_u64).map(|rpm| rpm as u32),
            gpu_temp_c: self.sensors.gpu_temp(),
            nixos: super::nixos::read(),
            processes: self.processes(elapsed),
        }
    }

    fn network(&mut self, elapsed: f64) -> Network {
        let active = default_route();
        let addresses = ipv4_addresses();
        let mut counters = Counters::new();
        let mut interfaces = Vec::new();
        for entry in fs::read_dir("/sys/class/net").into_iter().flatten().filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let kind = interface_kind(&name, &path);
            let rx = read_u64(path.join("statistics/rx_bytes")).unwrap_or(0);
            let tx = read_u64(path.join("statistics/tx_bytes")).unwrap_or(0);
            let (prev_rx, prev_tx) = self.counters.get(&name).copied().unwrap_or((rx, tx));
            counters.insert(name.clone(), (rx, tx));
            if kind == Kind::Loopback {
                continue;
            }
            let up = match read(path.join("operstate")).as_deref() {
                Some("up") => true,
                Some("unknown") => read(path.join("carrier")).as_deref() == Some("1"),
                _ => false,
            };
            interfaces.push(Interface {
                address: addresses.get(&name).cloned(),
                kind,
                up,
                down_rate: rx.saturating_sub(prev_rx) as f64 / elapsed,
                up_rate: tx.saturating_sub(prev_tx) as f64 / elapsed,
                speed_mbps: read(path.join("speed")).and_then(|s| s.parse::<i64>().ok()).filter(|s| *s > 0).map(|s| s as u32),
                name,
            });
        }
        self.counters = counters;
        interfaces.sort_by(|a, b| {
            let rank = |i: &Interface| (Some(&i.name) != active.as_ref(), !i.up);
            rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name))
        });

        let (down_rate, up_rate) = interfaces.first().filter(|i| Some(&i.name) == active.as_ref()).map(|i| (i.down_rate, i.up_rate)).unwrap_or((0.0, 0.0));
        push_capped(&mut self.down_history, down_rate);
        push_capped(&mut self.up_history, up_rate);

        let (today_rx, today_tx) = match active.as_ref().and_then(|a| Some((a, self.counters.get(a)?))) {
            Some((iface, &(rx, tx))) => self.today(iface, rx, tx),
            None => (0, 0),
        };

        Network { active, down_rate, up_rate, down_history: self.down_history.clone(), up_history: self.up_history.clone(), today_rx, today_tx, interfaces }
    }

    /// Bytes since the day's baseline, which starts over on a new day, a
    /// new interface, or counters that went backwards.
    fn today(&mut self, iface: &str, rx: u64, tx: u64) -> (u64, u64) {
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let stale = self.baseline.as_ref().is_none_or(|b| b.date != date || b.iface != iface || rx < b.rx || tx < b.tx);
        if stale {
            let baseline = Baseline { date, iface: iface.to_owned(), rx, tx };
            save_baseline(&baseline);
            self.baseline = Some(baseline);
        }
        let b = self.baseline.as_ref().expect("set just above");
        (rx - b.rx, tx - b.tx)
    }
}

/// A process's name, CPU time (user and system, in ticks) and resident
/// pages, from its `stat`. The name is in parentheses and may hold spaces.
fn parse_pid_stat(stat: &str) -> Option<(String, u64, u64)> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = stat.get(open + 1..close)?.to_owned();
    // After the name, from the state (field 3): utime is field 14, stime 15, rss 24.
    let fields: Vec<&str> = stat.get(close + 1..)?.split_whitespace().collect();
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    let rss: u64 = fields.get(21)?.parse().ok()?;
    Some((comm, utime + stime, rss))
}

/// The program a process runs: its command's file name (some programs
/// write their whole command line into the first argument), with a Nix
/// wrapper's dot and "-wrapped" taken off; else the kernel's short name.
fn program_name(dir: &Path, comm: &str) -> String {
    let cmdline = fs::read(dir.join("cmdline")).unwrap_or_default();
    let first = cmdline.split(|b| *b == 0).next().unwrap_or_default();
    let first = String::from_utf8_lossy(first);
    let command = first.split(' ').next().unwrap_or_default();
    let base = Path::new(command).file_name().and_then(|n| n.to_str()).filter(|n| !n.is_empty()).unwrap_or(comm);
    clean_program(base)
}

fn clean_program(name: &str) -> String {
    let name = name.trim_start_matches('.');
    name.strip_suffix("-wrapped").unwrap_or(name).to_owned()
}

fn samples() -> impl Stream<Item = System> + Send {
    stream::channel(4, |mut out: futures::channel::mpsc::Sender<System>| async move {
        let mut sampler = tokio::task::spawn_blocking(|| {
            // The first reading only seeds the diffs.
            let mut sampler = Sampler::new();
            sampler.sample();
            sampler
        })
        .await
        .expect("sampler setup");
        tokio::time::sleep(Duration::from_millis(400)).await;
        loop {
            let (next, system) = tokio::task::spawn_blocking(move || {
                let system = sampler.sample();
                (sampler, system)
            })
            .await
            .expect("sampler");
            sampler = next;
            if out.send(system).await.is_err() {
                return;
            }
            // Wait in short steps so opening the menu speeds up sampling
            // right away.
            let started = Instant::now();
            while started.elapsed() < interval() {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    })
}

pub fn subscription() -> Subscription<System> {
    Subscription::run(samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(bytes(3_800_000_000), "3.8 GB");
        assert_eq!(bytes(412_000_000), "412 MB");
        assert_eq!(bytes(9_000), "9 KB");
        assert_eq!(bytes(9_400), "9 KB");
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1_500_000_000_000), "1.5 TB");
    }

    #[test]
    fn rates() {
        assert_eq!(rate(1_200_000.0), "1.2 MB/s");
        assert_eq!(rate(90_000.0), "90 KB/s");
        assert_eq!(rate(0.0), "0 B/s");
        assert_eq!(rate(-5.0), "0 B/s");
        assert_eq!(rate(2_500_000_000.0), "2.5 GB/s");
    }

    #[test]
    fn stat_percent() {
        let before = parse_stat("cpu  1000 0 500 8000 500 0 0 0 0 0").unwrap();
        assert_eq!(before, (8500, 10000));
        // 1000 more jiffies, 250 of them idle or waiting.
        let after = parse_stat("cpu  1500 0 750 8700 550 0 0 0 0 0").unwrap();
        assert_eq!(after, (9250, 11500));
        assert!((cpu_percent(before, after) - 50.0).abs() < 0.01);
        assert_eq!(cpu_percent(before, before), 0.0);
        assert_eq!(cpu_percent(after, before), 0.0);
        assert_eq!(parse_stat("cpu0 1 2 3 4"), None);
        assert_eq!(parse_stat("intr 5"), None);
    }

    #[test]
    fn default_route_lowest_metric() {
        let route = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
            wlp192s0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n\
            enp197s0f0u1\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
            enp197s0f0u1\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n";
        assert_eq!(parse_default_route(route).as_deref(), Some("enp197s0f0u1"));
        let none = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
            enp197s0f0u1\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n";
        assert_eq!(parse_default_route(none), None);
        assert_eq!(parse_default_route(""), None);
    }

    #[test]
    fn process_stat() {
        let stat = "4242 (Web Content) S 1 4242 4242 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 30 0 123456 2000000000 51200 18446744073709551615";
        assert_eq!(parse_pid_stat(stat), Some(("Web Content".to_owned(), 300, 51200)));
        assert_eq!(parse_pid_stat("garbage"), None);
        assert_eq!(clean_program(".firefox-wrapped"), "firefox");
        assert_eq!(clean_program("rustc"), "rustc");
    }

    #[test]
    fn capped_history() {
        let mut history = Vec::new();
        for i in 0..50 {
            push_capped(&mut history, i);
        }
        assert_eq!(history.len(), HISTORY);
        assert_eq!(history.first(), Some(&10));
        assert_eq!(history.last(), Some(&49));
    }
}
