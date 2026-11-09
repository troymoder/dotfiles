// SPDX-License-Identifier: GPL-3.0-only

//! The workspaces applet: a dot per workspace on the panel's output, the
//! active one a pill. A click opens the overview; the wheel switches.

use std::os::{
    fd::{FromRawFd, RawFd},
    unix::net::UnixStream,
};

use cosmic::{
    Element, Task, app,
    cctk::{
        self,
        sctk::{
            self,
            output::{OutputHandler, OutputState},
            reexports::{calloop, calloop_wayland_source::WaylandSource},
            registry::{ProvidesRegistryState, RegistryState},
        },
        wayland_client::{
            ConnectError, Connection, EventQueue, QueueHandle,
            globals::{GlobalList, registry_queue_init},
            protocol::wl_output::WlOutput,
        },
        wayland_protocols::ext::workspace::v1::client::ext_workspace_handle_v1::{ExtWorkspaceHandleV1, State as WorkspaceFlags},
        workspace::{Workspace, WorkspaceHandler, WorkspaceState},
    },
    iced::{self, Alignment, Background, Length, Subscription, mouse::ScrollDelta, stream},
    theme, widget,
};
use futures::{SinkExt, StreamExt, channel::mpsc, executor::block_on};

use crate::{backend, ui::palette};

/// A dot's diameter, and the active pill's length.
const DOT: f32 = 7.0;
const PILL: f32 = 30.0;
const GAP: u16 = 6;
/// Scrolling this far moves one workspace on.
const STEP: f32 = 30.0;

#[derive(Clone, Debug)]
pub enum Message {
    Wayland(Event),
    Open,
    Scroll(ScrollDelta),
    Done,
}

/// What the wayland thread reports.
#[derive(Clone, Debug)]
pub enum Event {
    Ready(calloop::channel::SyncSender<Command>),
    Workspaces(Vec<Workspace>),
    /// The thread that talks to the compositor could not start, or has ended.
    Unavailable,
}

/// What the wayland thread is asked to do.
#[derive(Clone, Debug)]
pub enum Command {
    Activate(ExtWorkspaceHandleV1),
}

pub struct WorkspacesApp {
    core: app::Core,
    /// Those shown, in order.
    workspaces: Vec<Workspace>,
    commands: Option<calloop::channel::SyncSender<Command>>,
    /// Scrolling not yet turned into a step, in pixels.
    scrolled: f32,
    /// The workspace asked for, until the compositor confirms a change.
    pending: Option<usize>,
}

impl WorkspacesApp {
    fn active(&self) -> Option<usize> {
        self.workspaces.iter().position(|w| w.state.contains(WorkspaceFlags::Active))
    }

    /// Moves `by` workspaces on (back when negative), stopping at the ends.
    fn step(&mut self, by: isize) {
        let Some(from) = self.pending.or_else(|| self.active()) else {
            return;
        };
        let last = self.workspaces.len() as isize - 1;
        let to = (from as isize + by).clamp(0, last) as usize;
        if to == from {
            return;
        }
        if let Some(commands) = &self.commands {
            let _ = commands.try_send(Command::Activate(self.workspaces[to].handle.clone()));
            self.pending = Some(to);
        }
    }
}

fn dot<'a>(active: bool, horizontal: bool) -> Element<'a, Message> {
    let (width, height) = match (active, horizontal) {
        (true, true) => (PILL, DOT),
        (true, false) => (DOT, PILL),
        (false, _) => (DOT, DOT),
    };
    let color = if active { iced::Color::WHITE } else { palette::alpha(iced::Color::WHITE, 0.55) };
    let radius = if active { 4.0 } else { DOT / 2.0 };
    widget::container(widget::space::horizontal())
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .class(theme::Container::custom(move |_| widget::container::Style {
            background: Some(Background::Color(color)),
            border: iced::Border { radius: radius.into(), ..Default::default() },
            ..Default::default()
        }))
        .into()
}

fn hover_style() -> theme::Button {
    fn style(hovered: bool) -> widget::button::Style {
        widget::button::Style {
            background: Some(Background::Color(if hovered { palette::alpha(iced::Color::WHITE, 0.16) } else { iced::Color::TRANSPARENT })),
            border_radius: 12.0.into(),
            ..widget::button::Style::new()
        }
    }
    theme::Button::Custom {
        active: Box::new(|_, _| style(false)),
        disabled: Box::new(|_| style(false)),
        hovered: Box::new(|_, _| style(true)),
        pressed: Box::new(|_, _| style(true)),
    }
}

