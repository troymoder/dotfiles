// SPDX-License-Identifier: GPL-3.0-only

//! The cameras: every capture device, who holds it, and its image controls,
//! and a small, slow preview of one. Read from sysfs and straight from V4L2
//! through its ioctls. Describing a camera never streams a frame: opening a
//! node to ask it questions doesn't count as using the camera, and each
//! node is closed again right away. Only the preview holds one open.

use std::{
    collections::HashMap,
    fs::{self, File},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use cosmic::iced::Subscription;
use futures::{Stream, channel::mpsc};

use super::latest::Latest;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bus {
    Builtin,
    Usb,
    Virtual,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Integer {
        min: i32,
        max: i32,
        step: i32,
        default: i32,
    },
    Boolean {
        default: bool,
    },
    /// Option `i` is the name of value `i`; it's empty where the driver
    /// doesn't offer that value, which happens (auto exposure has holes).
    Menu {
        options: Vec<String>,
        default: i32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub id: u32,
    /// V4L2's name, such as "Brightness" or "White Balance, Automatic".
    pub name: String,
    pub kind: Kind,
    pub value: i32,
    /// Not in effect now: exposure while auto exposure is on, say.
    pub inactive: bool,
    /// Only to be looked at: the hardware privacy switch, say.
    pub read_only: bool,
    /// A Tabler icon name.
    pub icon: &'static str,
}

/// A process holding a node of the camera open.
#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub pid: u32,
    /// The process's `comm`, or its executable's file name.
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    /// A stable key: the sysfs path of the device behind the nodes.
    pub id: String,
    /// The card name, tidied: "Laptop Camera: Laptop Camera" is one name.
    pub name: String,
    /// The first capture node of the device, like "/dev/video0".
    pub node: String,
    /// "uvcvideo", "v4l2loopback" and such.
    pub driver: String,
    pub bus: Bus,
    /// The current capture size.
    pub resolution: Option<(u32, u32)>,
    /// The sizes the device offers in the pixel format the preview uses,
    /// smallest first.
    pub frame_sizes: Vec<(u32, u32)>,
    pub users: Vec<User>,
    pub controls: Vec<Control>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Cameras {
    pub list: Vec<Camera>,
    /// A built-in camera was seen before on this machine and isn't there
    /// now: the hardware privacy switch.
    pub privacy_switch: bool,
}

/// One picture from the camera, ready to show.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
}

// What follows mirrors linux/videodev2.h for x86_64.

const CAP_VIDEO_CAPTURE: u32 = 0x1;
const CAP_META_CAPTURE: u32 = 0x0080_0000;
const CAP_DEVICE_CAPS: u32 = 0x8000_0000;
const BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
const MEMORY_MMAP: u32 = 1;
const BUF_FLAG_ERROR: u32 = 0x40;
const PIX_FMT_MJPEG: u32 = u32::from_le_bytes(*b"MJPG");
const PIX_FMT_YUYV: u32 = u32::from_le_bytes(*b"YUYV");
const FRMSIZE_TYPE_DISCRETE: u32 = 1;

/// Sizes offered for a camera that takes any size within a range.
const COMMON_SIZES: [(u32, u32); 4] = [(320, 180), (640, 360), (1280, 720), (1920, 1080)];

/// Where the words of `v4l2_pix_format` sit in the format union.
const PIX_WIDTH: usize = 0;
const PIX_HEIGHT: usize = 4;
const PIX_PIXELFORMAT: usize = 8;
const PIX_BYTESPERLINE: usize = 16;

/// The picture asked of the camera for the preview, the widest one sent on,
/// how often one is sent, how many buffers the driver gets, how long a
/// frame may take before the camera is given up on, and how long to wait
/// before trying a camera that can't be streamed again.
const PREVIEW_WIDTH: u32 = 320;
const PREVIEW_HEIGHT: u32 = 180;
const PREVIEW_MAX_WIDTH: u32 = 832;
const PREVIEW_INTERVAL: Duration = Duration::from_millis(66);
const PREVIEW_BUFFERS: u32 = 3;
const FRAME_TIMEOUT_MS: i32 = 2000;
const PREVIEW_RETRY: Duration = Duration::from_secs(2);

const CTRL_TYPE_INTEGER: u32 = 1;
const CTRL_TYPE_BOOLEAN: u32 = 2;
const CTRL_TYPE_MENU: u32 = 3;
const CTRL_TYPE_INTEGER_MENU: u32 = 9;

const CTRL_FLAG_DISABLED: u32 = 0x1;
const CTRL_FLAG_READ_ONLY: u32 = 0x4;
const CTRL_FLAG_INACTIVE: u32 = 0x10;
const CTRL_FLAG_NEXT_CTRL: u32 = 0x8000_0000;

/// Where the user and the camera control classes start, for drivers that
/// can't walk the controls with `NEXT_CTRL`.
const CID_USER_BASE: u32 = 0x0098_0900;
const CID_CAMERA_BASE: u32 = 0x009a_0900;

/// At most this many entries of a menu are asked for.
const MENU_LIMIT: i32 = 64;

#[repr(C)]
struct Capability {
    driver: [u8; 16],
    card: [u8; 32],
    bus_info: [u8; 32],
    version: u32,
    capabilities: u32,
    device_caps: u32,
    reserved: [u32; 3],
}

#[repr(C)]
struct QueryCtrl {
    id: u32,
    kind: u32,
    name: [u8; 32],
    minimum: i32,
    maximum: i32,
    step: i32,
    default_value: i32,
    flags: u32,
    reserved: [u32; 2],
}

/// Packed in the kernel too: the name shares its bytes with an `i64` value
/// for integer menus, and the struct has no padding after it.
#[repr(C, packed)]
struct QueryMenu {
    id: u32,
    index: u32,
    name: [u8; 32],
    reserved: u32,
}

#[repr(C)]
struct ControlValue {
    id: u32,
    value: i32,
}

/// `v4l2_format`: the type, then a union of formats which holds a pointer
/// in one arm and so is aligned to 8. Only the pixel format's width and
/// height, its first two words, are read.
#[repr(C)]
struct Format {
    kind: u32,
    fmt: FormatUnion,
}

#[repr(C, align(8))]
struct FormatUnion([u8; 200]);

impl Format {
    fn word(&self, at: usize) -> u32 {
        u32::from_ne_bytes(self.fmt.0[at..at + 4].try_into().unwrap_or_default())
    }

    fn set_word(&mut self, at: usize, value: u32) {
        self.fmt.0[at..at + 4].copy_from_slice(&value.to_ne_bytes());
    }
}

#[repr(C)]
struct FmtDesc {
    index: u32,
    kind: u32,
    flags: u32,
    description: [u8; 32],
    pixelformat: u32,
    mbus_code: u32,
    reserved: [u32; 3],
}

#[repr(C)]
struct RequestBuffers {
    count: u32,
    kind: u32,
    memory: u32,
    capabilities: u32,
    flags: u8,
    reserved: [u8; 3],
}

/// `v4l2_frmsizeenum`: the union of a discrete size (width, height) and a
/// stepwise range (min, max and step of the width, then of the height) is
/// six words here.
#[repr(C)]
struct FrameSizeEnum {
    index: u32,
    pixel_format: u32,
    kind: u32,
    size: [u32; 6],
    reserved: [u32; 2],
}

/// `v4l2_buffer`: its memory union is one `u64` here, with the mmap offset
/// in its first word.
#[repr(C)]
struct Buffer {
    index: u32,
    kind: u32,
    bytesused: u32,
    flags: u32,
    field: u32,
    timestamp: libc::timeval,
    timecode: [u8; 16],
    sequence: u32,
    memory: u32,
    m: u64,
    length: u32,
    reserved2: u32,
    request_fd: u32,
}

/// All-zero bytes are a valid value of these: they're integers and byte
/// arrays only.
trait Plain: Sized {
    fn zeroed() -> Self {
        // SAFETY: implemented only for repr(C) structs of integers and byte
        // arrays, for which the all-zero bit pattern is valid.
        unsafe { std::mem::zeroed() }
    }
}

impl Plain for Capability {}
impl Plain for QueryCtrl {}
impl Plain for QueryMenu {}
impl Plain for ControlValue {}
impl Plain for Format {}
impl Plain for FmtDesc {}
impl Plain for RequestBuffers {}
impl Plain for FrameSizeEnum {}
impl Plain for Buffer {}
impl Plain for u32 {}

const IOC_WRITE: u32 = 1;
const IOC_READ: u32 = 2;

/// `_IOC(dir, 'V', nr, size)`.
const fn vidioc(dir: u32, nr: u32, size: usize) -> libc::c_ulong {
    ((dir << 30) | ((size as u32) << 16) | ((b'V' as u32) << 8) | nr) as libc::c_ulong
}

const VIDIOC_QUERYCAP: libc::c_ulong = vidioc(IOC_READ, 0, size_of::<Capability>());
const VIDIOC_ENUM_FMT: libc::c_ulong = vidioc(IOC_READ | IOC_WRITE, 2, size_of::<FmtDesc>());
const VIDIOC_G_FMT: libc::c_ulong = vidioc(IOC_READ | IOC_WRITE, 4, size_of::<Format>());
const VIDIOC_S_FMT: libc::c_ulong = vidioc(IOC_READ | IOC_WRITE, 5, size_of::<Format>());
const VIDIOC_REQBUFS: libc::c_ulong = vidioc(IOC_READ | IOC_WRITE, 8, size_of::<RequestBuffers>());
const VIDIOC_QUERYBUF: libc::c_ulong = vidioc(IOC_READ | IOC_WRITE, 9, size_of::<Buffer>());
const VIDIOC_QBUF: libc::c_ulong = vidioc(IOC_READ | IOC_WRITE, 15, size_of::<Buffer>());
const VIDIOC_DQBUF: libc::c_ulong = vidioc(IOC_READ | IOC_WRITE, 17, size_of::<Buffer>());
const VIDIOC_STREAMON: libc::c_ulong = vidioc(IOC_WRITE, 18, size_of::<u32>());
const VIDIOC_STREAMOFF: libc::c_ulong = vidioc(IOC_WRITE, 19, size_of::<u32>());
const VIDIOC_G_CTRL: libc::c_ulong = vidioc(IOC_READ | IOC_WRITE, 0x1b, size_of::<ControlValue>());
const VIDIOC_S_CTRL: libc::c_ulong = vidioc(IOC_READ | IOC_WRITE, 0x1c, size_of::<ControlValue>());
const VIDIOC_QUERYCTRL: libc::c_ulong = vidioc(IOC_READ | IOC_WRITE, 0x24, size_of::<QueryCtrl>());
const VIDIOC_QUERYMENU: libc::c_ulong = vidioc(IOC_READ | IOC_WRITE, 0x25, size_of::<QueryMenu>());
const VIDIOC_ENUM_FRAMESIZES: libc::c_ulong =
    vidioc(IOC_READ | IOC_WRITE, 0x4a, size_of::<FrameSizeEnum>());

/// One ioctl on an open node, reading and writing `arg`.
fn ioctl<T: Plain>(node: &File, request: libc::c_ulong, arg: &mut T) -> std::io::Result<()> {
    // SAFETY: `arg` is a live, exclusively borrowed struct whose size is
    // encoded in `request`, so the kernel reads and writes within it, and
    // the fd is open for as long as `node` is.
    let rc = unsafe { libc::ioctl(node.as_raw_fd(), request, arg as *mut T) };
    if rc < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// A node, opened to be asked questions: non-blocking, so a busy camera
/// still answers, and closed when dropped.
fn open(node: &str) -> std::io::Result<File> {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(node)
}

/// A NUL-terminated field as text.
fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_owned()
}

/// UVC names its card "product: interface", and both are the product's name
/// cut to fit: "Integrated Camera: Integrated C". One copy is enough; a
/// second part that says something else stays.
fn tidy(name: &str) -> String {
    let name = name.trim().trim_end_matches(':').trim();
    let Some((head, tail)) = name.split_once(':') else {
        return name.to_owned();
    };
    let (head, tail) = (head.trim(), tail.trim());
    if tail.is_empty() || head.starts_with(tail) || tail.starts_with(head) {
        head.to_owned()
    } else {
        name.to_owned()
    }
}

pub fn icon_for(name: &str) -> &'static str {
    let name = name.to_lowercase();
    let has = |word: &str| name.contains(word);
    if has("exposure") {
        "exposure"
    } else if has("white balance") || has("temperature") {
        "temperature"
    } else if has("backlight") {
        "brightness-half"
    } else if has("brightness") {
        "sun"
    } else if has("contrast") {
        "contrast"
    } else if has("sharpness") || has("focus") || has("zoom") {
        "focus-2"
    } else if has("power line") {
        "bolt"
    } else {
        "adjustments"
    }
}

