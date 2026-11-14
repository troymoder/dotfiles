// SPDX-License-Identifier: GPL-3.0-only

//! Live pictures of every window and workspace, kept on the GPU, for the
//! overview and the switcher. The compositor draws each into buffers of
//! ours (dmabufs from GBM, on the device it names for the capture), and the
//! views hand those back to it as subsurfaces, which it scales itself:
//! nothing passes through the CPU, so the pictures are full size and as
//! fresh as the compositor's frames. Two buffers a picture: one fills while
//! the other shows, and a buffer fills again only once the compositor has
//! let go of it.
//!
//! The thread runs from the daemon's start, connected and with the GPU
//! device open (opening it took ~70 ms), but takes pictures only while
//! told something shows them: then sessions and buffers come and go.

use std::{
    collections::HashMap,
    fs,
    os::{fd::AsFd, unix::fs::MetadataExt},
    sync::Arc,
    time::Duration,
};

use cosmic::{
    cctk::{
        self,
        screencopy::{
            CaptureFrame, CaptureOptions, CaptureSession, CaptureSource, FailureReason, Formats,
            Frame, Rect, ScreencopyFrameData, ScreencopyHandler, ScreencopySessionData,
            ScreencopyState,
        },
        sctk::{
            self,
            dmabuf::{DmabufFeedback, DmabufHandler, DmabufState},
            output::{OutputHandler, OutputState},
            reexports::{
                calloop::{
                    self, LoopHandle,
                    timer::{TimeoutAction, Timer},
                },
                calloop_wayland_source::WaylandSource,
                protocols::wp::linux_dmabuf::zv1::client::{
                    zwp_linux_buffer_params_v1::{self, ZwpLinuxBufferParamsV1},
                    zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
                },
            },
            registry::{ProvidesRegistryState, RegistryState},
        },
        toplevel_info::{ToplevelInfoHandler, ToplevelInfoState},
        wayland_client::{
            Connection, QueueHandle, WEnum,
            globals::registry_queue_init,
            protocol::{wl_buffer::WlBuffer, wl_output::WlOutput},
        },
        wayland_protocols::ext::{
            foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
            workspace::v1::client::ext_workspace_handle_v1::State as WorkspaceFlags,
        },
        workspace::{WorkspaceHandler, WorkspaceState},
    },
    iced::{
        Subscription,
        platform_specific::shell::subsurface_widget::{
            BufferSource, Dmabuf, Plane, SubsurfaceBuffer, SubsurfaceBufferRelease,
        },
        stream,
    },
};
use futures::{FutureExt, SinkExt, StreamExt, channel::mpsc, executor::block_on};

use crate::backend::windows::workspace_key;

/// At most how often a picture is taken again: cosmic-comp answers each
/// ask at once, changed or not.
const STEP: Duration = Duration::from_millis(33);
/// How soon to look again for a buffer the compositor still shows.
const HELD: Duration = Duration::from_millis(8);
/// The formats asked for, in order: with alpha, so rounded corners stay round.
const FORMATS: [gbm::Format; 4] = [
    gbm::Format::Argb8888,
    gbm::Format::Abgr8888,
    gbm::Format::Xrgb8888,
    gbm::Format::Xbgr8888,
];

/// What the thread reports.
#[derive(Clone, Debug)]
pub enum Event {
    /// Its ear: whether pictures are wanted now.
    Ready(calloop::channel::SyncSender<bool>),
    /// A new picture of a window (by its id) or a workspace (by
    /// `workspace_key`), to show as a subsurface.
    Picture(String, SubsurfaceBuffer),
}

pub fn subscription() -> Subscription<Event> {
    Subscription::run(|| {
        stream::channel(16, |mut output: mpsc::Sender<Event>| async move {
            let (commands, receiver) = calloop::channel::sync_channel(4);
            let (tx, mut rx) = mpsc::channel(16);
            if let Err(err) = std::thread::Builder::new()
                .name("live".into())
                .spawn(move || run(receiver, tx))
            {
                tracing::error!("live: no thread: {err}");
                return futures::future::pending().await;
            }
            let _ = output.send(Event::Ready(commands)).await;
            while let Some(event) = rx.next().await {
                if output.send(event).await.is_err() {
                    return;
                }
            }
            futures::future::pending().await
        })
    })
}

