// SPDX-License-Identifier: GPL-3.0-only

//! The GPUs: which drives the display, whether the discrete one is awake,
//! and what it's doing. All from sysfs and procfs, with care never to wake
//! a GPU that is runtime-suspended just to look at it.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
    time::Duration,
};

use cosmic::iced::Subscription;

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Awake and doing work; numbers where the driver gives them.
    Active { util_percent: Option<u8>, temp_c: Option<f32>, power_w: Option<f32> },
    /// Runtime-suspended (D3cold): nothing was read from it.
    Asleep,
    /// Powered off (e.g. supergfxd Integrated mode) or not present.
    Off,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Process {
    pub pid: u32,
    /// `/proc/<pid>/comm`, or the exe's file name.
    pub name: String,
    /// Bytes.
    pub vram: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gpu {
    /// "Radeon 780M" / "Radeon RX 7700S" when known, else "Integrated GPU" / "Discrete GPU".
    pub name: String,
    pub discrete: bool,
    /// Has a connected connector (status "connected") in /sys/class/drm/cardN-*/status.
    pub drives_display: bool,
    pub status: Status,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    /// System memory the GPU has mapped (GTT): where an APU keeps most of
    /// its data, its own VRAM carve-out being small.
    pub shared_used: Option<u64>,
    pub shared_total: Option<u64>,
    pub processes: Vec<Process>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Gpus {
    pub integrated: Option<Gpu>,
    pub discrete: Option<Gpu>,
}

const VENDOR_AMD: u32 = 0x1002;
const VENDOR_NVIDIA: u32 = 0x10de;
const VENDOR_INTEL: u32 = 0x8086;

/// AMD APU graphics ids, from Raven to Strix Halo.
const AMD_APUS: &[u32] = &[
    0x15dd, 0x15d8, 0x1636, 0x1638, 0x164c, 0x15e7, 0x1681, 0x164e, 0x164d, 0x15bf, 0x15c8, 0x13c0, 0x150e, 0x1114, 0x1900, 0x1901,
];

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// Marketing names by PCI slot, looked up once per card.
static NAMES: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);
/// The last VRAM total seen per PCI slot, shown while the card sleeps.
static VRAM_TOTALS: LazyLock<Mutex<HashMap<String, u64>>> = LazyLock::new(Default::default);

/// What can be known about a card without waking it.
#[derive(Debug)]
struct Card {
    path: PathBuf,
    vendor: u32,
    device: u32,
    class: u32,
    boot_vga: bool,
    slot: String,
    bus: Option<u32>,
    /// Missing when the driver has no runtime PM: treated as off.
    runtime_status: Option<String>,
    has_edp: bool,
}

impl Card {
    fn read(path: PathBuf) -> Option<Self> {
        let device = path.join("device");
        let hex = |name: &str| u32::from_str_radix(read_trimmed(&device.join(name))?.trim_start_matches("0x"), 16).ok();
        let vendor = hex("vendor")?;
        let uevent = read_trimmed(&device.join("uevent")).unwrap_or_default();
        let slot = uevent
            .lines()
            .find_map(|line| line.strip_prefix("PCI_SLOT_NAME="))
            .map(str::to_owned)
            .or_else(|| device.canonicalize().ok()?.file_name().map(|name| name.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let bus = slot.split(':').nth(1).and_then(|bus| u32::from_str_radix(bus, 16).ok());
        let card = path.file_name()?.to_string_lossy().into_owned();
        let has_edp = fs::read_dir(path.parent()?)
            .ok()?
            .filter_map(|entry| entry.ok())
            .any(|entry| entry.file_name().to_string_lossy().starts_with(&format!("{card}-eDP")));
        Some(Card {
            vendor,
            device: hex("device").unwrap_or(0),
            class: hex("class").unwrap_or(0),
            boot_vga: read_trimmed(&device.join("boot_vga")).as_deref() == Some("1"),
            slot,
            bus,
            runtime_status: read_trimmed(&device.join("power/runtime_status")),
            has_edp,
            path,
        })
    }

    fn active(&self) -> bool {
        self.runtime_status.as_deref() == Some("active")
    }

    fn known_vram_total(&self) -> Option<u64> {
        VRAM_TOTALS.lock().ok()?.get(&self.slot).copied()
    }

    /// How much this looks like the integrated GPU: it boots the firmware
    /// console, sits on bus 0, is a known APU or an Intel part, carries
    /// the panel; a discrete one is NVIDIA or has real VRAM.
    fn integrated_score(&self) -> i32 {
        let mut score = 0;
        if self.boot_vga {
            score += 3;
        }
        if self.bus == Some(0) {
            score += 3;
        }
        if self.vendor == VENDOR_AMD && AMD_APUS.contains(&self.device) {
            score += 4;
        }
        if self.vendor == VENDOR_INTEL {
            score += 3;
        }
        // "Display controller" rather than "VGA compatible": how an APU
        // shows up next to a discrete card.
        if self.class >> 8 == 0x0380 {
            score += 1;
        }
        if self.has_edp {
            score += 1;
        }
        if self.vendor == VENDOR_NVIDIA {
            score -= 4;
        }
        if self.known_vram_total().is_some_and(|total| total > 2 * GIB as u64) {
            score -= 2;
        }
        score
    }

    /// Alone in the machine: integrated unless plainly not.
    fn clearly_discrete(&self) -> bool {
        self.vendor == VENDOR_NVIDIA || (self.vendor == VENDOR_AMD && !self.boot_vga && self.known_vram_total().is_some_and(|total| total > 2 * GIB as u64))
    }

    fn drives_display(&self) -> bool {
        let Some(parent) = self.path.parent() else {
            return false;
        };
        let prefix = format!("{}-", self.path.file_name().unwrap_or_default().to_string_lossy());
        fs::read_dir(parent)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
            .any(|entry| read_trimmed(&entry.path().join("status")).as_deref() == Some("connected"))
    }

    /// The marketing name, from pci.ids by the ids sysfs gives (which
    /// touches nothing), or from lspci while the card is awake: lspci
    /// reads config space, which wakes a card in D3cold.
    fn name(&self) -> String {
        let fallback = || if self.clearly_discrete() { "Discrete GPU" } else { "Integrated GPU" };
        if let Ok(mut names) = NAMES.lock() {
            if let Some(name) = names.get(&self.slot) {
                return name.clone();
            }
            let found = pci_ids_name(self.vendor, self.device).or_else(|| if self.active() { lspci_name(&self.slot) } else { None });
            if let Some(name) = found {
                let name = shorten(&name);
                names.insert(self.slot.clone(), name.clone());
                return name;
            }
        }
        fallback().to_owned()
    }

    /// Everything an awake card can say. Only called while it is active:
    /// these files wake a sleeping one.
    fn snapshot(&self, discrete: bool, own_pid: u32) -> Gpu {
        let device = self.path.join("device");
        let name = self.name();
        if self.vendor == VENDOR_NVIDIA {
            return self.nvidia_snapshot(discrete, name);
        }
        let status = if !self.active() {
            if self.runtime_status.is_some() { Status::Asleep } else { Status::Off }
        } else {
            let hwmon = hwmon_dir(&device);
            let hwmon_value = |name: &str| hwmon.as_ref().and_then(|dir| read_number(&dir.join(name)));
            Status::Active {
                util_percent: read_number(&device.join("gpu_busy_percent")).map(|n| n.min(100) as u8),
                temp_c: hwmon_value("temp1_input").map(|n| n as f32 / 1000.0),
                power_w: hwmon_value("power1_average").or_else(|| hwmon_value("power1_input")).map(|n| n as f32 / 1_000_000.0),
            }
        };
        let mut vram_total = None;
        let mut vram_used = None;
        let mut processes = Vec::new();
        if self.active() {
            vram_total = read_number(&device.join("mem_info_vram_total"));
            if let (Some(total), Ok(mut totals)) = (vram_total, VRAM_TOTALS.lock()) {
                totals.insert(self.slot.clone(), total);
            }
            vram_used = read_number(&device.join("mem_info_vram_used"));
            processes = processes_of(&self.slot, own_pid);
        }
        let (shared_used, shared_total) = if self.active() {
            (read_number(&device.join("mem_info_gtt_used")), read_number(&device.join("mem_info_gtt_total")))
        } else {
            (None, None)
        };
        Gpu {
            name: if discrete && name == "Integrated GPU" { "Discrete GPU".to_owned() } else { name },
            discrete,
            drives_display: self.drives_display(),
            status,
            vram_used,
            vram_total: vram_total.or_else(|| self.known_vram_total()),
            shared_used,
            shared_total,
            processes,
        }
    }

    /// An NVIDIA card: its driver keeps no counters in sysfs, so nvidia-smi
    /// says the rest, and only while the card is awake anyway (it would
    /// wake it).
    fn nvidia_snapshot(&self, discrete: bool, name: String) -> Gpu {
        let name = if discrete && name == "Integrated GPU" { "Discrete GPU".to_owned() } else { name };
        let mut gpu = Gpu {
            name,
            discrete,
            drives_display: self.drives_display(),
            status: if self.runtime_status.is_some() { Status::Asleep } else { Status::Off },
            vram_used: None,
            vram_total: self.known_vram_total(),
            shared_used: None,
            shared_total: None,
            processes: Vec::new(),
        };
        if !self.active() {
            nvidia::forget();
            return gpu;
        }
        let reading = nvidia::read();
        gpu.status = Status::Active { util_percent: reading.util, temp_c: reading.temp_c, power_w: reading.power_w };
        if let (Some(total), Ok(mut totals)) = (reading.vram_total, VRAM_TOTALS.lock()) {
            totals.insert(self.slot.clone(), total);
        }
        gpu.vram_total = reading.vram_total.or(gpu.vram_total);
        gpu.vram_used = reading.vram_used;
        gpu.processes = reading.processes;
        gpu
    }
}

/// NVIDIA's numbers, through nvidia-smi: its proprietary driver keeps them
/// out of sysfs. nvidia-smi itself touches the card, so a card that has gone
/// quiet (nothing rendering on it, no load) is left alone after a few
/// readings and can fall back asleep; it is read again once it has slept
/// and woken.
mod nvidia {
    use std::sync::Mutex;

    use super::Process;

    /// Quiet readings in a row before nvidia-smi is left alone.
    const QUIET_LIMIT: u32 = 3;

    #[derive(Clone, Debug, Default)]
    pub struct Reading {
        pub util: Option<u8>,
        pub temp_c: Option<f32>,
        pub power_w: Option<f32>,
        pub vram_used: Option<u64>,
        pub vram_total: Option<u64>,
        pub processes: Vec<Process>,
    }

    struct State {
        quiet: u32,
        last: Option<Reading>,
    }

    static STATE: Mutex<State> = Mutex::new(State { quiet: 0, last: None });

    /// The card slept: read it afresh when it wakes.
    pub fn forget() {
        if let Ok(mut state) = STATE.lock() {
            state.quiet = 0;
        }
    }

    pub fn read() -> Reading {
        let Ok(mut state) = STATE.lock() else {
            return Reading::default();
        };
        if state.quiet >= QUIET_LIMIT {
            // Gone quiet: what it last said stands (idle, a few watts) until
            // it has slept and woken.
            return state.last.clone().unwrap_or_default();
        }
        let mut reading = query();
        reading.processes = processes();
        let busy = reading.util.is_some_and(|util| util > 0) || !reading.processes.is_empty();
        state.quiet = if busy { 0 } else { state.quiet + 1 };
        state.last = Some(reading.clone());
        reading
    }

    fn nvidia_smi(args: &[&str]) -> Option<String> {
        let output = std::process::Command::new("nvidia-smi").args(args).env_remove("LD_LIBRARY_PATH").output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn query() -> Reading {
        let Some(out) = nvidia_smi(&["--query-gpu=utilization.gpu,temperature.gpu,power.draw,memory.used,memory.total", "--format=csv,noheader,nounits"]) else {
            return Reading::default();
        };
        parse_query(out.lines().next().unwrap_or(""))
    }

    /// One line of nvidia-smi's CSV; "[N/A]" and "[Not Supported]" are none.
    pub fn parse_query(line: &str) -> Reading {
        let fields: Vec<&str> = line.split(',').collect();
        let get = |i: usize| fields.get(i).and_then(|v| v.trim().parse::<f64>().ok());
        const MIB: f64 = 1024.0 * 1024.0;
        Reading {
            util: get(0).map(|v| v.clamp(0.0, 100.0) as u8),
            temp_c: get(1).map(|v| v as f32),
            power_w: get(2).map(|v| v as f32),
            vram_used: get(3).map(|v| (v * MIB) as u64),
            vram_total: get(4).map(|v| (v * MIB) as u64),
            processes: Vec::new(),
        }
    }

    /// The processes with a context on the card, graphics and compute, by
    /// the memory they hold, named for the app they are.
    fn processes() -> Vec<Process> {
        let Some(out) = nvidia_smi(&["-q", "-d", "PIDS"]) else {
            return Vec::new();
        };
        let own = std::process::id();
        let mut list: Vec<Process> = parse_processes(&out).into_iter().filter(|p| p.pid != own).collect();
        for process in &mut list {
            let named = crate::backend::privacy::process_name(process.pid);
            if !named.is_empty() && named != "An app" {
                process.name = named;
            }
        }
        list.sort_by(|a, b| b.vram.cmp(&a.vram).then(a.pid.cmp(&b.pid)));
        list.truncate(8);
        list
    }

    /// `nvidia-smi -q -d PIDS`: a "Process ID" line opens each entry, its
    /// "Name" and "Used GPU Memory" lines follow.
    pub fn parse_processes(text: &str) -> Vec<Process> {
        let mut list: Vec<Process> = Vec::new();
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "Process ID" => {
                    if let Ok(pid) = value.parse() {
                        list.push(Process { pid, name: String::new(), vram: 0 });
                    }
                }
                "Name" => {
                    if let Some(last) = list.last_mut() {
                        last.name = value.rsplit('/').next().unwrap_or(value).to_owned();
                    }
                }
                "Used GPU Memory" => {
                    if let Some(last) = list.last_mut() {
                        let mib: u64 = value.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
                        last.vram = mib * 1024 * 1024;
                    }
                }
                _ => {}
            }
        }
        // A process once per context: merged, its memory summed.
        let mut merged: Vec<Process> = Vec::new();
        for process in list {
            match merged.iter_mut().find(|p| p.pid == process.pid) {
                Some(known) => known.vram += process.vram,
                None => merged.push(process),
            }
        }
        merged
    }
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_owned())
}

fn read_number(path: &Path) -> Option<u64> {
    read_trimmed(path)?.parse().ok()
}

fn hwmon_dir(device: &Path) -> Option<PathBuf> {
    fs::read_dir(device.join("hwmon")).ok()?.filter_map(|entry| entry.ok()).map(|entry| entry.path()).find(|path| path.join("temp1_input").exists() || path.join("power1_average").exists() || path.join("power1_input").exists())
}

/// Where a pci.ids database may be: next to the lspci on PATH (how
/// pciutils is packaged on Nix), or where distributions put hwdata.
fn pci_ids_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for dir in std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect::<Vec<_>>()).unwrap_or_default() {
        let lspci = dir.join("lspci");
        if lspci.exists() {
            let real = fs::canonicalize(&lspci).unwrap_or(lspci);
            if let Some(prefix) = real.parent().and_then(Path::parent) {
                paths.push(prefix.join("share/pci.ids"));
                paths.push(prefix.join("share/hwdata/pci.ids"));
            }
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    paths.extend(
        [
            "/usr/share/hwdata/pci.ids".to_owned(),
            "/usr/share/misc/pci.ids".to_owned(),
            "/run/current-system/sw/share/hwdata/pci.ids".to_owned(),
            "/run/current-system/sw/share/pci.ids".to_owned(),
            format!("{home}/.nix-profile/share/pci.ids"),
            format!("{home}/.nix-profile/share/hwdata/pci.ids"),
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    paths
}

fn pci_ids_name(vendor: u32, device: u32) -> Option<String> {
    pci_ids_paths().into_iter().find_map(|path| fs::read_to_string(path).ok()).and_then(|text| pci_ids_lookup(&text, vendor, device))
}

/// The device line under the vendor in a pci.ids database: vendors at
/// the margin, their devices one tab in, subsystems two.
fn pci_ids_lookup(text: &str, vendor: u32, device: u32) -> Option<String> {
    let vendor = format!("{vendor:04x}");
    let device = format!("{device:04x}");
    let mut in_vendor = false;
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with('\t') {
            in_vendor = line.strip_prefix(vendor.as_str()).is_some_and(|rest| rest.starts_with(|c: char| c.is_whitespace()));
            continue;
        }
        if !in_vendor || line.starts_with("\t\t") {
            continue;
        }
        if let Some(name) = line[1..].strip_prefix(device.as_str())
            && name.starts_with(|c: char| c.is_whitespace())
        {
            return Some(name.trim().to_owned());
        }
    }
    None
}

/// `lspci -mm` on one slot. Only for an awake card: see `Card::name`.
fn lspci_name(slot: &str) -> Option<String> {
    let output = std::process::Command::new("lspci").args(["-mm", "-s", slot]).output().ok().filter(|output| output.status.success())?;
    lspci_device_field(&String::from_utf8_lossy(&output.stdout))
}

/// The device name: the third quoted field of an `lspci -mm` line.
fn lspci_device_field(line: &str) -> Option<String> {
    let mut fields = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let end = after.find('"')?;
        fields.push(&after[..end]);
        rest = &after[end + 1..];
    }
    fields.get(2).map(|s| s.to_string())
}

/// The marketing name out of an lspci device string: the bracketed part
/// when there is one (`Phoenix1 [Radeon 780M]`), the first of several
/// alternatives (`Navi 33 [Radeon RX 7700S/7600S / 7600M XT]`), and no
/// parenthetical asides.
fn shorten(device: &str) -> String {
    let mut name = device;
    if let (Some(open), Some(close)) = (device.find('['), device.rfind(']'))
        && open < close
    {
        name = &device[open + 1..close];
    }
    let name = name.split('/').next().unwrap_or(name);
    let name = match name.find('(') {
        Some(paren) => &name[..paren],
        None => name,
    };
    let name = name.trim();
    if name.is_empty() { device.trim().to_owned() } else { name.to_owned() }
}

/// One DRM client as its fdinfo describes it.
#[derive(Debug, PartialEq)]
struct FdInfo {
    pdev: String,
    client_id: Option<String>,
    vram: u64,
}

/// The DRM fields of one fdinfo file, when it describes a DRM client
/// with a PCI device.
fn parse_fdinfo(text: &str) -> Option<FdInfo> {
    let mut pdev = None;
    let mut client_id = None;
    let mut memory_vram = None;
    let mut total_vram = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "drm-pdev" => pdev = Some(value.to_owned()),
            "drm-client-id" => client_id = Some(value.to_owned()),
            "drm-memory-vram" => memory_vram = parse_size(value),
            "drm-total-vram" => total_vram = parse_size(value),
            _ => {}
        }
    }
    Some(FdInfo { pdev: pdev?, client_id, vram: memory_vram.or(total_vram).unwrap_or(0) })
}