/// The capabilities of one node, if it's a V4L2 device at all.
fn capability(node: &File) -> Option<Capability> {
    let mut cap = Capability::zeroed();
    ioctl(node, VIDIOC_QUERYCAP, &mut cap).ok()?;
    Some(cap)
}

/// Can capture video, rather than only metadata about it.
fn captures(cap: &Capability) -> bool {
    let caps = if cap.capabilities & CAP_DEVICE_CAPS != 0 {
        cap.device_caps
    } else {
        cap.capabilities
    };
    caps & CAP_VIDEO_CAPTURE != 0 && caps != CAP_META_CAPTURE
}

fn resolution(node: &File) -> Option<(u32, u32)> {
    let mut format = Format::zeroed();
    format.kind = BUF_TYPE_VIDEO_CAPTURE;
    ioctl(node, VIDIOC_G_FMT, &mut format).ok()?;
    match (format.word(PIX_WIDTH), format.word(PIX_HEIGHT)) {
        (0, _) | (_, 0) => None,
        size => Some(size),
    }
}

/// Every control the driver has, enabled or not: with `NEXT_CTRL` where the
/// driver walks them, else by trying the user and the camera classes.
fn query(node: &File) -> Vec<QueryCtrl> {
    let mut found = Vec::new();
    let mut ctrl = QueryCtrl::zeroed();
    ctrl.id = CTRL_FLAG_NEXT_CTRL;
    if ioctl(node, VIDIOC_QUERYCTRL, &mut ctrl).is_err() {
        for id in
            (CID_USER_BASE..CID_USER_BASE + 0x40).chain(CID_CAMERA_BASE..CID_CAMERA_BASE + 0x40)
        {
            let mut ctrl = QueryCtrl::zeroed();
            ctrl.id = id;
            if ioctl(node, VIDIOC_QUERYCTRL, &mut ctrl).is_ok() {
                found.push(ctrl);
            }
        }
        return found;
    }
    loop {
        let mut next = QueryCtrl::zeroed();
        next.id = ctrl.id | CTRL_FLAG_NEXT_CTRL;
        found.push(ctrl);
        if ioctl(node, VIDIOC_QUERYCTRL, &mut next).is_err() {
            return found;
        }
        ctrl = next;
    }
}

