// SPDX-License-Identifier: GPL-3.0-only

//! Stills and recordings straight from the compositor, through
//! ext-image-copy-capture, which COSMIC offers every client: no portal, so
//! nothing asks for permission. A thread with its own wayland connection
//! takes a still of each output as the overlay opens, lists the windows
//! (with small pictures when asked), takes a window on its own (covered or
//! on another workspace, it doesn't matter), and while recording copies
//! frames to ffmpeg, which encodes H.264 on the integrated GPU (VAAPI).

use std::{
    collections::{HashMap, HashSet},
    io::Write,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command as Process, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use cosmic::{
    cctk::{
        self,
        cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::State as ToplevelFlags,
        screencopy::{
            CaptureFrame, CaptureOptions, CaptureSession, CaptureSource, FailureReason, Formats,
            Frame, ScreencopyFrameData, ScreencopyHandler, ScreencopySessionData, ScreencopyState,
        },
        sctk::{
            self,
            output::{OutputHandler, OutputState},
            reexports::{
                calloop::{
                    self, LoopHandle,
                    timer::{TimeoutAction, Timer},
                },
                calloop_wayland_source::WaylandSource,
            },
            registry::{ProvidesRegistryState, RegistryState},
            shm::{Shm, ShmHandler, raw::RawPool},
        },
        toplevel_info::{ToplevelInfoHandler, ToplevelInfoState},
        wayland_client::{
            Connection, Dispatch, QueueHandle, WEnum,
            globals::registry_queue_init,
            protocol::{
                wl_buffer::{self, WlBuffer},
                wl_output::WlOutput,
                wl_shm,
            },
        },
        wayland_protocols::ext::{
            foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
            workspace::v1::client::ext_workspace_handle_v1::State as WorkspaceFlags,
        },
        workspace::{WorkspaceHandler, WorkspaceState},
    },
    iced::{Subscription, stream},
};
use futures::{SinkExt, StreamExt, channel::mpsc, executor::block_on};
use serde_json::Value;

/// A recording's frames a second, the newest repeated while the screen holds still.
const FPS: u32 = 60;
/// How many roundtrips the windows take to fill in (see `backend::toplevels`).
const ROUNDTRIPS: usize = 6;

/// A rectangle in whole pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// A picture of an output (by name) or a window (by id), in its buffer's pixels.
#[derive(Clone)]
pub struct Shot {
    pub name: String,
    pub image: Arc<image::RgbaImage>,
}

impl std::fmt::Debug for Shot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Shot({}, {}×{})",
            self.name,
            self.image.width(),
            self.image.height()
        )
    }
}

/// A window, wherever it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub id: String,
    pub title: String,
    pub app_id: String,
    /// Where it shows: per output it's on, in logical pixels from that
    /// output's corner; empty when minimized or on a workspace not shown.
    pub shown: Vec<(String, Rect)>,
    /// Focused, so on top of the others.
    pub active: bool,
}

/// The running cabin-capture on the session bus.
pub const BUS_NAME: &str = "dev.troy.Cabin.Capture";
pub const BUS_PATH: &str = "/dev/troy/Cabin/Capture";

#[zbus::proxy(
    interface = "dev.troy.Cabin.Capture",
    default_service = "dev.troy.Cabin.Capture",
    default_path = "/dev/troy/Cabin/Capture"
)]
pub trait Running {
    /// The shortcut again: cancels the choosing, or stops the recording.
    fn toggle(&self) -> zbus::Result<()>;
    /// What's being recorded ("Entire screen · eDP-1"), empty when nothing.
    fn recording(&self) -> zbus::Result<String>;
}

/// The sound a recording takes in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sound {
    /// Whatever plays: the default output's monitor.
    pub desktop: bool,
    /// One app's sound alone, by its name in the registry ("Spotify").
    pub app: Option<String>,
    /// The default input.
    pub mic: bool,
}

#[derive(Clone, Debug)]
pub enum Target {
    /// An output, by name.
    Output(String),
    /// A window, by its foreign-toplevel identifier.
    Window(String),
}

/// What the thread reports.
#[derive(Clone, Debug)]
pub enum Event {
    Ready(calloop::channel::SyncSender<Command>),
    /// An output as it looked when the overlay opened.
    Shot(Shot),
    /// A window taken whole (`Command::Shoot`).
    Window(Shot),
    /// A window's small picture (`Command::Thumbnails`).
    Thumb(Shot),
    /// Every window, again on every change.
    Windows(Vec<Window>),
    /// A recording, finished and saved.
    Recorded(PathBuf),
    Failed(String),
    /// The thread could not start, or has ended.
    Unavailable,
}

/// What the thread is asked to do.
#[derive(Debug)]
pub enum Command {
    /// `crop` is in the target's buffer pixels.
    Record {
        target: Target,
        crop: Option<Rect>,
        sound: Sound,
        path: PathBuf,
    },
    Stop,
    /// A window, by id, taken whole.
    Shoot(String),
    /// A small picture of every window, from now on.
    Thumbnails,
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run(|| {
        stream::channel(8, |mut output: mpsc::Sender<Event>| async move {
            let (tx, mut rx) = mpsc::channel(8);
            let (commands, receiver) = calloop::channel::sync_channel(8);
            if std::thread::Builder::new()
                .name("capture".into())
                .spawn(move || run(receiver, tx))
                .is_ok()
            {
                let _ = output.send(Event::Ready(commands)).await;
                let mut held = None;
                while let Some(mut event) = match held.take() {
                    Some(event) => Some(event),
                    None => rx.next().await,
                } {
                    // A moving window sends a list per step: of those queued,
                    // only the newest counts, so the overlay never trails.
                    if matches!(event, Event::Windows(_)) {
                        while let Ok(next) = rx.try_recv() {
                            if matches!(next, Event::Windows(_)) {
                                event = next;
                            } else {
                                held = Some(next);
                                break;
                            }
                        }
                    }
                    if output.send(event).await.is_err() {
                        return;
                    }
                }
            }
            let _ = output.send(Event::Unavailable).await;
            futures::future::pending().await
        })
    })
}