/// A size with a unit as fdinfo writes it: `45744 KiB`, `12 MiB`, `300`.
fn parse_size(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let number: u64 = parts.next()?.parse().ok()?;
    let scale = match parts.next() {
        None => 1,
        Some("KiB") => 1024,
        Some("MiB") => 1024 * 1024,
        Some("GiB") => 1024 * 1024 * 1024,
        Some(_) => return None,
    };
    Some(number * scale)
}

/// Processes with clients on the card at `slot`, from fdinfo texts tagged
/// with their pid: each client counted once, summed per process, biggest
/// first, at most eight, none without VRAM.
fn collect_processes<'a>(slot: &str, entries: impl IntoIterator<Item = (u32, &'a str)>, name: impl Fn(u32) -> String) -> Vec<Process> {
    let mut seen_clients = HashSet::new();
    let mut vram_by_pid: HashMap<u32, u64> = HashMap::new();
    let mut order = Vec::new();
    for (pid, text) in entries {
        let Some(info) = parse_fdinfo(text) else {
            continue;
        };
        if info.pdev != slot {
            continue;
        }
        if let Some(client) = info.client_id
            && !seen_clients.insert(client)
        {
            continue;
        }
        if !vram_by_pid.contains_key(&pid) {
            order.push(pid);
        }
        *vram_by_pid.entry(pid).or_default() += info.vram;
    }
    let mut processes: Vec<Process> = order
        .into_iter()
        .filter_map(|pid| {
            let vram = vram_by_pid[&pid];
            (vram > 0).then(|| Process { pid, name: name(pid), vram })
        })
        .collect();
    processes.sort_by(|a, b| b.vram.cmp(&a.vram).then(a.pid.cmp(&b.pid)));
    processes.truncate(8);
    processes
}