/// Of a kind the menu can show and set, and not disabled.
fn settable(ctrl: &QueryCtrl) -> bool {
    ctrl.flags & CTRL_FLAG_DISABLED == 0
        && matches!(
            ctrl.kind,
            CTRL_TYPE_INTEGER | CTRL_TYPE_BOOLEAN | CTRL_TYPE_MENU | CTRL_TYPE_INTEGER_MENU
        )
}

/// The names of a menu's values, 0 through its highest.
fn options(node: &File, ctrl: &QueryCtrl) -> Vec<String> {
    (0..=ctrl.maximum.clamp(0, MENU_LIMIT))
        .map(|index| {
            let mut menu = QueryMenu::zeroed();
            menu.id = ctrl.id;
            menu.index = index as u32;
            if ioctl(node, VIDIOC_QUERYMENU, &mut menu).is_err() {
                return String::new();
            }
            // Copied out: a packed field can't be borrowed.
            let name = menu.name;
            if ctrl.kind == CTRL_TYPE_INTEGER_MENU {
                i64::from_ne_bytes(name[..8].try_into().unwrap_or_default()).to_string()
            } else {
                text(&name)
            }
        })
        .collect()
}

fn value(node: &File, ctrl: &QueryCtrl) -> i32 {
    let mut current = ControlValue {
        id: ctrl.id,
        value: 0,
    };
    if ioctl(node, VIDIOC_G_CTRL, &mut current).is_ok() {
        current.value
    } else {
        ctrl.default_value
    }
}

fn control(node: &File, ctrl: &QueryCtrl) -> Option<Control> {
    if !settable(ctrl) {
        return None;
    }
    let kind = match ctrl.kind {
        CTRL_TYPE_INTEGER => Kind::Integer {
            min: ctrl.minimum,
            max: ctrl.maximum,
            step: ctrl.step.max(1),
            default: ctrl.default_value,
        },
        CTRL_TYPE_BOOLEAN => Kind::Boolean {
            default: ctrl.default_value != 0,
        },
        _ => Kind::Menu {
            options: options(node, ctrl),
            default: ctrl.default_value,
        },
    };
    let name = text(&ctrl.name);
    Some(Control {
        id: ctrl.id,
        icon: icon_for(&name),
        name,
        kind,
        value: value(node, ctrl),
        inactive: ctrl.flags & CTRL_FLAG_INACTIVE != 0,
        read_only: ctrl.flags & CTRL_FLAG_READ_ONLY != 0,
    })
}

/// Where the camera is attached. A USB device says whether it's soldered
/// in through `removable`; when it doesn't, its names decide.
fn bus(driver: &str, device: &Path, name: &str) -> Bus {
    if driver == "v4l2loopback" {
        return Bus::Virtual;
    }
    if !device.to_string_lossy().contains("/usb") {
        return Bus::Builtin;
    }
    let mut names = name.to_owned();
    let mut dir = Some(device);
    while let Some(d) = dir {
        if let Ok(removable) = fs::read_to_string(d.join("removable")) {
            match removable.trim() {
                "fixed" => return Bus::Builtin,
                "removable" => return Bus::Usb,
                _ => {}
            }
        }
        for file in ["product", "manufacturer"] {
            if let Ok(text) = fs::read_to_string(d.join(file)) {
                names.push(' ');
                names.push_str(&text);
            }
        }
        // The root hub: the end of the USB part of the path.
        if d.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("usb"))
        {
            break;
        }
        dir = d.parent();
    }
    let names = names.to_lowercase();
    if ["laptop", "integrated", "internal", "built-in", "framework"]
        .iter()
        .any(|word| names.contains(word))
    {
        Bus::Builtin
    } else {
        Bus::Usb
    }
}