fn run(commands: calloop::channel::Channel<bool>, tx: mpsc::Sender<Event>) {
    let connection = match Connection::connect_to_env() {
        Ok(connection) => connection,
        Err(err) => return tracing::warn!("live: no wayland connection: {err}"),
    };
    let (globals, queue) = match registry_queue_init::<State>(&connection) {
        Ok(init) => init,
        Err(err) => return tracing::warn!("live: no registry: {err}"),
    };
    let mut event_loop = match calloop::EventLoop::<State>::try_new() {
        Ok(event_loop) => event_loop,
        Err(err) => return tracing::error!("live: no event loop: {err}"),
    };
    let qh = queue.handle();
    let registry = RegistryState::new(&globals);
    // Outputs before the workspaces, which name them.
    let outputs = OutputState::new(&globals, &qh);
    let workspaces = WorkspaceState::new(&registry, &qh);
    let Some(toplevels) = ToplevelInfoState::try_new(&registry, &qh) else {
        return tracing::warn!("live: the compositor lists no windows");
    };
    // The default feedback names the compositor's device: opened at once.
    let dmabuf = DmabufState::new(&globals, &qh);
    if let Err(err) = dmabuf.get_default_feedback(&qh) {
        tracing::debug!("live: no dmabuf feedback to warm up with: {err}");
    }
    let mut state = State {
        on: false,
        outputs,
        workspaces,
        toplevels,
        screencopy: ScreencopyState::new(&globals, &qh),
        dmabuf,
        registry,
        gbm: None,
        qh,
        handle: event_loop.handle(),
        tx,
        jobs: HashMap::new(),
        taking: Vec::new(),
    };
    if WaylandSource::new(connection, queue)
        .insert(event_loop.handle())
        .is_err()
    {
        return tracing::error!("live: cannot watch the wayland socket");
    }
    let inserted = event_loop
        .handle()
        .insert_source(commands, |event, (), state| match event {
            calloop::channel::Event::Msg(on) => {
                state.on = on;
                state.sync();
            }
            calloop::channel::Event::Closed => state.on = false,
        });
    if inserted.is_err() {
        return tracing::error!("live: cannot watch for commands");
    }
    // Woken now and then to see whether anyone still listens.
    while !state.tx.is_closed() {
        if let Err(err) = event_loop.dispatch(Some(Duration::from_millis(250)), &mut state) {
            return tracing::error!("live: {err}");
        }
    }
}

struct State {
    /// Pictures wanted.
    on: bool,
    registry: RegistryState,
    outputs: OutputState,
    workspaces: WorkspaceState,
    toplevels: ToplevelInfoState,
    screencopy: ScreencopyState,
    dmabuf: DmabufState,
    /// The render device, by its number, once opened.
    gbm: Option<(u64, gbm::Device<fs::File>)>,
    qh: QueueHandle<State>,
    handle: LoopHandle<'static, State>,
    tx: mpsc::Sender<Event>,
    jobs: HashMap<CaptureSession, Job>,
    /// What's being taken: each picture's key, its source, its session.
    taking: Vec<(String, CaptureSource, CaptureSession)>,
}

struct Job {
    key: String,
    /// Two once the compositor has said what it wants.
    buffers: Vec<Buffer>,
    /// The one to fill next.
    back: usize,
    /// Each buffer's last showing, until the compositor lets go of it.
    shown: [Option<SubsurfaceBufferRelease>; 2],
    /// A frame asked for and not yet in.
    pending: bool,
}

struct Buffer {
    backing: Arc<BufferSource>,
    buffer: WlBuffer,
    size: (u32, u32),
}

impl Drop for Buffer {
    fn drop(&mut self) {
        self.buffer.destroy();
    }
}

impl State {
    /// A session for each window and workspace, by its key now: windows by
    /// id; workspaces by output and name, which cosmic-comp renumbers as
    /// empty ones go, so a name gone to another workspace starts afresh.
    fn sync(&mut self) {
        let mut wanted: Vec<(String, CaptureSource)> = self
            .toplevels
            .toplevels()
            .filter(|_| self.on)
            .map(|t| {
                (
                    t.identifier.clone(),
                    CaptureSource::Toplevel(t.foreign_toplevel.clone()),
                )
            })
            .collect();
        for group in self.workspaces.workspace_groups().filter(|_| self.on) {
            let Some(output) = group
                .outputs
                .first()
                .and_then(|o| self.outputs.info(o))
                .and_then(|info| info.name)
            else {
                continue;
            };
            wanted.extend(
                group
                    .workspaces
                    .iter()
                    .filter_map(|handle| self.workspaces.workspace_info(handle))
                    .filter(|w| !w.state.contains(WorkspaceFlags::Hidden))
                    .map(|w| {
                        (
                            workspace_key(&output, &w.name),
                            CaptureSource::Workspace(w.handle.clone()),
                        )
                    }),
            );
        }
        let (kept, gone): (Vec<_>, Vec<_>) = std::mem::take(&mut self.taking)
            .into_iter()
            .partition(|(key, source, _)| wanted.iter().any(|(k, s)| k == key && s == source));
        for (.., session) in gone {
            self.jobs.remove(&session);
        }
        self.taking = kept;
        for (key, source) in wanted {
            if self.taking.iter().any(|(k, ..)| *k == key) {
                continue;
            }
            match self.screencopy.capturer().create_session(
                &source,
                CaptureOptions::empty(),
                &self.qh,
                ScreencopySessionData::default(),
            ) {
                Ok(session) => {
                    self.jobs.insert(
                        session.clone(),
                        Job {
                            key: key.clone(),
                            buffers: Vec::new(),
                            back: 0,
                            shown: [None, None],
                            pending: false,
                        },
                    );
                    self.taking.push((key, source, session));
                }
                Err(err) => tracing::warn!("live: no picture of {key}: {err}"),
            }
        }
    }