impl cosmic::Application for WorkspacesApp {
    type Message = Message;
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    const APP_ID: &'static str = crate::WORKSPACES_ID;

    fn init(core: app::Core, _flags: ()) -> (Self, app::Task<Message>) {
        (Self { core, workspaces: Vec::new(), commands: None, scrolled: 0.0, pending: None }, Task::none())
    }

    fn core(&self) -> &app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut app::Core {
        &mut self.core
    }

    fn style(&self) -> Option<iced::theme::Style> {
        Some(cosmic::applet::style())
    }

    fn update(&mut self, message: Message) -> app::Task<Message> {
        match message {
            Message::Wayland(Event::Ready(commands)) => self.commands = Some(commands),
            Message::Wayland(Event::Workspaces(mut list)) => {
                list.retain(|w| !w.state.contains(WorkspaceFlags::Hidden));
                list.sort_by(|a, b| a.coordinates.cmp(&b.coordinates));
                self.workspaces = list;
                self.pending = None;
            }
            Message::Wayland(Event::Unavailable) => tracing::warn!("no workspace protocol; the applet stays empty"),
            Message::Open => {
                return Task::future(async {
                    backend::session::overview().await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::Done => {}
            Message::Scroll(delta) => {
                let (x, y) = match delta {
                    ScrollDelta::Lines { x, y } => (x * STEP, y * STEP),
                    ScrollDelta::Pixels { x, y } => (x, y),
                };
                // Up or left goes back; down or right goes on.
                self.scrolled += if y != 0.0 { -y } else { x };
                let steps = (self.scrolled / STEP).trunc();
                if steps != 0.0 {
                    self.scrolled -= steps * STEP;
                    self.step(steps as isize);
                }
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        subscription().map(Message::Wayland)
    }

    fn view(&self) -> Element<'_, Message> {
        let applet = &self.core.applet;
        let horizontal = applet.is_horizontal();
        let (major, minor) = applet.suggested_padding(true);
        let dots: Vec<Element<'_, Message>> = self.workspaces.iter().map(|w| dot(w.state.contains(WorkspaceFlags::Active), horizontal)).collect();
        let content: Element<'_, Message> = if horizontal {
            widget::row::with_children(dots).spacing(GAP).align_y(Alignment::Center).into()
        } else {
            widget::column::with_children(dots).spacing(GAP).align_x(Alignment::Center).into()
        };
        let padding = if horizontal { [minor, major] } else { [major, minor] };
        let button = widget::button::custom(content).padding(padding).class(hover_style()).on_press(Message::Open);
        let area = widget::mouse_area(button).on_scroll(Message::Scroll);
        widget::autosize::autosize(area, widget::Id::new("cabin-workspaces")).into()
    }
}

// ---- the wayland side

/// The workspaces, as the compositor reports them, and a way to ask for one.
pub fn subscription() -> Subscription<Event> {
    Subscription::run(|| {
        stream::channel(16, |mut output: mpsc::Sender<Event>| async move {
            let (tx, mut rx) = mpsc::channel(16);
            match start(tx) {
                Some(commands) => {
                    let _ = output.send(Event::Ready(commands)).await;
                }
                None => {
                    let _ = output.send(Event::Unavailable).await;
                    return futures::future::pending().await;
                }
            }
            while let Some(list) = rx.next().await {
                let _ = output.send(Event::Workspaces(list)).await;
            }
            let _ = output.send(Event::Unavailable).await;
            futures::future::pending().await
        })
    })
}

type Registry = (Connection, GlobalList, EventQueue<State>);

fn registry(connection: Result<Connection, ConnectError>) -> Option<Registry> {
    let connection = connection.inspect_err(|err| tracing::warn!("workspaces: no wayland connection: {err}")).ok()?;
    let (globals, queue) = registry_queue_init::<State>(&connection).inspect_err(|err| tracing::warn!("workspaces: no registry: {err}")).ok()?;
    Some((connection, globals, queue))
}

/// The compositor's registry: over the socket the panel hands a privileged
/// applet, which the workspace protocol needs, else over `WAYLAND_DISPLAY`.
fn open() -> Option<Registry> {
    std::env::var("X_PRIVILEGED_WAYLAND_SOCKET")
        .ok()
        .and_then(|fd| fd.parse::<RawFd>().ok())
        // SAFETY: the panel opened this fd for us alone, and nothing else in
        // the process reads the variable.
        .map(|fd| unsafe { UnixStream::from_raw_fd(fd) })
        .and_then(|socket| registry(Connection::from_socket(socket)))
        .or_else(|| registry(Connection::connect_to_env()))
}

fn start(tx: mpsc::Sender<Vec<Workspace>>) -> Option<calloop::channel::SyncSender<Command>> {
    let (commands, receiver) = calloop::channel::sync_channel(16);
    std::thread::Builder::new().name("workspaces".into()).spawn(move || run(receiver, tx)).ok()?;
    Some(commands)
}

fn run(commands: calloop::channel::Channel<Command>, tx: mpsc::Sender<Vec<Workspace>>) {
    let Some((connection, globals, queue)) = open() else {
        return;
    };
    let mut event_loop = match calloop::EventLoop::<State>::try_new() {
        Ok(event_loop) => event_loop,
        Err(err) => return tracing::error!("workspaces: no event loop: {err}"),
    };
    let qh = queue.handle();
    if WaylandSource::new(connection, queue).insert(event_loop.handle()).is_err() {
        return tracing::error!("workspaces: cannot watch the wayland socket");
    }
    let registry = RegistryState::new(&globals);
    let mut state = State {
        // Before the workspaces, so outputs are known when groups name them.
        output_state: OutputState::new(&globals, &qh),
        workspace_state: WorkspaceState::new(&registry, &qh),
        registry,
        panel_output: std::env::var("COSMIC_PANEL_OUTPUT").ok().filter(|name| !name.is_empty() && name != "all"),
        output: None,
        tx,
        running: true,
        ready: false,
    };
    let inserted = event_loop.handle().insert_source(commands, |event, (), state| match event {
        calloop::channel::Event::Msg(Command::Activate(handle)) => {
            handle.activate();
            match state.workspace_state.workspace_manager().get() {
                Ok(manager) => manager.commit(),
                Err(err) => tracing::warn!("workspaces: no manager to commit: {err}"),
            }
        }
        calloop::channel::Event::Closed => state.running = false,
    });
    if inserted.is_err() {
        return tracing::error!("workspaces: cannot watch for commands");
    }
    while state.running {
        if let Err(err) = event_loop.dispatch(None, &mut state) {
            return tracing::error!("workspaces: {err}");
        }
    }
}

struct State {
    registry: RegistryState,
    output_state: OutputState,
    workspace_state: WorkspaceState,
    /// The name of the output the panel is on; `None` for all of them.
    panel_output: Option<String>,
    /// That output, once seen.
    output: Option<WlOutput>,
    tx: mpsc::Sender<Vec<Workspace>>,
    running: bool,
    /// Whether the compositor has sent a first complete list.
    ready: bool,
}

impl State {
    /// The workspaces on the panel's output, or everywhere.
    fn list(&self) -> Vec<Workspace> {
        self.workspace_state
            .workspace_groups()
            .filter(|group| match &self.output {
                Some(output) => group.outputs.contains(output),
                None => self.panel_output.is_none(),
            })
            .flat_map(|group| group.workspaces.iter().filter_map(|handle| self.workspace_state.workspace_info(handle)))
            .cloned()
            .collect()
    }

    fn send(&mut self) {
        let list = self.list();
        if block_on(self.tx.send(list)).is_err() {
            self.running = false;
        }
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
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: WlOutput) {
        let name = self.output_state.info(&output).and_then(|info| info.name);
        if self.panel_output.is_some() && name == self.panel_output {
            self.output = Some(output);
            if self.ready {
                self.send();
            }
        }
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: WlOutput) {
        if self.output.as_ref() == Some(&output) {
            self.output = None;
        }
    }
}

impl WorkspaceHandler for State {
    fn workspace_state(&mut self) -> &mut WorkspaceState {
        &mut self.workspace_state
    }

    fn done(&mut self) {
        self.ready = true;
        self.send();
    }
}

cctk::delegate_workspace!(State);
sctk::delegate_output!(State);
sctk::delegate_registry!(State);