/// Every `/dev/video*` node open in some process, with who holds it.
fn holders() -> Vec<(String, User)> {
    let me = std::process::id();
    let mut found = Vec::new();
    let Ok(procs) = fs::read_dir("/proc") else {
        return found;
    };
    for entry in procs.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == me {
            continue;
        }
        let Ok(fds) = fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        let mut name: Option<String> = None;
        for fd in fds.flatten() {
            let Ok(target) = fs::read_link(fd.path()) else {
                continue;
            };
            let Some(target) = target.to_str().filter(|t| t.starts_with("/dev/video")) else {
                continue;
            };
            let name = name
                .get_or_insert_with(|| super::privacy::process_name(pid))
                .clone();
            found.push((target.to_owned(), User { pid, name }));
        }
    }
    found
}

#[allow(dead_code)]
fn process_name(pid: u32) -> String {
    let proc = PathBuf::from(format!("/proc/{pid}"));
    let comm = fs::read_to_string(proc.join("comm"))
        .map(|c| c.trim().to_owned())
        .unwrap_or_default();
    if !comm.is_empty() {
        return comm;
    }
    fs::read_link(proc.join("exe"))
        .ok()
        .and_then(|exe| exe.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| format!("Process {pid}"))
}

/// What's remembered between runs, to tell a switched-off camera from none.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Seen {
    #[serde(default)]
    builtin_seen: bool,
}

fn state_file() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(dir.join(crate::APP_ID).join("cameras.json"))
}

/// Whether a built-in camera is known to this machine but missing now,
/// remembering it when it's here.
fn privacy_switch(builtin_now: bool) -> bool {
    let file = state_file();
    let seen: Seen = file
        .as_ref()
        .and_then(|f| fs::read(f).ok())
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or_default();
    if builtin_now && !seen.builtin_seen {
        if let Some(file) = file {
            let result = file
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| {
                    let data = serde_json::to_vec(&Seen { builtin_seen: true })
                        .map_err(std::io::Error::other)?;
                    fs::write(&file, data)
                });
            if let Err(err) = result {
                tracing::warn!("cannot remember the camera: {err}");
            }
        }
    }
    seen.builtin_seen && !builtin_now
}

/// One physical device's nodes, in order, with its sysfs name.
struct Group {
    device: PathBuf,
    nodes: Vec<(u32, String)>,
    name: String,
}

/// The nodes of `/sys/class/video4linux`, grouped by the device behind
/// them, in order of their first node.
fn groups() -> Vec<Group> {
    let mut groups: HashMap<PathBuf, Group> = HashMap::new();
    let Ok(entries) = fs::read_dir("/sys/class/video4linux") else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(index) = name
            .strip_prefix("video")
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let sysfs = entry.path();
        // A virtual device has no device behind it: it stands alone.
        let device = fs::canonicalize(sysfs.join("device"))
            .or_else(|_| fs::canonicalize(&sysfs))
            .unwrap_or(sysfs.clone());
        let group = groups.entry(device.clone()).or_insert_with(|| Group {
            device,
            nodes: Vec::new(),
            name: fs::read_to_string(sysfs.join("name"))
                .map(|n| n.trim().to_owned())
                .unwrap_or_default(),
        });
        group.nodes.push((index, format!("/dev/video{index}")));
    }
    let mut groups: Vec<Group> = groups.into_values().collect();
    for group in &mut groups {
        group.nodes.sort_unstable();
    }
    groups.sort_by_key(|g| g.nodes.first().map_or(u32::MAX, |n| n.0));
    groups
}

/// The last snapshot of each camera, by id. Opening a USB camera that has
/// gone to sleep wakes it, which takes a tenth of a second and costs power,
/// so one that's asleep is described from memory: nothing can have changed
/// it, since holding it open keeps it awake.
static REMEMBERED: std::sync::Mutex<Vec<Camera>> = std::sync::Mutex::new(Vec::new());

/// Whether the USB device behind a sysfs device path is runtime-suspended,
/// read from the nearest `power/runtime_status` above it.
fn suspended(device: &Path) -> bool {
    device
        .ancestors()
        .find_map(|dir| fs::read_to_string(dir.join("power/runtime_status")).ok())
        .is_some_and(|status| status.trim() == "suspended")
}

/// What was last read of the camera at `node`, if anything, and forget it.
fn forget(node: &str) {
    if let Ok(mut remembered) = REMEMBERED.lock() {
        remembered.retain(|c| c.node != node);
    }
}

/// The camera a group is, from its first node that captures video.
fn camera(group: &Group, holders: &[(String, User)]) -> Option<Camera> {
    let users = |camera: &mut Camera| {
        for (held, user) in holders {
            if group.nodes.iter().any(|(_, n)| n == held)
                && !camera.users.iter().any(|u| u.pid == user.pid)
            {
                camera.users.push(user.clone());
            }
        }
    };
    let id = group.device.to_string_lossy().into_owned();
    if suspended(&group.device)
        && let Ok(remembered) = REMEMBERED.lock()
        && let Some(known) = remembered.iter().find(|c| c.id == id)
    {
        let mut camera = known.clone();
        camera.users.clear();
        users(&mut camera);
        return Some(camera);
    }
    let (node, file, cap) = group.nodes.iter().find_map(|(_, node)| {
        let file = open(node).ok()?;
        let cap = capability(&file).filter(captures)?;
        Some((node.clone(), file, cap))
    })?;
    let driver = text(&cap.driver);
    let card = tidy(&text(&cap.card));
    let name = if card.is_empty() {
        tidy(&group.name)
    } else {
        card
    };
    let mut camera = Camera {
        bus: bus(&driver, &group.device, &name),
        id,
        name,
        driver,
        resolution: resolution(&file),
        frame_sizes: preview_format(&formats(&file))
            .map(|f| frame_sizes(&file, f))
            .unwrap_or_default(),
        users: Vec::new(),
        controls: query(&file)
            .iter()
            .filter_map(|ctrl| control(&file, ctrl))
            .collect(),
        node,
    };
    if let Ok(mut remembered) = REMEMBERED.lock() {
        remembered.retain(|c| c.id != camera.id);
        remembered.push(camera.clone());
    }
    users(&mut camera);
    Some(camera)
}