    fn end(&mut self, session: &CaptureSession) {
        self.jobs.remove(session);
        self.taking.retain(|(.., s)| s != session);
    }

    /// The next frame into the back buffer, once the compositor no longer
    /// shows it.
    fn capture(&mut self, session: &CaptureSession) {
        let Some(job) = self.jobs.get_mut(session) else {
            return;
        };
        if job.pending || job.buffers.len() < 2 {
            return;
        }
        if let Some(shown) = &mut job.shown[job.back] {
            if shown.now_or_never().is_none() {
                return self.later(session.clone(), HELD);
            }
            job.shown[job.back] = None;
        }
        job.pending = true;
        let back = &job.buffers[job.back];
        let whole = Rect {
            x: 0,
            y: 0,
            width: back.size.0 as i32,
            height: back.size.1 as i32,
        };
        session.capture(
            &back.buffer,
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

    /// A buffer on the GPU the compositor offers to draw into.
    fn buffer(&mut self, formats: &Formats) -> Result<Buffer, String> {
        let (format, modifiers) = FORMATS
            .into_iter()
            .find_map(|f| {
                formats
                    .dmabuf_formats
                    .iter()
                    .find(|(code, _)| *code == f as u32)
                    .map(|(_, modifiers)| (f, modifiers))
            })
            .ok_or_else(|| format!("no GPU format we know in {:?}", formats.dmabuf_formats))?;
        let device = formats
            .dmabuf_device
            .ok_or("the compositor named no device to draw on")?;
        if self.gbm.as_ref().is_none_or(|(d, _)| *d != device) {
            self.gbm = Some((device, open_device(device)?));
        }
        let gbm = &self.gbm.as_ref().expect("opened above").1;
        let (width, height) = formats.buffer_size;
        let modifiers: Vec<gbm::Modifier> = modifiers.iter().map(|m| (*m).into()).collect();
        let bo = if modifiers.iter().all(|m| *m == gbm::Modifier::Invalid) {
            gbm.create_buffer_object::<()>(
                width,
                height,
                format,
                gbm::BufferObjectFlags::RENDERING,
            )
        } else {
            gbm.create_buffer_object_with_modifiers::<()>(
                width,
                height,
                format,
                modifiers.into_iter(),
            )
        }
        .map_err(|err| format!("GBM: {err}"))?;
        let params = self
            .dmabuf
            .create_params(&self.qh)
            .map_err(|err| format!("dmabuf: {err}"))?;
        let modifier: u64 = bo.modifier().into();
        let mut planes = Vec::new();
        for i in 0..bo.plane_count() as i32 {
            let fd = bo
                .fd_for_plane(i)
                .map_err(|err| format!("GBM plane {i}: {err}"))?;
            let (offset, stride) = (bo.offset(i), bo.stride_for_plane(i));
            params.add(fd.as_fd(), i as u32, offset, stride, modifier);
            planes.push(Plane {
                fd,
                plane_idx: i as u32,
                offset,
                stride,
            });
        }
        let (buffer, _) = params.create_immed(
            width as i32,
            height as i32,
            format as u32,
            zwp_linux_buffer_params_v1::Flags::empty(),
            &self.qh,
        );
        Ok(Buffer {
            backing: Arc::new(
                Dmabuf {
                    width: width as i32,
                    height: height as i32,
                    planes,
                    format: format as u32,
                    modifier,
                }
                .into(),
            ),
            buffer,
            size: (width, height),
        })
    }
}

/// The DRM node with this device number. Only that one is opened: opening
/// the NVIDIA card would wake it.
fn open_device(device: u64) -> Result<gbm::Device<fs::File>, String> {
    let path = fs::read_dir("/dev/dri")
        .map_err(|err| format!("/dev/dri: {err}"))?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| fs::metadata(path).is_ok_and(|m| m.rdev() == device))
        .ok_or_else(|| format!("no DRM node for device {device}"))?;
    let file = fs::File::options()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    gbm::Device::new(file).map_err(|err| format!("GBM on {}: {err}", path.display()))
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
        let Some(job) = self.jobs.get(session) else {
            return;
        };
        let key = job.key.clone();
        // New buffers when the size is new: a window resized.
        if job
            .buffers
            .first()
            .is_none_or(|b| b.size != formats.buffer_size)
        {
            let buffers = match (self.buffer(formats), self.buffer(formats)) {
                (Ok(a), Ok(b)) => vec![a, b],
                (Err(err), _) | (_, Err(err)) => {
                    tracing::warn!("live: no buffer for {key}: {err}");
                    return self.end(session);
                }
            };
            let Some(job) = self.jobs.get_mut(session) else {
                return;
            };
            job.buffers = buffers;
            job.shown = [None, None];
            job.back = 0;
        }
        self.capture(session);
    }