fn run(commands: calloop::channel::Channel<Command>, tx: mpsc::Sender<Event>) {
    let connection = match Connection::connect_to_env() {
        Ok(connection) => connection,
        Err(err) => return tracing::error!("capture: no wayland connection: {err}"),
    };
    let (globals, mut queue) = match registry_queue_init::<State>(&connection) {
        Ok(init) => init,
        Err(err) => return tracing::error!("capture: no registry: {err}"),
    };
    let mut event_loop = match calloop::EventLoop::<State>::try_new() {
        Ok(event_loop) => event_loop,
        Err(err) => return tracing::error!("capture: no event loop: {err}"),
    };
    let qh = queue.handle();
    let registry = RegistryState::new(&globals);
    let shm = match Shm::bind(&globals, &qh) {
        Ok(shm) => shm,
        Err(err) => return tracing::error!("capture: no shared memory: {err}"),
    };
    // Workspaces before the windows, which name them.
    let workspaces = WorkspaceState::new(&registry, &qh);
    let Some(toplevels) = ToplevelInfoState::try_new(&registry, &qh) else {
        return tracing::error!("capture: the compositor lists no windows");
    };
    let mut state = State {
        outputs: OutputState::new(&globals, &qh),
        screencopy: ScreencopyState::new(&globals, &qh),
        shm,
        workspaces,
        toplevels,
        registry,
        qh,
        handle: event_loop.handle(),
        tx,
        jobs: HashMap::new(),
        windows: None,
        thumbs: None,
        running: true,
    };
    for _ in 0..ROUNDTRIPS {
        if let Err(err) = queue.roundtrip(&mut state) {
            return tracing::error!("capture: {err}");
        }
    }
    for output in state.outputs.outputs().collect::<Vec<_>>() {
        if let Some(name) = state.name(&output) {
            state.start(
                CaptureSource::Output(output),
                CaptureOptions::empty(),
                Kind::Still(Still::Output(name)),
            );
        }
    }
    state.send_windows();
    if WaylandSource::new(connection, queue)
        .insert(event_loop.handle())
        .is_err()
    {
        return tracing::error!("capture: cannot watch the wayland socket");
    }
    let inserted = event_loop
        .handle()
        .insert_source(commands, |event, (), state| match event {
            calloop::channel::Event::Msg(command) => state.command(command),
            calloop::channel::Event::Closed => state.running = false,
        });
    if inserted.is_err() {
        return tracing::error!("capture: cannot watch for commands");
    }
    while state.running {
        if let Err(err) = event_loop.dispatch(None, &mut state) {
            return tracing::error!("capture: {err}");
        }
    }
}

struct State {
    registry: RegistryState,
    outputs: OutputState,
    shm: Shm,
    screencopy: ScreencopyState,
    workspaces: WorkspaceState,
    toplevels: ToplevelInfoState,
    qh: QueueHandle<State>,
    handle: LoopHandle<'static, State>,
    tx: mpsc::Sender<Event>,
    jobs: HashMap<CaptureSession, Job>,
    /// The windows last sent; `None` until the first list is complete.
    windows: Option<Vec<Window>>,
    /// The windows whose small pictures were taken; `None` until asked for.
    thumbs: Option<HashSet<String>>,
    running: bool,
}

struct Job {
    kind: Kind,
    buffer: Option<Buffer>,
    /// A frame asked for and not yet in.
    pending: bool,
    /// New buffer constraints came since the last frame was asked for.
    fresh: bool,
}

enum Kind {
    Still(Still),
    Record(Recording),
}

/// One frame, of an output or a window by name.
enum Still {
    Output(String),
    Window(String),
    Thumb(String),
}

struct Recording {
    crop: Option<Rect>,
    sound: Sound,
    path: PathBuf,
    encoder: Option<Encoder>,
    asked: Option<Instant>,
}

impl State {
    fn name(&self, output: &WlOutput) -> Option<String> {
        self.outputs.info(output).and_then(|info| info.name)
    }

    fn send(&mut self, event: Event) {
        if block_on(self.tx.send(event)).is_err() {
            self.running = false;
        }
    }

    fn start(&mut self, source: CaptureSource, options: CaptureOptions, kind: Kind) -> bool {
        match self.screencopy.capturer().create_session(
            &source,
            options,
            &self.qh,
            ScreencopySessionData::default(),
        ) {
            Ok(session) => {
                self.jobs.insert(
                    session,
                    Job {
                        kind,
                        buffer: None,
                        pending: false,
                        fresh: false,
                    },
                );
                true
            }
            Err(err) => {
                tracing::warn!("capture: {err}");
                false
            }
        }
    }