/// Processes using the card at `slot`, read from the fdinfo of every fd
/// that is a DRM device node.
fn processes_of(slot: &str, own_pid: u32) -> Vec<Process> {
    let mut entries: Vec<(u32, String)> = Vec::new();
    for pid in fs::read_dir("/proc").ok().into_iter().flatten().filter_map(|entry| entry.ok()).filter_map(|entry| entry.file_name().to_string_lossy().parse::<u32>().ok()) {
        if pid == own_pid {
            continue;
        }
        let proc_dir = PathBuf::from(format!("/proc/{pid}"));
        for fd in fs::read_dir(proc_dir.join("fd")).ok().into_iter().flatten().filter_map(|entry| entry.ok()) {
            let is_drm = fs::read_link(fd.path()).is_ok_and(|target| target.starts_with("/dev/dri/"));
            if !is_drm {
                continue;
            }
            if let Ok(text) = fs::read_to_string(proc_dir.join("fdinfo").join(fd.file_name())) {
                entries.push((pid, text));
            }
        }
    }
    collect_processes(slot, entries.iter().map(|(pid, text)| (*pid, text.as_str())), process_name)
}

/// The process's comm, or its exe's file name when comm is a Nix
/// wrapper's (`.cosmic-comp-wr`, cut at 15 bytes): the exe's name
/// (`.cosmic-comp-wrapped`) is whole, and loses its dressing.
fn process_name(pid: u32) -> String {
    let exe = || fs::read_link(format!("/proc/{pid}/exe")).ok()?.file_name().map(|name| name.to_string_lossy().into_owned());
    let comm = read_trimmed(&PathBuf::from(format!("/proc/{pid}/comm"))).filter(|name| !name.is_empty());
    let name = match comm {
        Some(comm) if comm.starts_with('.') => exe().unwrap_or(comm),
        Some(comm) => comm,
        None => exe().unwrap_or_else(|| pid.to_string()),
    };
    unwrap_name(&name).to_owned()
}