    fn stopped(&mut self, _: &Connection, _: &QueueHandle<Self>, session: &CaptureSession) {
        self.end(session);
    }

    fn ready(&mut self, _: &Connection, _: &QueueHandle<Self>, frame: &CaptureFrame, _: Frame) {
        let Some(session) = frame.session::<ScreencopyFrameData>() else {
            return;
        };
        let Some(job) = self.jobs.get_mut(&session) else {
            return;
        };
        job.pending = false;
        let Some(filled) = job.buffers.get(job.back) else {
            return;
        };
        let (picture, release) = SubsurfaceBuffer::new(filled.backing.clone());
        job.shown[job.back] = Some(release);
        job.back ^= 1;
        let key = job.key.clone();
        if block_on(self.tx.send(Event::Picture(key, picture))).is_err() {
            return;
        }
        self.later(session, STEP);
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
        // The size changed: init_done brings the new one and asks again.
        if reason != WEnum::Value(FailureReason::BufferConstraints) {
            tracing::debug!("live: no picture of {}: {reason:?}", job.key);
            self.end(&session);
        }
    }
}

impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf
    }
    /// The compositor's device: opened now, and a first buffer made and
    /// dropped, so the driver is up before the first picture is wanted.
    fn dmabuf_feedback(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ZwpLinuxDmabufFeedbackV1,
        feedback: DmabufFeedback,
    ) {
        let device = feedback.main_device() as u64;
        if self.gbm.as_ref().is_some_and(|(d, _)| *d == device) {
            return;
        }
        match open_device(device) {
            Ok(gbm) => {
                let _ = gbm.create_buffer_object::<()>(
                    64,
                    64,
                    gbm::Format::Argb8888,
                    gbm::BufferObjectFlags::RENDERING,
                );
                self.gbm = Some((device, gbm));
            }
            Err(err) => tracing::warn!("live: {err}"),
        }
    }
    fn created(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ZwpLinuxBufferParamsV1,
        _: WlBuffer,
    ) {
    }
    fn failed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &ZwpLinuxBufferParamsV1) {}
    fn released(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlBuffer) {}
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
    // An output's name may come after the workspaces that name it.
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {
        self.sync();
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {
        self.sync();
    }
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
        self.sync();
    }
    fn update_toplevel(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ExtForeignToplevelHandleV1,
    ) {
    }
    fn toplevel_closed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ExtForeignToplevelHandleV1,
    ) {
        self.sync();
    }
}

impl WorkspaceHandler for State {
    fn workspace_state(&mut self) -> &mut WorkspaceState {
        &mut self.workspaces
    }
    fn done(&mut self) {
        self.sync();
    }
}

cctk::delegate_screencopy!(State);
cctk::delegate_toplevel_info!(State);
cctk::delegate_workspace!(State);
sctk::delegate_dmabuf!(State);
sctk::delegate_output!(State);
sctk::delegate_registry!(State);

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs a running COSMIC session: the pictures that come in 2 s, per
    /// window and workspace, each dropped at once as a view would replace it.
    #[test]
    #[ignore = "needs the session's compositor and GPU"]
    fn pictures_keep_coming() {
        let (commands, receiver) = calloop::channel::sync_channel(4);
        let (tx, mut rx) = mpsc::channel(16);
        std::thread::spawn(move || run(receiver, tx));
        commands.send(true).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let until = std::time::Instant::now() + Duration::from_secs(2);
        let mut counts: HashMap<String, (u32, (i32, i32))> = HashMap::new();
        let started = std::time::Instant::now();
        let mut first = None;
        while let Ok(Some(Event::Picture(key, buffer))) = runtime.block_on(async {
            tokio::time::timeout(until.saturating_duration_since(std::time::Instant::now()), rx.next())
                .await
        }) {
            first.get_or_insert(started.elapsed());
            if !counts.contains_key(&key) {
                println!("first picture of {key} after {:?}", started.elapsed());
            }
            let entry = counts.entry(key).or_default();
            entry.0 += 1;
            entry.1 = (buffer.width(), buffer.height());
        }
        for (key, (n, size)) in &counts {
            println!("{n:4} pictures {size:?} {key}");
        }
        println!("first of all after {first:?}");
        assert!(counts.values().any(|(n, _)| *n > 10), "frames keep coming");
    }
}