fn read() -> Cameras {
    let holders = holders();
    let mut list: Vec<Camera> = groups()
        .iter()
        .filter_map(|group| camera(group, &holders))
        .collect();
    // Built-in first, then by node.
    list.sort_by_key(|c| (c.bus != Bus::Builtin, c.node.clone()));
    let builtin_now = list.iter().any(|c| c.bus == Bus::Builtin);
    Cameras {
        list,
        privacy_switch: privacy_switch(builtin_now),
    }
}

pub async fn now() -> Option<Cameras> {
    tokio::task::spawn_blocking(read).await.ok()
}

pub fn subscription() -> Subscription<Cameras> {
    Subscription::run(|| super::watch(Duration::from_secs(3), super::no_changes, now))
}

fn set(node: &str, id: u32, value: i32) -> std::io::Result<()> {
    forget(node);
    let file = open(node)?;
    let mut ctrl = ControlValue { id, value };
    ioctl(&file, VIDIOC_S_CTRL, &mut ctrl)
}

pub async fn set_control(node: String, id: u32, value: i32) {
    static LATEST: Latest<(String, u32, i32)> = Latest::new();
    LATEST
        .send((node, id, value), |(node, id, value)| async move {
            let result = tokio::task::spawn_blocking(move || {
                set(&node, id, value).map_err(|err| format!("on {node}: {err}"))
            })
            .await;
            if let Err(err) = result.unwrap_or_else(|err| Err(err.to_string())) {
                tracing::warn!("cannot set camera control {id:#x} {err}");
            }
        })
        .await;
}

/// The pixel formats a node can capture in.
fn formats(node: &File) -> Vec<u32> {
    let mut found = Vec::new();
    for index in 0..64 {
        let mut desc = FmtDesc::zeroed();
        desc.index = index;
        desc.kind = BUF_TYPE_VIDEO_CAPTURE;
        if ioctl(node, VIDIOC_ENUM_FMT, &mut desc).is_err() {
            break;
        }
        found.push(desc.pixelformat);
    }
    found
}

/// The pixel format the preview streams in: MJPEG if the device has it,
/// else YUYV.
fn preview_format(formats: &[u32]) -> Option<u32> {
    [PIX_FMT_MJPEG, PIX_FMT_YUYV]
        .into_iter()
        .find(|f| formats.contains(f))
}

/// The sizes a node captures `pixelformat` in, smallest first. For a device
/// that takes any size in a range, the common ones within it and its
/// largest.
fn frame_sizes(node: &File, pixelformat: u32) -> Vec<(u32, u32)> {
    let mut sizes = Vec::new();
    for index in 0..64 {
        let mut entry = FrameSizeEnum::zeroed();
        entry.index = index;
        entry.pixel_format = pixelformat;
        if ioctl(node, VIDIOC_ENUM_FRAMESIZES, &mut entry).is_err() {
            break;
        }
        if entry.kind == FRMSIZE_TYPE_DISCRETE {
            sizes.push((entry.size[0], entry.size[1]));
            continue;
        }
        let [min_w, max_w, _, min_h, max_h, _] = entry.size;
        sizes.extend(
            COMMON_SIZES
                .into_iter()
                .chain([(max_w, max_h)])
                .filter(|(w, h)| (min_w..=max_w).contains(w) && (min_h..=max_h).contains(h)),
        );
        break;
    }
    sizes.retain(|(w, h)| *w > 0 && *h > 0);
    sizes.sort_unstable();
    sizes.dedup();
    sizes
}