    /// Asks for the next frame into the job's buffer.
    fn capture(&mut self, session: &CaptureSession) {
        let Some(job) = self.jobs.get_mut(session) else {
            return;
        };
        let Some(buffer) = &job.buffer else {
            return;
        };
        if job.pending {
            return;
        }
        job.pending = true;
        job.fresh = false;
        if let Kind::Record(recording) = &mut job.kind {
            recording.asked = Some(Instant::now());
        }
        let whole = cctk::screencopy::Rect {
            x: 0,
            y: 0,
            width: buffer.width as i32,
            height: buffer.height as i32,
        };
        session.capture(
            &buffer.buffer,
            &[whole],
            &self.qh,
            ScreencopyFrameData::default(),
        );
    }

    /// The next frame after `wait`: never from inside a frame's own event,
    /// as that frame lives until the handler returns and two at once is a
    /// protocol error.
    fn later(&self, session: CaptureSession, wait: Duration) {
        let _ = self
            .handle
            .insert_source(Timer::from_duration(wait), move |_, _, state| {
                state.capture(&session);
                TimeoutAction::Drop
            });
    }

    /// The job done with: a recording stops (its encoder reports), anything
    /// else fails.
    fn end(&mut self, session: &CaptureSession, why: &str) {
        let Some(job) = self.jobs.remove(session) else {
            return;
        };
        match job.kind {
            Kind::Record(Recording {
                encoder: Some(encoder),
                ..
            }) => encoder.stop(),
            Kind::Record(_) => self.send(Event::Failed(format!(
                "the recording ended before its first frame: {why}"
            ))),
            Kind::Still(Still::Output(name) | Still::Window(name)) => {
                self.send(Event::Failed(format!("no picture of {name}: {why}")))
            }
            // A card without a picture is no reason to stop.
            Kind::Still(Still::Thumb(id)) => {
                tracing::debug!("capture: no picture of window {id}: {why}")
            }
        }
    }

    fn window(&self, id: &str) -> Option<CaptureSource> {
        self.toplevels
            .toplevels()
            .find(|t| t.identifier == id)
            .map(|t| CaptureSource::Toplevel(t.foreign_toplevel.clone()))
    }

    /// A small picture of each window not yet taken, once asked for.
    fn thumbnails(&mut self) {
        let Some(taken) = &mut self.thumbs else {
            return;
        };
        let new: Vec<(String, CaptureSource)> = self
            .toplevels
            .toplevels()
            .filter(|t| taken.insert(t.identifier.clone()))
            .map(|t| {
                (
                    t.identifier.clone(),
                    CaptureSource::Toplevel(t.foreign_toplevel.clone()),
                )
            })
            .collect();
        for (id, source) in new {
            self.start(
                source,
                CaptureOptions::empty(),
                Kind::Still(Still::Thumb(id)),
            );
        }
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Shoot(id) => match self.window(&id) {
                Some(source) => {
                    if !self.start(
                        source,
                        CaptureOptions::empty(),
                        Kind::Still(Still::Window(id)),
                    ) {
                        self.send(Event::Failed(
                            "the compositor would not take the window".into(),
                        ));
                    }
                }
                None => self.send(Event::Failed("the window is gone".into())),
            },
            Command::Thumbnails => {
                self.thumbs.get_or_insert_default();
                self.thumbnails();
            }
            Command::Record {
                target,
                crop,
                sound,
                path,
            } => {
                let source = match &target {
                    Target::Output(name) => self
                        .outputs
                        .outputs()
                        .find(|o| self.name(o).as_ref() == Some(name))
                        .map(CaptureSource::Output),
                    Target::Window(id) => self.window(id),
                };
                let Some(source) = source else {
                    return self.send(Event::Failed(format!("{target:?} is gone")));
                };
                let recording = Recording {
                    crop,
                    sound,
                    path,
                    encoder: None,
                    asked: None,
                };
                if !self.start(
                    source,
                    CaptureOptions::PaintCursors,
                    Kind::Record(recording),
                ) {
                    self.send(Event::Failed("the compositor would not record it".into()));
                }
            }
            Command::Stop => {
                let recording: Vec<CaptureSession> = self
                    .jobs
                    .iter()
                    .filter(|(_, job)| matches!(job.kind, Kind::Record(_)))
                    .map(|(session, _)| session.clone())
                    .collect();
                for session in recording {
                    self.end(&session, "stopped");
                }
            }
        }
    }

    /// Every window, and where it shows: on an active workspace, not minimized.
    fn windows(&self) -> Vec<Window> {
        let active: HashSet<_> = self
            .workspaces
            .workspace_groups()
            .flat_map(|group| group.workspaces.iter())
            .filter(|handle| {
                self.workspaces
                    .workspace_info(handle)
                    .is_some_and(|w| w.state.contains(WorkspaceFlags::Active))
            })
            .collect();
        self.toplevels
            .toplevels()
            .map(|t| {
                let visible = !t.state.contains(&ToplevelFlags::Minimized)
                    && (t.workspace.is_empty() || t.workspace.iter().any(|w| active.contains(&w)));
                let shown = t
                    .geometry
                    .iter()
                    .filter(|_| visible)
                    .filter_map(|(output, g)| {
                        Some((
                            self.name(output)?,
                            Rect {
                                x: g.x,
                                y: g.y,
                                width: g.width,
                                height: g.height,
                            },
                        ))
                    });
                Window {
                    id: t.identifier.clone(),
                    title: t.title.clone(),
                    app_id: t.app_id.clone(),
                    shown: shown.collect(),
                    active: t.state.contains(&ToplevelFlags::Activated),
                }
            })
            .collect()
    }

    fn send_windows(&mut self) {
        let windows = self.windows();
        if self.windows.as_ref() != Some(&windows) {
            self.windows = Some(windows.clone());
            self.send(Event::Windows(windows));
        }
        self.thumbnails();
    }

    /// After the first full list, every change.
    fn windows_changed(&mut self) {
        if self.windows.is_some() {
            self.send_windows();
        }
    }
}