/// `.spotify-wrapped` → `spotify`.
fn unwrap_name(name: &str) -> &str {
    let unwrapped = name.strip_prefix('.').and_then(|rest| rest.strip_suffix("-wrapped")).unwrap_or(name);
    if unwrapped.is_empty() { name } else { unwrapped }
}

fn cards() -> Vec<Card> {
    let mut cards: Vec<Card> = fs::read_dir("/sys/class/drm")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.strip_prefix("card").is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
        })
        .filter_map(|entry| Card::read(entry.path()))
        .collect();
    cards.sort_by(|a, b| a.path.cmp(&b.path));
    cards
}

fn read_all() -> Option<Gpus> {
    let mut cards = cards();
    if cards.is_empty() {
        return None;
    }
    let own_pid = std::process::id();
    if cards.len() == 1 {
        let card = cards.remove(0);
        let discrete = card.clearly_discrete();
        let gpu = card.snapshot(discrete, own_pid);
        return Some(if discrete { Gpus { integrated: None, discrete: Some(gpu) } } else { Gpus { integrated: Some(gpu), discrete: None } });
    }
    // The most integrated-looking card is the integrated one; of the rest,
    // the one that looks least so is the discrete one.
    let (integrated_at, _) = cards.iter().enumerate().max_by_key(|(i, card)| (card.integrated_score(), -(*i as i32)))?;
    let integrated = cards.remove(integrated_at);
    let (discrete_at, _) = cards.iter().enumerate().min_by_key(|(i, card)| (card.integrated_score(), *i))?;
    let discrete = cards.remove(discrete_at);
    Some(Gpus { integrated: Some(integrated.snapshot(false, own_pid)), discrete: Some(discrete.snapshot(true, own_pid)) })
}