/// Whether a frame is waiting on the node, within `timeout_ms`.
fn ready(node: &File, timeout_ms: i32) -> std::io::Result<bool> {
    let mut poll = libc::pollfd {
        fd: node.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one pollfd, as the count says, live for the call.
    let rc = unsafe { libc::poll(&mut poll, 1, timeout_ms) };
    if rc < 0 {
        let err = std::io::Error::last_os_error();
        return if err.kind() == std::io::ErrorKind::Interrupted {
            Ok(false)
        } else {
            Err(err)
        };
    }
    Ok(rc > 0)
}

/// One of the driver's buffers, mapped into memory until dropped.
struct Mapping {
    ptr: *mut libc::c_void,
    len: usize,
}

impl Mapping {
    fn new(node: &File, buffer: &Buffer) -> std::io::Result<Self> {
        let len = buffer.length as usize;
        let offset = (buffer.m & 0xffff_ffff) as libc::off_t;
        // SAFETY: a shared mapping of the driver's buffer at the offset and
        // length it reported, unmapped in Drop and never touched after.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                node.as_raw_fd(),
                offset,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self { ptr, len })
    }

    /// The first `used` bytes of the buffer.
    fn bytes(&self, used: usize) -> &[u8] {
        // SAFETY: the mapping lives as long as `self`, and the length is
        // kept within it.
        unsafe { std::slice::from_raw_parts(self.ptr.cast::<u8>(), used.min(self.len)) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: the mapping was made by `new` with this pointer and
        // length, and no slice of it outlives `self`.
        unsafe { libc::munmap(self.ptr, self.len) };
    }
}

/// A camera streaming into mapped buffers; stopped, unmapped and closed
/// when dropped.
struct Session {
    node: File,
    mappings: Vec<Mapping>,
    width: u32,
    height: u32,
    pixelformat: u32,
    bytesperline: u32,
}

impl Session {
    /// The camera at `path` streaming frames of about `size` (the preview's
    /// small default when none): MJPEG if it has it, else YUYV. Fails when
    /// it's busy or has neither.
    fn start(path: &str, size: Option<(u32, u32)>) -> std::io::Result<Self> {
        let node = open(path)?;
        let wanted = preview_format(&formats(&node))
            .ok_or_else(|| std::io::Error::other("neither MJPEG nor YUYV"))?;
        let (width, height) = size.unwrap_or((PREVIEW_WIDTH, PREVIEW_HEIGHT));
        let mut format = Format::zeroed();
        format.kind = BUF_TYPE_VIDEO_CAPTURE;
        format.set_word(PIX_WIDTH, width);
        format.set_word(PIX_HEIGHT, height);
        format.set_word(PIX_PIXELFORMAT, wanted);
        ioctl(&node, VIDIOC_S_FMT, &mut format)?;
        let pixelformat = format.word(PIX_PIXELFORMAT);
        if pixelformat != PIX_FMT_MJPEG && pixelformat != PIX_FMT_YUYV {
            return Err(std::io::Error::other("the driver changed the pixel format"));
        }
        let mut request = RequestBuffers::zeroed();
        request.count = PREVIEW_BUFFERS;
        request.kind = BUF_TYPE_VIDEO_CAPTURE;
        request.memory = MEMORY_MMAP;
        ioctl(&node, VIDIOC_REQBUFS, &mut request)?;
        if request.count == 0 {
            return Err(std::io::Error::other("no buffers"));
        }
        let mut session = Self {
            node,
            mappings: Vec::new(),
            width: format.word(PIX_WIDTH),
            height: format.word(PIX_HEIGHT),
            pixelformat,
            bytesperline: format.word(PIX_BYTESPERLINE),
        };
        for index in 0..request.count {
            let mut buffer = session.buffer(index);
            ioctl(&session.node, VIDIOC_QUERYBUF, &mut buffer)?;
            session.mappings.push(Mapping::new(&session.node, &buffer)?);
            session.requeue(index)?;
        }
        let mut kind = BUF_TYPE_VIDEO_CAPTURE;
        ioctl(&session.node, VIDIOC_STREAMON, &mut kind)?;
        Ok(session)
    }

    fn buffer(&self, index: u32) -> Buffer {
        let mut buffer = Buffer::zeroed();
        buffer.index = index;
        buffer.kind = BUF_TYPE_VIDEO_CAPTURE;
        buffer.memory = MEMORY_MMAP;
        buffer
    }

    /// A buffer the driver has filled, if one is ready within the timeout.
    fn dequeue(&self, timeout_ms: i32) -> std::io::Result<Option<Buffer>> {
        if !ready(&self.node, timeout_ms)? {
            return Ok(None);
        }
        let mut buffer = self.buffer(0);
        match ioctl(&self.node, VIDIOC_DQBUF, &mut buffer) {
            Ok(()) => Ok(Some(buffer)),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Buffer `index` back to the driver to fill.
    fn requeue(&self, index: u32) -> std::io::Result<()> {
        let mut buffer = self.buffer(index);
        ioctl(&self.node, VIDIOC_QBUF, &mut buffer)
    }

    /// The newest frame, within the timeout, or nothing for a frame the
    /// driver marked bad. Older frames waiting are skipped.
    fn frame(&mut self) -> std::io::Result<Option<Frame>> {
        let mut buffer = self
            .dequeue(FRAME_TIMEOUT_MS)?
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, "no frame"))?;
        while let Some(newer) = self.dequeue(0)? {
            self.requeue(buffer.index)?;
            buffer = newer;
        }
        let frame = if buffer.flags & BUF_FLAG_ERROR != 0 {
            None
        } else {
            self.decode(&buffer)
        };
        self.requeue(buffer.index)?;
        Ok(frame)
    }

    fn decode(&self, buffer: &Buffer) -> Option<Frame> {
        let data = self
            .mappings
            .get(buffer.index as usize)?
            .bytes(buffer.bytesused as usize);
        let image = if self.pixelformat == PIX_FMT_MJPEG {
            image::load_from_memory_with_format(data, image::ImageFormat::Jpeg)
                .ok()?
                .into_rgba8()
        } else {
            yuyv_to_rgba(data, self.width, self.height, self.bytesperline as usize)?
        };
        Some(frame_from(image))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let mut kind = BUF_TYPE_VIDEO_CAPTURE;
        _ = ioctl(&self.node, VIDIOC_STREAMOFF, &mut kind);
        self.mappings.clear();
    }
}

/// A packed YUYV picture as RGBA, by BT.601.
fn yuyv_to_rgba(data: &[u8], width: u32, height: u32, stride: usize) -> Option<image::RgbaImage> {
    let (w, h) = (width as usize, height as usize);
    let stride = stride.max(w * 2);
    if w == 0 || h == 0 || data.len() < stride * (h - 1) + w * 2 {
        return None;
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in data.chunks(stride).take(h) {
        for px in row[..w * 2].chunks_exact(4) {
            rgba.extend_from_slice(&yuv_to_rgba(px[0], px[1], px[3]));
            rgba.extend_from_slice(&yuv_to_rgba(px[2], px[1], px[3]));
        }
    }
    image::RgbaImage::from_raw(width, height, rgba)
}

fn yuv_to_rgba(y: u8, u: u8, v: u8) -> [u8; 4] {
    let c = 298 * (i32::from(y) - 16);
    let d = i32::from(u) - 128;
    let e = i32::from(v) - 128;
    let channel = |value: i32| ((value + 128) >> 8).clamp(0, 255) as u8;
    [
        channel(c + 409 * e),
        channel(c - 100 * d - 208 * e),
        channel(c + 516 * d),
        255,
    ]
}

/// A frame to send on, no wider than the preview wants.
fn frame_from(image: image::RgbaImage) -> Frame {
    let image = if image.width() > PREVIEW_MAX_WIDTH {
        let height = (u64::from(image.height()) * u64::from(PREVIEW_MAX_WIDTH)
            / u64::from(image.width()))
        .max(1) as u32;
        image::imageops::resize(
            &image,
            PREVIEW_MAX_WIDTH,
            height,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };
    Frame {
        width: image.width(),
        height: image.height(),
        rgba: Arc::new(image.into_raw()),
    }
}

/// Waits `how_long` unless `out` closes first; whether it's still open.
fn wait(out: &mpsc::Sender<Frame>, how_long: Duration) -> bool {
    let until = Instant::now() + how_long;
    loop {
        if out.is_closed() {
            return false;
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return true;
        }
        std::thread::sleep(left.min(Duration::from_millis(100)));
    }
}

/// Frames from the session into `out`, a few a second, until `out` closes
/// or the camera fails.
fn run(session: &mut Session, out: &mut mpsc::Sender<Frame>) -> std::io::Result<()> {
    loop {
        let started = Instant::now();
        if let Some(frame) = session.frame()?
            && let Err(err) = out.try_send(frame)
            && err.is_disconnected()
        {
            return Ok(());
        }
        if !wait(out, PREVIEW_INTERVAL.saturating_sub(started.elapsed())) {
            return Ok(());
        }
    }
}

/// The preview thread: streams `node` into `out` for as long as `out` is
/// held, trying again every so often while the camera can't be streamed.
fn stream_frames(node: String, size: Option<(u32, u32)>, mut out: mpsc::Sender<Frame>) {
    loop {
        match Session::start(&node, size) {
            Ok(mut session) => {
                if let Err(err) = run(&mut session, &mut out) {
                    tracing::debug!("camera preview on {node} stopped: {err}");
                }
            }
            Err(err) => tracing::debug!("cannot stream {node}: {err}"),
        }
        // The session is closed now: the camera is back to what it was.
        forget(&node);
        if !wait(&out, PREVIEW_RETRY) {
            return;
        }
    }
}

/// Pictures from the camera at the node in `request`, a few a second, for
/// as long as the stream is polled. The size in `request` is asked of the
/// camera (one of its `frame_sizes`; a small default when none), and the
/// pictures are shrunk to fit the preview anyway. Nothing while the device
/// can't be streamed (busy, no usable format): it's tried again every 2 s.
/// The camera is released promptly once the stream is dropped.
pub fn preview(
    request: &(String, Option<(u32, u32)>),
) -> impl Stream<Item = Frame> + Send + 'static + use<> {
    let (out, frames) = mpsc::channel(1);
    let (node, size) = request.clone();
    if let Err(err) = std::thread::Builder::new()
        .name("camera-preview".to_owned())
        .spawn(move || stream_frames(node, size, out))
    {
        tracing::warn!("cannot start the camera preview: {err}");
    }
    frames
}

/// Every writable control back to its default: the switches and menus
/// first, since an automatic mode decides whether its manual value takes.
pub async fn reset_controls(node: String) {
    let reset = move || {
        forget(&node);
        let file = open(&node)?;
        let mut ctrls: Vec<QueryCtrl> = query(&file)
            .into_iter()
            .filter(|c| settable(c) && c.flags & CTRL_FLAG_READ_ONLY == 0)
            .collect();
        ctrls.sort_by_key(|c| c.kind == CTRL_TYPE_INTEGER);
        for ctrl in ctrls {
            let mut set = ControlValue {
                id: ctrl.id,
                value: ctrl.default_value,
            };
            if let Err(err) = ioctl(&file, VIDIOC_S_CTRL, &mut set) {
                tracing::debug!("cannot reset camera control {:#x}: {err}", ctrl.id);
            }
        }
        Ok::<(), std::io::Error>(())
    };
    let result = tokio::task::spawn_blocking(reset).await;
    if let Err(err) = result.unwrap_or_else(|err| Err(std::io::Error::other(err))) {
        tracing::warn!("cannot reset camera controls: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_match_the_kernel() {
        assert_eq!(VIDIOC_QUERYCAP, 0x8068_5600);
        assert_eq!(VIDIOC_ENUM_FMT, 0xc040_5602);
        assert_eq!(VIDIOC_G_FMT, 0xc0d0_5604);
        assert_eq!(VIDIOC_S_FMT, 0xc0d0_5605);
        assert_eq!(VIDIOC_REQBUFS, 0xc014_5608);
        assert_eq!(VIDIOC_QUERYBUF, 0xc058_5609);
        assert_eq!(VIDIOC_QBUF, 0xc058_560f);
        assert_eq!(VIDIOC_DQBUF, 0xc058_5611);
        assert_eq!(VIDIOC_STREAMON, 0x4004_5612);
        assert_eq!(VIDIOC_STREAMOFF, 0x4004_5613);
        assert_eq!(VIDIOC_G_CTRL, 0xc008_561b);
        assert_eq!(VIDIOC_S_CTRL, 0xc008_561c);
        assert_eq!(VIDIOC_QUERYCTRL, 0xc044_5624);
        assert_eq!(VIDIOC_QUERYMENU, 0xc02c_5625);
        assert_eq!(VIDIOC_ENUM_FRAMESIZES, 0xc02c_564a);
        assert_eq!(size_of::<FrameSizeEnum>(), 44);
        assert_eq!(PIX_FMT_MJPEG, 0x4750_4a4d);
        assert_eq!(PIX_FMT_YUYV, 0x5659_5559);
    }

    #[test]
    fn yuyv_becomes_rgba() {
        // White and black share one chroma pair; then two reds; a row of
        // padding after each line.
        let data = [235, 128, 16, 128, 81, 90, 81, 240, 0, 0, 0, 0];
        let image = yuyv_to_rgba(&data, 4, 1, 12).unwrap();
        let pixels: Vec<[u8; 4]> = image.pixels().map(|p| p.0).collect();
        assert_eq!(
            pixels,
            [
                [255, 255, 255, 255],
                [0, 0, 0, 255],
                [255, 0, 0, 255],
                [255, 0, 0, 255]
            ]
        );
        assert!(
            yuyv_to_rgba(&data[..7], 4, 1, 0).is_none(),
            "a short buffer is no picture"
        );
        assert_eq!(
            frame_from(image).width,
            4,
            "a small picture is left as it is"
        );
    }

    #[test]
    fn wide_frames_are_shrunk() {
        let frame = frame_from(image::RgbaImage::new(1920, 1080));
        assert_eq!((frame.width, frame.height), (832, 468));
        assert_eq!(frame.rgba.len(), 832 * 468 * 4);
        assert_eq!(frame_from(image::RgbaImage::new(640, 360)).width, 640);
    }

    /// Three frames of the camera, straight and through `preview`, and
    /// whether it is let go after.
    /// `cargo test --release camera::tests::frames -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn frames() {
        use futures::StreamExt;
        let node = "/dev/video0".to_owned();
        let started = Instant::now();
        let mut session = Session::start(&node, None).expect("the camera streams");
        let fourcc = String::from_utf8_lossy(&session.pixelformat.to_le_bytes()).into_owned();
        println!(
            "{fourcc} {}x{} (stride {}) started in {:?}",
            session.width,
            session.height,
            session.bytesperline,
            started.elapsed()
        );
        for _ in 0..3 {
            let started = Instant::now();
            let frame = session.frame().expect("a frame comes");
            if let (Some(f), Ok(dir)) = (frame.as_ref(), std::env::var("QS_FRAME_DIR")) {
                let _ = image::RgbaImage::from_raw(f.width, f.height, f.rgba.as_ref().clone())
                    .map(|img| img.save(format!("{dir}/frame.png")));
            }
            println!(
                "frame {:?} in {:?}",
                frame.map(|f| (f.width, f.height, f.rgba.len())),
                started.elapsed()
            );
        }
        drop(session);
        let started = Instant::now();
        let frames = futures::executor::block_on(
            preview(&(node.clone(), Some((1280, 720))))
                .take(45)
                .collect::<Vec<Frame>>(),
        );
        if let (Some(f), Ok(dir)) = (frames.last(), std::env::var("QS_FRAME_DIR")) {
            let _ = image::RgbaImage::from_raw(f.width, f.height, f.rgba.as_ref().clone())
                .map(|img| img.save(format!("{dir}/frame-late.png")));
        }
        println!(
            "{} frames through preview at 1280x720 in {:?}: {:?}",
            frames.len(),
            started.elapsed(),
            frames
                .iter()
                .map(|f| (f.width, f.height))
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(300));
        let open: Vec<String> = fs::read_dir("/proc/self/fd")
            .unwrap()
            .flatten()
            .filter_map(|fd| fs::read_link(fd.path()).ok())
            .map(|t| t.to_string_lossy().into_owned())
            .filter(|t| t.starts_with("/dev/video"))
            .collect();
        println!("video nodes still open: {open:?}");
        assert!(
            open.is_empty(),
            "the camera is closed once the preview is dropped"
        );
    }

    #[test]
    fn names_are_tidied() {
        assert_eq!(tidy("Laptop Camera: Laptop Camera"), "Laptop Camera");
        assert_eq!(tidy("Integrated Camera: Integrated C"), "Integrated Camera");
        assert_eq!(
            tidy("Laptop Webcam Module (2nd Gen):"),
            "Laptop Webcam Module (2nd Gen)"
        );
        assert_eq!(tidy("  HD Pro Webcam C920 "), "HD Pro Webcam C920");
        assert_eq!(
            tidy("Integrated Camera: Integrated I"),
            "Integrated Camera: Integrated I"
        );
        assert_eq!(tidy(""), "");
    }

    #[test]
    fn icons_follow_the_name() {
        assert_eq!(icon_for("Brightness"), "sun");
        assert_eq!(icon_for("Contrast"), "contrast");
        assert_eq!(icon_for("Sharpness"), "focus-2");
        assert_eq!(icon_for("Exposure, Auto"), "exposure");
        assert_eq!(icon_for("Exposure Time, Absolute"), "exposure");
        assert_eq!(icon_for("White Balance, Automatic"), "temperature");
        assert_eq!(icon_for("White Balance Temperature"), "temperature");
        assert_eq!(icon_for("Backlight Compensation"), "brightness-half");
        assert_eq!(icon_for("Gain"), "adjustments");
        assert_eq!(icon_for("Saturation"), "adjustments");
        assert_eq!(icon_for("Focus, Automatic Continuous"), "focus-2");
        assert_eq!(icon_for("Zoom, Absolute"), "focus-2");
        assert_eq!(icon_for("Power Line Frequency"), "bolt");
        assert_eq!(icon_for("Something Else"), "adjustments");
    }

    #[test]
    fn bytes_make_text() {
        assert_eq!(text(b"uvcvideo\0\0\0\0\0\0\0\0"), "uvcvideo");
        assert_eq!(text(b"no terminator"), "no terminator");
    }

    /// What this machine has, and how long each read takes: the first wakes
    /// a sleeping USB camera, the second finds it awake, the third, after
    /// it has gone back to sleep, is from memory.
    /// `cargo test --release camera::tests::here -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn here() {
        let started = std::time::Instant::now();
        let held = holders();
        println!("holders scanned in {:?}: {held:?}", started.elapsed());
        let started = std::time::Instant::now();
        let cameras = read();
        println!("{cameras:#?}");
        println!("first read in {:?}", started.elapsed());
        let started = std::time::Instant::now();
        let again = read();
        println!(
            "second read in {:?}, same: {}",
            started.elapsed(),
            again == cameras
        );
        std::thread::sleep(Duration::from_secs(3));
        let started = std::time::Instant::now();
        let asleep = read();
        println!(
            "third read in {:?}, same: {}",
            started.elapsed(),
            asleep == cameras
        );
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;
    use std::time::Instant;

    /// `cargo test --release --lib camera::size_tests::big -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn big() {
        let node = "/dev/video0".to_owned();
        for size in [Some((1920, 1080)), Some((1280, 720)), None] {
            let started = Instant::now();
            match Session::start(&node, size) {
                Ok(mut session) => {
                    println!(
                        "{size:?}: started {}x{} in {:?}",
                        session.width,
                        session.height,
                        started.elapsed()
                    );
                    for _ in 0..3 {
                        let started = Instant::now();
                        match session.frame() {
                            Ok(frame) => println!(
                                "  frame {:?} in {:?}",
                                frame.map(|f| (f.width, f.height)),
                                started.elapsed()
                            ),
                            Err(err) => println!("  frame error: {err}"),
                        }
                    }
                }
                Err(err) => println!("{size:?}: cannot start: {err}"),
            }
        }
    }
}