/// The 32-bit formats read, the compositor's choice among them.
const READABLE: [wl_shm::Format; 4] = [
    wl_shm::Format::Xrgb8888,
    wl_shm::Format::Argb8888,
    wl_shm::Format::Xbgr8888,
    wl_shm::Format::Abgr8888,
];

/// Whether a format lies in memory as B, G, R (else R, G, B), little-endian.
fn bgr(format: wl_shm::Format) -> bool {
    matches!(format, wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888)
}

/// Shared memory the compositor copies a frame into.
struct Buffer {
    pool: RawPool,
    buffer: WlBuffer,
    width: u32,
    height: u32,
    format: wl_shm::Format,
}

impl Buffer {
    fn new(
        shm: &Shm,
        width: u32,
        height: u32,
        format: wl_shm::Format,
        qh: &QueueHandle<State>,
    ) -> Result<Self, String> {
        let stride = width * 4;
        let mut pool =
            RawPool::new((stride * height) as usize, shm).map_err(|err| err.to_string())?;
        let buffer = pool.create_buffer(
            0,
            width as i32,
            height as i32,
            stride as i32,
            format,
            (),
            qh,
        );
        Ok(Self {
            pool,
            buffer,
            width,
            height,
            format,
        })
    }

    fn rgba(&mut self) -> image::RgbaImage {
        let (width, height) = (self.width, self.height);
        let swap = bgr(self.format);
        let mut pixels = self.pool.mmap()[..(width * height * 4) as usize].to_vec();
        for pixel in pixels.chunks_exact_mut(4) {
            if swap {
                pixel.swap(0, 2);
            }
            pixel[3] = 255;
        }
        image::RgbaImage::from_raw(width, height, pixels).expect("width × height × 4 bytes")
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        self.buffer.destroy();
        self.pool.pool().destroy();
    }
}

/// Small enough for a card, at most 480×300, the same shape.
fn shrink(image: &image::RgbaImage) -> image::RgbaImage {
    let (width, height) = (image.width() as f32, image.height() as f32);
    let scale = (480.0 / width).min(300.0 / height).min(1.0);
    image::imageops::thumbnail(
        image,
        ((width * scale) as u32).max(1),
        ((height * scale) as u32).max(1),
    )
}

/// `size` pixels of a `width`-wide BGRX picture from `crop`'s corner, into
/// `out`; what lies past the picture's edge is black.
fn cut(
    source: &[u8],
    (width, height): (u32, u32),
    crop: Option<Rect>,
    size: (u32, u32),
    out: &mut Vec<u8>,
) {
    let (x0, y0) = crop.map_or((0, 0), |c| (c.x.max(0) as usize, c.y.max(0) as usize));
    let (width, height) = (width as usize, height as usize);
    let (out_width, out_height) = (size.0 as usize, size.1 as usize);
    out.clear();
    out.resize(out_width * out_height * 4, 0);
    let row = out_width.min(width.saturating_sub(x0)) * 4;
    for y in 0..out_height.min(height.saturating_sub(y0)) {
        let from = ((y0 + y) * width + x0) * 4;
        out[y * out_width * 4..][..row].copy_from_slice(&source[from..from + row]);
    }
}

/// Frames between the capture thread and the encoder: the newest, and
/// those done with, to fill again. Reused, a frame costs a copy, not an
/// allocation's page faults, so the capture thread stays free for the
/// window moves the overlay follows.
#[derive(Default)]
struct Frames {
    newest: Option<Vec<u8>>,
    spare: Vec<Vec<u8>>,
}

/// ffmpeg, fed the newest frame at a steady rate on a thread of its own; it
/// reports the file, or why not, when stopped or broken.
struct Encoder {
    size: (u32, u32),
    frames: Arc<Mutex<Frames>>,
    stop: Arc<AtomicBool>,
}

impl Encoder {
    fn start(
        size: (u32, u32),
        pixel_format: &'static str,
        sound: Sound,
        path: PathBuf,
        mut tx: mpsc::Sender<Event>,
    ) -> Self {
        let encoder = Self {
            size,
            frames: Arc::default(),
            stop: Arc::default(),
        };
        let (frames, stop) = (encoder.frames.clone(), encoder.stop.clone());
        let spawned = std::thread::Builder::new()
            .name("encoder".into())
            .spawn(move || {
                let event = match encode(size, pixel_format, &sound, &path, &frames, &stop) {
                    Ok(()) => Event::Recorded(path),
                    Err(err) => Event::Failed(err),
                };
                let _ = block_on(tx.send(event));
            });
        if let Err(err) = spawned {
            tracing::error!("capture: no encoder thread: {err}");
        }
        encoder
    }

    /// A frame filled by `fill`, made the newest.
    fn put(&self, fill: impl FnOnce(&mut Vec<u8>)) {
        let mut frame = self
            .frames
            .lock()
            .ok()
            .and_then(|mut frames| frames.spare.pop())
            .unwrap_or_default();
        fill(&mut frame);
        if let Ok(mut frames) = self.frames.lock()
            && let Some(unsent) = frames.newest.replace(frame)
        {
            frames.spare.push(unsent);
        }
    }

    fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn encode(
    size: (u32, u32),
    pixel_format: &str,
    sound: &Sound,
    path: &Path,
    frames: &Mutex<Frames>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let device = render_node().ok_or("no GPU to encode on")?;
    // Held for the recording, gone with it.
    let app_sink = sound.app.clone().map(AppSink::start).transpose()?;
    let sources: Vec<String> = [
        sound.desktop.then(|| "@DEFAULT_MONITOR@".to_owned()),
        app_sink.as_ref().map(AppSink::monitor),
        sound.mic.then(|| "@DEFAULT_SOURCE@".to_owned()),
    ]
    .into_iter()
    .flatten()
    .collect();
    let video_size = format!("{}x{}", size.0, size.1);
    let rate = FPS.to_string();
    let mut ffmpeg = Process::new("ffmpeg");
    ffmpeg.args(["-hide_banner", "-loglevel", "error", "-y"]);
    if !sources.is_empty() {
        // Frames stamped as they come, on the clock PulseAudio's are, so
        // `-isync` lines the sound up with them.
        ffmpeg.args(["-use_wallclock_as_timestamps", "1"]);
    }
    ffmpeg.args([
        "-f",
        "rawvideo",
        "-pix_fmt",
        pixel_format,
        "-video_size",
        &video_size,
        "-framerate",
        &rate,
        "-i",
        "-",
    ]);
    for source in &sources {
        ffmpeg.args([
            "-isync",
            "0",
            "-thread_queue_size",
            "1024",
            "-f",
            "pulse",
            "-i",
            source,
        ]);
    }
    ffmpeg.args(["-map", "0:v"]);
    let mix = (1..=sources.len())
        .map(|i| format!("[{i}:a]"))
        .collect::<String>()
        + &format!("amix=inputs={}:normalize=0[a]", sources.len());
    match sources.len() {
        0 => {}
        1 => _ = ffmpeg.args(["-map", "1:a"]),
        _ => _ = ffmpeg.args(["-filter_complex", &mix, "-map", "[a]"]),
    }
    // Limited-range BT.709, as every player reads it: left alone, the GPU
    // makes full-range YUV tagged as RGB, which players show far too dark.
    ffmpeg.args([
        "-vaapi_device",
        &device,
        "-vf",
        "hwupload,scale_vaapi=format=nv12:out_range=limited:out_color_matrix=bt709",
    ]);
    ffmpeg.args([
        "-c:v",
        "h264_vaapi",
        "-qp",
        "20",
        "-color_range",
        "tv",
        "-colorspace",
        "bt709",
    ]);
    if !sources.is_empty() {
        // The video's end (stdin closed) ends it: the sound never would.
        ffmpeg.args([
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-fps_mode",
            "cfr",
            "-r",
            &rate,
            "-shortest",
        ]);
    }
    let mut child = ffmpeg
        .args(["-movflags", "+faststart"])
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot start ffmpeg: {err}"))?;
    let mut stdin = child.stdin.take().expect("piped");
    let take = || {
        frames
            .lock()
            .ok()
            .and_then(|mut frames| frames.newest.take())
    };
    let give_back = |frame: Vec<u8>| {
        if let Ok(mut frames) = frames.lock() {
            frames.spare.push(frame);
        }
    };
    let written = (|| -> std::io::Result<()> {
        // Nothing until the first frame.
        let mut frame = loop {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            if let Some(frame) = take() {
                break frame;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let start = Instant::now();
        let step = Duration::from_secs(1) / FPS;
        let mut frames = 0u32;
        while !stop.load(Ordering::Relaxed) {
            if let Some(wait) = (start + step * frames).checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
            if let Some(newer) = take() {
                give_back(std::mem::replace(&mut frame, newer));
            }
            stdin.write_all(&frame)?;
            frames += 1;
        }
        tracing::info!("capture: {frames} frames in {:.2?}", start.elapsed());
        Ok(())
    })();
    drop(stdin);
    let finished = child
        .wait_with_output()
        .map_err(|err| format!("ffmpeg: {err}"))?;
    if !finished.status.success() {
        return Err(format!(
            "ffmpeg: {}",
            String::from_utf8_lossy(&finished.stderr).trim()
        ));
    }
    written.map_err(|err| format!("writing to ffmpeg: {err}"))
}

/// The integrated GPU's render node: the first one NVIDIA doesn't drive, as
/// VAAPI can't use that card and opening it would wake it.
fn render_node() -> Option<String> {
    let mut nodes: Vec<String> = std::fs::read_dir("/sys/class/drm")
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("renderD"))
        .collect();
    nodes.sort();
    nodes
        .into_iter()
        .find(|node| {
            std::fs::read_link(format!("/sys/class/drm/{node}/device/driver"))
                .is_ok_and(|driver| driver.file_name().is_some_and(|name| name != "nvidia"))
        })
        .map(|node| format!("/dev/dri/{node}"))
}

/// The apps with a sound stream open (playing or paused), by their name in
/// the registry: what `Sound::app` takes.
pub fn audio_apps() -> Vec<String> {
    let mut apps: Vec<String> = pw_dump()
        .iter()
        .filter(|o| is_app_stream(o))
        .map(|o| app_name(props(o)))
        .collect();
    apps.sort();
    apps.dedup();
    apps
}

fn pw_dump() -> Vec<Value> {
    Process::new("pw-dump")
        .output()
        .ok()
        .and_then(|output| serde_json::from_slice(&output.stdout).ok())
        .unwrap_or_default()
}

fn props(object: &Value) -> &Value {
    &object["info"]["props"]
}

/// A stream an app plays its sound through.
fn is_app_stream(object: &Value) -> bool {
    object["type"] == "PipeWire:Interface:Node"
        && props(object)["media.class"] == "Stream/Output/Audio"
}

fn app_name(props: &Value) -> String {
    let raw = [
        "application.name",
        "application.process.binary",
        "node.name",
    ]
    .into_iter()
    .find_map(|key| props[key].as_str())
    .unwrap_or_default();
    crate::ui::registry::name(raw)
}

/// A sink of our own that one app's streams also play into, besides the
/// speakers: its monitor is that app's sound alone, silence while it's
/// quiet or paused (recording the app's stream itself would hang until it
/// plays). pw-cli holds the sink and dies with the thread that started it,
/// so it never outlives the recording.
struct AppSink {
    name: String,
    holder: std::process::Child,
    done: Arc<AtomicBool>,
}

impl AppSink {
    fn start(app: String) -> Result<Self, String> {
        let name = format!("cabin-capture-{}", std::process::id());
        let mut command = Process::new("pw-cli");
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: prctl is async-signal-safe and touches nothing of ours.
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let mut holder = command
            .spawn()
            .map_err(|err| format!("cannot start pw-cli: {err}"))?;
        let create = format!(
            "create-node adapter {{ factory.name=support.null-audio-sink node.name={name} node.description=\"Screen recording\" media.class=Audio/Sink object.linger=false audio.position=[FL FR] }}\n"
        );
        holder
            .stdin
            .as_mut()
            .expect("piped")
            .write_all(create.as_bytes())
            .map_err(|err| format!("pw-cli: {err}"))?;
        let sink = Self {
            name,
            holder,
            done: Arc::default(),
        };
        // ffmpeg can only open it once it's there.
        let deadline = Instant::now() + Duration::from_secs(3);
        while !link(&app, &sink.name) {
            if Instant::now() > deadline {
                return Err("the app's sound sink never came up".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // Streams the app opens later (a new song, a new tab) join as they come.
        let (name, done) = (sink.name.clone(), sink.done.clone());
        let _ = std::thread::Builder::new()
            .name("app-sound".into())
            .spawn(move || {
                while !done.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_secs(1));
                    link(&app, &name);
                }
            });
        Ok(sink)
    }

    fn monitor(&self) -> String {
        format!("{}.monitor", self.name)
    }
}

impl Drop for AppSink {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
        let _ = self.holder.kill();
        let _ = self.holder.wait();
    }
}

/// The app's streams linked into the sink, channel to channel (mono to
/// both); whether the sink is up, its ports and all (they come a moment
/// after the node).
fn link(app: &str, sink: &str) -> bool {
    let objects = pw_dump();
    let Some(sink_id) = objects
        .iter()
        .find(|o| o["type"] == "PipeWire:Interface:Node" && props(o)["node.name"] == sink)
        .and_then(|o| o["id"].as_u64())
    else {
        return false;
    };
    let streams: HashSet<u64> = objects
        .iter()
        .filter(|o| is_app_stream(o) && app_name(props(o)) == app)
        .filter_map(|o| o["id"].as_u64())
        .collect();
    let linked: HashSet<(u64, u64)> = objects
        .iter()
        .filter(|o| o["type"] == "PipeWire:Interface:Link")
        .filter_map(|o| {
            Some((
                o["info"]["output-port-id"].as_u64()?,
                o["info"]["input-port-id"].as_u64()?,
            ))
        })
        .collect();
    let ports = |direction: &'static str| {
        objects
            .iter()
            .filter(move |o| {
                o["type"] == "PipeWire:Interface:Port" && o["info"]["direction"] == direction
            })
            .filter_map(|o| {
                Some((
                    props(o)["node.id"].as_u64()?,
                    props(o)["audio.channel"].as_str()?,
                    o["id"].as_u64()?,
                ))
            })
    };
    let inputs: Vec<(&str, u64)> = ports("input")
        .filter(|(node, ..)| *node == sink_id)
        .map(|(_, channel, id)| (channel, id))
        .collect();
    if inputs.is_empty() {
        return false;
    }
    for (_, channel, output) in ports("output").filter(|(node, ..)| streams.contains(node)) {
        for &(_, input) in inputs
            .iter()
            .filter(|(c, _)| *c == channel || channel == "MONO")
        {
            if !linked.contains(&(output, input)) {
                tracing::debug!("capture: {app}'s port {output} into {sink}'s {input}");
                let _ = Process::new("pw-link")
                    .args([output.to_string(), input.to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }
    true
}

impl ScreencopyHandler for State {
    fn screencopy_state(&mut self) -> &mut ScreencopyState {
        &mut self.screencopy
    }

    fn init_done(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        session: &CaptureSession,
        formats: &Formats,
    ) {
        let Some(job) = self.jobs.get_mut(session) else {
            return;
        };
        let (width, height) = formats.buffer_size;
        let Some(format) = READABLE
            .into_iter()
            .find(|f| formats.shm_formats.contains(f))
        else {
            return self.end(
                session,
                &format!(
                    "none of the shared-memory formats {:?} is one we read",
                    formats.shm_formats
                ),
            );
        };
        if job
            .buffer
            .as_ref()
            .is_none_or(|b| (b.width, b.height) != (width, height))
        {
            // The old one goes first, so its pool is never mapped twice.
            job.buffer = None;
            match Buffer::new(&self.shm, width, height, format, &self.qh) {
                Ok(buffer) => job.buffer = Some(buffer),
                Err(err) => return self.end(session, &err),
            }
        }
        job.fresh = true;
        if let Kind::Record(recording) = &mut job.kind
            && recording.encoder.is_none()
        {
            let (w, h) = recording
                .crop
                .map_or((width, height), |c| (c.width as u32, c.height as u32));
            // H.264's 4:2:0 wants even sides, and the GPU's encoder 128 at
            // least: a smaller region is padded with black.
            let size = ((w & !1).max(128), (h & !1).max(128));
            let pixel_format = if bgr(format) { "bgr0" } else { "rgb0" };
            recording.encoder = Some(Encoder::start(
                size,
                pixel_format,
                recording.sound.clone(),
                recording.path.clone(),
                self.tx.clone(),
            ));
        }
        self.capture(session);
    }

    fn stopped(&mut self, _: &Connection, _: &QueueHandle<Self>, session: &CaptureSession) {
        self.end(session, "it went away");
    }

    fn ready(&mut self, _: &Connection, _: &QueueHandle<Self>, frame: &CaptureFrame, _: Frame) {
        let Some(session) = frame.session::<ScreencopyFrameData>() else {
            return;
        };
        let Some(job) = self.jobs.get_mut(&session) else {
            return;
        };
        job.pending = false;
        let Some(buffer) = job.buffer.as_mut() else {
            return;
        };
        match &mut job.kind {
            Kind::Still(still) => {
                let image = buffer.rgba();
                let event = match still {
                    Still::Output(name) => Event::Shot(Shot {
                        name: name.clone(),
                        image: Arc::new(image),
                    }),
                    Still::Window(id) => Event::Window(Shot {
                        name: id.clone(),
                        image: Arc::new(image),
                    }),
                    Still::Thumb(id) => Event::Thumb(Shot {
                        name: id.clone(),
                        image: Arc::new(shrink(&image)),
                    }),
                };
                self.jobs.remove(&session);
                self.send(event);
            }
            Kind::Record(recording) => {
                if let Some(encoder) = &recording.encoder {
                    let size = (buffer.width, buffer.height);
                    encoder.put(|frame| {
                        cut(
                            buffer.pool.mmap(),
                            size,
                            recording.crop,
                            encoder.size,
                            frame,
                        )
                    });
                }
                let step = Duration::from_secs(1) / FPS;
                let wait = recording.asked.map_or(Duration::ZERO, |asked| {
                    (asked + step).saturating_duration_since(Instant::now())
                });
                self.later(session, wait);
            }
        }
    }

    fn failed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        frame: &CaptureFrame,
        reason: WEnum<FailureReason>,
    ) {
        let Some(session) = frame.session::<ScreencopyFrameData>() else {
            return;
        };
        let Some(job) = self.jobs.get_mut(&session) else {
            return;
        };
        job.pending = false;
        match reason {
            // The size changed: init_done brings the new one and asks again,
            // unless it came first.
            WEnum::Value(FailureReason::BufferConstraints) => {
                if job.fresh {
                    self.later(session, Duration::ZERO);
                }
            }
            reason => self.end(&session, &format!("{reason:?}")),
        }
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl Dispatch<WlBuffer, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlBuffer,
        _: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    sctk::registry_handlers![OutputState];
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}
}

impl ToplevelInfoHandler for State {
    fn toplevel_info_state(&mut self) -> &mut ToplevelInfoState {
        &mut self.toplevels
    }
    fn new_toplevel(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ExtForeignToplevelHandleV1,
    ) {
        self.windows_changed();
    }
    fn update_toplevel(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ExtForeignToplevelHandleV1,
    ) {
        self.windows_changed();
    }
    fn toplevel_closed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ExtForeignToplevelHandleV1,
    ) {
        self.windows_changed();
    }
}

impl WorkspaceHandler for State {
    fn workspace_state(&mut self) -> &mut WorkspaceState {
        &mut self.workspaces
    }
    fn done(&mut self) {
        self.windows_changed();
    }
}

cctk::delegate_screencopy!(State);
cctk::delegate_toplevel_info!(State);
cctk::delegate_workspace!(State);
sctk::delegate_shm!(State);
sctk::delegate_output!(State);
sctk::delegate_registry!(State);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cut_crops_and_pads() {
        // 4×2 picture, each pixel's bytes its index.
        let source: Vec<u8> = (0..8u8).flat_map(|i| [i; 4]).collect();
        let mut out = Vec::new();
        cut(
            &source,
            (4, 2),
            Some(Rect {
                x: 1,
                y: 1,
                width: 2,
                height: 1,
            }),
            (2, 1),
            &mut out,
        );
        assert_eq!(out, [[5; 4], [6; 4]].concat());
        // Past the right and bottom edges: black, whatever the reused frame held.
        cut(
            &source,
            (4, 2),
            Some(Rect {
                x: 3,
                y: 1,
                width: 2,
                height: 2,
            }),
            (2, 2),
            &mut out,
        );
        assert_eq!(out, [[7; 4], [0; 4], [0; 4], [0; 4]].concat());
    }

    /// Against the running compositor: a still of every output, then a
    /// second of part of the first output recorded, and of the first window.
    /// Run with run-dev.env sourced and ffmpeg on PATH:
    /// `cargo test --lib -- --ignored records_a_second`.
    #[test]
    #[ignore = "needs the session's compositor, a GPU and ffmpeg"]
    fn records_a_second() {
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        let (commands, receiver) = calloop::channel::sync_channel(8);
        let (tx, mut rx) = mpsc::channel(8);
        std::thread::spawn(move || run(receiver, tx));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let mut next = || {
            runtime
                .block_on(async { tokio::time::timeout(Duration::from_secs(5), rx.next()).await })
                .expect("an event within 5 s")
                .expect("the thread alive")
        };
        // The windows fill in over a few lists.
        let (mut output, mut window) = (None, None);
        while output.is_none() || window.is_none() {
            match next() {
                Event::Shot(shot) => output = Some(shot.name),
                Event::Windows(windows) => window = windows.first().map(|w| w.id.clone()),
                Event::Failed(err) => panic!("{err}"),
                _ => {}
            }
        }
        let window = window.unwrap();
        // The window whole, then every window small.
        commands.send(Command::Shoot(window.clone())).unwrap();
        commands.send(Command::Thumbnails).unwrap();
        let (mut whole, mut thumb) = (false, false);
        while !(whole && thumb) {
            match next() {
                Event::Window(shot) => whole = shot.name == window && shot.image.width() > 0,
                Event::Thumb(shot) => {
                    thumb |= shot.image.width() <= 480 && shot.image.height() <= 300
                }
                Event::Failed(err) => panic!("{err}"),
                _ => {}
            }
        }
        let crop = Some(Rect {
            x: 10,
            y: 10,
            width: 320,
            height: 200,
        });
        // The output with the desktop's sound (silence will do), the window without.
        let desktop = Sound {
            desktop: true,
            ..Sound::default()
        };
        let targets = [
            ("output", Target::Output(output.unwrap()), crop, desktop),
            ("window", Target::Window(window), None, Sound::default()),
        ];
        for (name, target, crop, sound) in targets {
            let path = std::env::temp_dir().join(format!("cabin-capture-test-{name}.mp4"));
            commands
                .send(Command::Record {
                    target,
                    crop,
                    sound,
                    path: path.clone(),
                })
                .unwrap();
            std::thread::sleep(Duration::from_secs(1));
            commands.send(Command::Stop).unwrap();
            loop {
                match next() {
                    Event::Recorded(saved) => break assert_eq!(saved, path),
                    Event::Failed(err) => panic!("{name}: {err}"),
                    _ => {}
                }
            }
            assert!(std::fs::metadata(&path).unwrap().len() > 0, "{name}: empty");
        }
    }

    /// One app's sound, alone: a tone played by a made-up app into a silent
    /// sink of the test's own (nothing reaches the speakers), recorded with
    /// the first output. Run like `records_a_second`.
    #[test]
    #[ignore = "needs the session's compositor, PipeWire, a GPU and ffmpeg"]
    fn records_an_apps_sound() {
        struct Speaker(String);
        impl Drop for Speaker {
            fn drop(&mut self) {
                let _ = Process::new("pactl").args(["unload-module", &self.0]).status();
            }
        }
        let dir = std::env::temp_dir();
        let (tone, path) = (dir.join("cabin-capture-tone.wav"), dir.join("cabin-capture-test-app.mp4"));
        let made = Process::new("ffmpeg").args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=8"]).arg(&tone).status();
        assert!(made.unwrap().success(), "no tone");
        let module = Process::new("pactl").args(["load-module", "module-null-sink", "sink_name=cabin_capture_test"]).output().unwrap();
        let _speaker = Speaker(String::from_utf8_lossy(&module.stdout).trim().to_owned());
        let mut player = Process::new("pw-play").args(["--target", "cabin_capture_test", "-P", "{ application.name = \"CabinTone\" }"]).arg(&tone).spawn().unwrap();
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(audio_apps().iter().filter(|a| *a == "CabinTone").count(), 1, "the tone's app isn't listed");

        let (commands, receiver) = calloop::channel::sync_channel(8);
        let (tx, mut rx) = mpsc::channel(8);
        std::thread::spawn(move || run(receiver, tx));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let mut next = || runtime.block_on(async { tokio::time::timeout(Duration::from_secs(10), rx.next()).await }).expect("an event within 10 s").expect("the thread alive");
        let output = loop {
            if let Event::Shot(shot) = next() {
                break shot.name;
            }
        };
        let sound = Sound { app: Some("CabinTone".into()), ..Sound::default() };
        let crop = Some(Rect { x: 0, y: 0, width: 320, height: 200 });
        commands.send(Command::Record { target: Target::Output(output), crop, sound, path: path.clone() }).unwrap();
        std::thread::sleep(Duration::from_secs(2));
        commands.send(Command::Stop).unwrap();
        loop {
            match next() {
                Event::Recorded(_) => break,
                Event::Failed(err) => panic!("{err}"),
                _ => {}
            }
        }
        let _ = player.kill();
        // The tone is about -21 dB; silence would be -91.
        let measured = Process::new("ffmpeg").args(["-v", "info", "-i"]).arg(&path).args(["-af", "volumedetect", "-f", "null", "-"]).output().unwrap();
        let log = String::from_utf8_lossy(&measured.stderr);
        let mean: f32 = log.split("mean_volume: ").nth(1).and_then(|s| s.split(' ').next()).and_then(|s| s.parse().ok()).expect("an audio stream");
        assert!(mean > -30.0, "the app's sound is missing: mean {mean} dB");
    }
}