/// `None` without any DRM card. A discrete card that is not enumerated
/// (powered off by supergfxd) leaves `discrete` empty.
pub async fn now() -> Option<Gpus> {
    tokio::task::spawn_blocking(read_all).await.ok().flatten()
}

pub fn subscription() -> Subscription<Gpus> {
    Subscription::run(|| super::watch(Duration::from_secs(2), super::no_changes, now))
}

/// Bytes as "2.1 GB", or "8 GB" when whole.
pub fn gigabytes(bytes: u64) -> String {
    let gb = bytes as f64 / GIB;
    let rounded = (gb * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 { format!("{} GB", rounded as u64) } else { format!("{rounded:.1} GB") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_lspci_names() {
        assert_eq!(shorten("Phoenix1 [Radeon 780M]"), "Radeon 780M");
        assert_eq!(shorten("Navi 33 [Radeon RX 7700S/7600S / 7600M XT]"), "Radeon RX 7700S");
        assert_eq!(shorten("Strix [Radeon 880M / 890M]"), "Radeon 880M");
        assert_eq!(shorten("GB206M [GeForce RTX 5070 Max-Q / Mobile]"), "GeForce RTX 5070 Max-Q");
        assert_eq!(shorten("Alder Lake-P GT2 [Iris Xe Graphics]"), "Iris Xe Graphics");
        assert_eq!(shorten("GA104M [GeForce RTX 3080 Mobile / Max-Q 8GB/16GB]"), "GeForce RTX 3080 Mobile");
        assert_eq!(shorten("Device 7480 (rev c7)"), "Device 7480");
        assert_eq!(shorten("  Plain Name  "), "Plain Name");
    }

    #[test]
    fn picks_lspci_device_field() {
        let line = r#"c3:00.0 "Display controller" "Advanced Micro Devices, Inc. [AMD/ATI]" "Strix [Radeon 880M / 890M]" -rc1 -p00 "Framework Computer Inc." "Device 000d""#;
        assert_eq!(lspci_device_field(line).as_deref(), Some("Strix [Radeon 880M / 890M]"));
        assert_eq!(lspci_device_field(""), None);
    }

    const PCI_IDS: &str = "# comment\n1002  Advanced Micro Devices, Inc. [AMD/ATI]\n\t150e  Strix [Radeon 880M / 890M]\n\t\tf111 000d  Framework Laptop 13\n\t15bf  Phoenix1 [Radeon 780M]\n10de  NVIDIA Corporation\n\t2d58  GB206M [GeForce RTX 5070 Max-Q / Mobile]\n\t150e  Not the AMD one\n";

    #[test]
    fn looks_up_pci_ids() {
        assert_eq!(pci_ids_lookup(PCI_IDS, 0x1002, 0x150e).as_deref(), Some("Strix [Radeon 880M / 890M]"));
        assert_eq!(pci_ids_lookup(PCI_IDS, 0x1002, 0x15bf).as_deref(), Some("Phoenix1 [Radeon 780M]"));
        assert_eq!(pci_ids_lookup(PCI_IDS, 0x10de, 0x2d58).as_deref(), Some("GB206M [GeForce RTX 5070 Max-Q / Mobile]"));
        assert_eq!(pci_ids_lookup(PCI_IDS, 0x1002, 0x000d), None);
        assert_eq!(pci_ids_lookup(PCI_IDS, 0x8086, 0x150e), None);
    }

    const AMDGPU_FDINFO: &str = "pos:\t0\nflags:\t02100002\nmnt_id:\t41\nino:\t1003\ndrm-driver:\tamdgpu\ndrm-client-id:\t1374\ndrm-pdev:\t0000:c3:00.0\npasid:\t937\ndrm-total-gtt:\t12300 KiB\ndrm-total-vram:\t45744 KiB\ndrm-shared-vram:\t5200 KiB\n";

    #[test]
    fn parses_fdinfo() {
        assert_eq!(parse_fdinfo(AMDGPU_FDINFO), Some(FdInfo { pdev: "0000:c3:00.0".to_owned(), client_id: Some("1374".to_owned()), vram: 45744 * 1024 }));
        // Older amdgpu: drm-memory-vram, which wins over drm-total-vram.
        let old = "drm-driver:\tamdgpu\ndrm-client-id:\t7\ndrm-pdev:\t0000:03:00.0\ndrm-memory-vram:\t2048 KiB\ndrm-total-vram:\t1 KiB\n";
        assert_eq!(parse_fdinfo(old).map(|info| info.vram), Some(2 * 1024 * 1024));
        // A space after the colon instead of a tab, and megabytes.
        let spaced = "drm-driver: nvidia-drm\ndrm-pdev: 0000:c2:00.0\ndrm-memory-vram: 12 MiB\n";
        assert_eq!(parse_fdinfo(spaced), Some(FdInfo { pdev: "0000:c2:00.0".to_owned(), client_id: None, vram: 12 * 1024 * 1024 }));
        // Not a DRM client at all.
        assert_eq!(parse_fdinfo("pos:\t0\nflags:\t02\n"), None);
    }

    #[test]
    fn reads_nvidia_smi() {
        let reading = nvidia::parse_query("54, 57, 32.71, 2150, 8151");
        assert_eq!(reading.util, Some(54));
        assert_eq!(reading.power_w, Some(32.71));
        assert_eq!(reading.vram_used, Some(2150 * 1024 * 1024));
        assert_eq!(nvidia::parse_query("0, 41, [N/A], 5, 8151").power_w, None);
        let text = "GPU 00000000:C2:00.0\n    Processes\n        Process ID                        : 4242\n            Type                          : G\n            Name                          : /nix/store/abc-steam/bin/steam\n            Used GPU Memory               : 1600 MiB\n        Process ID                        : 77\n            Type                          : C+G\n            Name                          : chromium\n            Used GPU Memory               : 500 MiB\n";
        let processes = nvidia::parse_processes(text);
        assert_eq!(processes.len(), 2);
        assert_eq!(processes[0].name, "steam");
        assert_eq!(processes[0].vram, 1600 * 1024 * 1024);
        assert!(nvidia::parse_processes("    Processes                             : None\n").is_empty());
    }

    #[test]
    fn collects_processes_by_vram() {
        let other_card = "drm-client-id:\t9\ndrm-pdev:\t0000:c2:00.0\ndrm-total-vram:\t999999 KiB\n";
        let repeat = AMDGPU_FDINFO; // same client id 1374, a dup'd fd
        let second = "drm-client-id:\t2000\ndrm-pdev:\t0000:c3:00.0\ndrm-total-vram:\t1024 KiB\n";
        let third = "drm-client-id:\t3000\ndrm-pdev:\t0000:c3:00.0\ndrm-total-vram:\t0\n";
        let entries = [(10, AMDGPU_FDINFO), (10, repeat), (10, second), (20, other_card), (30, third), (40, second /* same client id as pid 10's: already counted */)];
        let processes = collect_processes("0000:c3:00.0", entries, |pid| format!("proc{pid}"));
        assert_eq!(processes, vec![Process { pid: 10, name: "proc10".to_owned(), vram: (45744 + 1024) * 1024 }]);
    }

    #[test]
    fn keeps_eight_biggest() {
        let texts: Vec<String> = (1..=12).map(|i| format!("drm-client-id:\t{i}\ndrm-pdev:\t0000:01:00.0\ndrm-total-vram:\t{} KiB\n", i * 1024)).collect();
        let entries = texts.iter().enumerate().map(|(i, text)| (i as u32 + 1, text.as_str()));
        let processes = collect_processes("0000:01:00.0", entries, |pid| pid.to_string());
        assert_eq!(processes.len(), 8);
        assert_eq!(processes[0].pid, 12);
        assert_eq!(processes[7].pid, 5);
    }

    #[test]
    fn unwraps_nix_names() {
        assert_eq!(unwrap_name(".spotify-wrapped"), "spotify");
        assert_eq!(unwrap_name(".cosmic-app-library-wrapped"), "cosmic-app-library");
        assert_eq!(unwrap_name("chromium"), "chromium");
        assert_eq!(unwrap_name(".hidden"), ".hidden");
        assert_eq!(unwrap_name(".-wrapped"), ".-wrapped");
    }

    #[test]
    fn formats_gigabytes() {
        assert_eq!(gigabytes(8 * 1024 * 1024 * 1024), "8 GB");
        assert_eq!(gigabytes(512 * 1024 * 1024), "0.5 GB");
        assert_eq!(gigabytes((2.1 * GIB) as u64), "2.1 GB");
        assert_eq!(gigabytes(0), "0 GB");
    }
}
