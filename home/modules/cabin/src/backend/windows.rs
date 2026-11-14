// SPDX-License-Identifier: GPL-3.0-only

//! The open windows, the one used last first, from COSMIC's toplevel
//! protocols on a connection of the daemon's own (cosmic-comp gives them to
//! any client outside a sandbox); and raising one. The order is the
//! daemon's to keep: a window the switcher raises goes to the front at
//! once. The compositor says which window is active, but only as each app
//! acknowledges it (Electron ones late): mid-switch it reports the window
//! left behind as active still, and an earlier switch's news can land after
//! a later raise. So its word moves a window to the front only once it has
//! settled, on one window, with no raise of ours still landing; that's how
//! a click, a new window or a raise that went nowhere gets in.

use std::time::{Duration, Instant};

use cosmic::{
    cctk::{
        self,
        cosmic_protocols::{
            toplevel_info::v1::client::zcosmic_toplevel_handle_v1::{
                State as Flag, ZcosmicToplevelHandleV1,
            },
            toplevel_management::v1::client::zcosmic_toplevel_manager_v1::ZcosmicToplelevelManagementCapabilitiesV1,
        },
        sctk::{
            self,
            reexports::{calloop, calloop_wayland_source::WaylandSource},
            registry::{ProvidesRegistryState, RegistryState},
            seat::{Capability, SeatHandler, SeatState},
        },
        toplevel_info::{ToplevelInfoHandler, ToplevelInfoState},
        toplevel_management::{ToplevelManagerHandler, ToplevelManagerState},
        wayland_client::{
            Connection, QueueHandle, WEnum, globals::registry_queue_init, protocol::wl_seat::WlSeat,
        },
        wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
        wayland_protocols::ext::workspace::v1::client::ext_workspace_handle_v1::State as WorkspaceFlags,
        workspace::{WorkspaceHandler, WorkspaceState},
    },
    iced::{Subscription, stream},
};
use futures::{SinkExt, StreamExt, channel::mpsc, executor::block_on};

/// How long the active windows must stay as they are to count.
const SETTLE: Duration = Duration::from_millis(250);
/// How long after a raise of ours the compositor's word waits.
const RAISE_HOLD: Duration = Duration::from_millis(500);

/// A window as the switcher shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    /// The compositor's id for it, as capture knows it too.
    pub id: String,
    pub app_id: String,
    pub title: String,
    pub handle: ZcosmicToplevelHandleV1,
    /// On a workspace that shows now (or on all of them).
    pub here: bool,
}

/// What the wayland thread reports.
#[derive(Clone, Debug)]
pub enum Event {
    Ready(calloop::channel::SyncSender<Command>),
    /// Every window, the one used last first.
    Windows(Vec<Window>),
    /// The thread that talks to the compositor could not start, or has ended.
    Unavailable,
}

/// What the wayland thread is asked to do.
#[derive(Clone, Debug)]
pub enum Command {
    Activate(ZcosmicToplevelHandleV1),
}

/// The windows as they change, and a way to raise one.
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
                let _ = output.send(Event::Windows(list)).await;
            }
            let _ = output.send(Event::Unavailable).await;
            futures::future::pending().await
        })
    })
}

fn start(tx: mpsc::Sender<Vec<Window>>) -> Option<calloop::channel::SyncSender<Command>> {
    let (commands, receiver) = calloop::channel::sync_channel(16);
    std::thread::Builder::new()
        .name("windows".into())
        .spawn(move || run(receiver, tx))
        .ok()?;
    Some(commands)
}

fn run(commands: calloop::channel::Channel<Command>, tx: mpsc::Sender<Vec<Window>>) {
    let connection = match Connection::connect_to_env() {
        Ok(connection) => connection,
        Err(err) => return tracing::warn!("windows: no wayland connection: {err}"),
    };
    let (globals, queue) = match registry_queue_init::<State>(&connection) {
        Ok(init) => init,
        Err(err) => return tracing::warn!("windows: no registry: {err}"),
    };
    let mut event_loop = match calloop::EventLoop::<State>::try_new() {
        Ok(event_loop) => event_loop,
        Err(err) => return tracing::error!("windows: no event loop: {err}"),
    };
    let qh = queue.handle();
    if WaylandSource::new(connection, queue)
        .insert(event_loop.handle())
        .is_err()
    {
        return tracing::error!("windows: cannot watch the wayland socket");
    }
    let registry = RegistryState::new(&globals);
    let (Some(info), Some(manager)) = (
        ToplevelInfoState::try_new(&registry, &qh),
        ToplevelManagerState::try_new(&registry, &qh),
    ) else {
        return tracing::warn!(
            "windows: the compositor offers no toplevel info or management here"
        );
    };
    let workspaces = WorkspaceState::new(&registry, &qh);
    let mut state = State {
        seats: SeatState::new(&globals, &qh),
        registry,
        info,
        manager,
        workspaces,
        order: Vec::new(),
        active: Vec::new(),
        settle_at: None,
        raised_at: None,
        sent: Vec::new(),
        tx,
        running: true,
    };
    let inserted = event_loop
        .handle()
        .insert_source(commands, |event, (), state| match event {
            calloop::channel::Event::Msg(Command::Activate(handle)) => {
                match state.seats.seats().next() {
                    Some(seat) => state.manager.manager.activate(&handle, &seat),
                    None => return tracing::warn!("windows: no seat to activate on"),
                }
                let raised = state
                    .info
                    .toplevels()
                    .find(|t| t.cosmic_toplevel.as_ref() == Some(&handle))
                    .map(|t| t.foreign_toplevel.clone());
                if let Some(raised) = raised {
                    bring_to_front(&mut state.order, &raised);
                    state.raised_at = Some(Instant::now());
                    state.send();
                }
            }
            calloop::channel::Event::Closed => state.running = false,
        });
    if inserted.is_err() {
        return tracing::error!("windows: cannot watch for commands");
    }
    while state.running {
        let timeout = state
            .settle_at
            .map(|at| at.saturating_duration_since(Instant::now()));
        if let Err(err) = event_loop.dispatch(timeout, &mut state) {
            return tracing::error!("windows: {err}");
        }
        if state.settle_at.is_some_and(|at| Instant::now() >= at) {
            state.settle();
        }
    }
}

struct State {
    registry: RegistryState,
    seats: SeatState,
    info: ToplevelInfoState,
    manager: ToplevelManagerState,
    workspaces: WorkspaceState,
    /// Every window, the one used last first.
    order: Vec<ExtForeignToplevelHandleV1>,
    /// Those last reported active, and when that will have settled.
    active: Vec<ExtForeignToplevelHandleV1>,
    settle_at: Option<Instant>,
    /// When the switcher last raised a window.
    raised_at: Option<Instant>,
    /// The list as last sent.
    sent: Vec<Window>,
    tx: mpsc::Sender<Vec<Window>>,
    running: bool,
}

impl State {
    fn changed(&mut self, handle: &ExtForeignToplevelHandleV1) {
        if !self.order.contains(handle) {
            self.order.push(handle.clone());
        }
        let active = self
            .info
            .info(handle)
            .is_some_and(|t| t.state.contains(&Flag::Activated));
        if active != self.active.contains(handle) {
            if active {
                self.active.push(handle.clone());
            } else {
                self.active.retain(|h| h != handle);
            }
            self.settle_at = Some(Instant::now() + SETTLE);
        }
        self.send();
    }

    /// The active windows have stayed put: the one window active goes to
    /// the front, unless a raise of ours is still landing.
    fn settle(&mut self) {
        self.settle_at = None;
        if let Some(at) = self
            .raised_at
            .map(|t| t + RAISE_HOLD)
            .filter(|at| *at > Instant::now())
        {
            self.settle_at = Some(at);
            return;
        }
        if settled(&mut self.order, &self.active) {
            self.send();
        }
    }

    /// The list sent if it changed. Only windows cctk has the cosmic handle
    /// of can be raised, so only they are listed.
    fn send(&mut self) {
        let showing: Vec<_> = self
            .workspaces
            .workspace_groups()
            .flat_map(|group| group.workspaces.iter())
            .filter(|handle| {
                self.workspaces
                    .workspace_info(handle)
                    .is_some_and(|w| w.state.contains(WorkspaceFlags::Active))
            })
            .collect();
        let list: Vec<Window> = self
            .order
            .iter()
            .filter_map(|h| self.info.info(h))
            .filter_map(|t| {
                Some(Window {
                    id: t.identifier.clone(),
                    app_id: t.app_id.clone(),
                    title: t.title.clone(),
                    handle: t.cosmic_toplevel.clone()?,
                    here: t.workspace.is_empty()
                        || t.workspace.iter().any(|w| showing.contains(&w)),
                })
            })
            .collect();
        if list != self.sent {
            self.sent = list.clone();
            if block_on(self.tx.send(list)).is_err() {
                self.running = false;
            }
        }
    }
}

/// The window active, if just one is, to the front; whether that moved it.
fn settled<T: PartialEq + Clone>(order: &mut Vec<T>, active: &[T]) -> bool {
    match active {
        [only] if order.first() != Some(only) => {
            bring_to_front(order, only);
            true
        }
        _ => false,
    }
}

/// `item` moved to the front of `order`, the rest kept in order.
fn bring_to_front<T: PartialEq + Clone>(order: &mut Vec<T>, item: &T) {
    order.retain(|i| i != item);
    order.insert(0, item.clone());
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    sctk::registry_handlers![SeatState];
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}
    fn new_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat, _: Capability) {}
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: WlSeat,
        _: Capability,
    ) {
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}
}

impl ToplevelInfoHandler for State {
    fn toplevel_info_state(&mut self) -> &mut ToplevelInfoState {
        &mut self.info
    }
    fn new_toplevel(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        handle: &ExtForeignToplevelHandleV1,
    ) {
        self.changed(handle);
    }
    fn update_toplevel(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        handle: &ExtForeignToplevelHandleV1,
    ) {
        self.changed(handle);
    }
    fn toplevel_closed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        handle: &ExtForeignToplevelHandleV1,
    ) {
        self.order.retain(|h| h != handle);
        self.active.retain(|h| h != handle);
        self.settle_at = Some(Instant::now() + SETTLE);
        self.send();
    }
}

impl WorkspaceHandler for State {
    fn workspace_state(&mut self) -> &mut WorkspaceState {
        &mut self.workspaces
    }
    fn done(&mut self) {
        self.send();
    }
}

impl ToplevelManagerHandler for State {
    fn toplevel_manager_state(&mut self) -> &mut ToplevelManagerState {
        &mut self.manager
    }
    fn capabilities(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: Vec<WEnum<ZcosmicToplelevelManagementCapabilitiesV1>>,
    ) {
    }
}

cctk::delegate_toplevel_info!(State);
cctk::delegate_toplevel_manager!(State);
cctk::delegate_workspace!(State);
sctk::delegate_seat!(State);
sctk::delegate_registry!(State);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_one_active_window_comes_first() {
        let mut order = vec!['a', 'b', 'c'];
        // Mid-switch two are reported active (logged 2026-11-13): nothing moves.
        assert!(!settled(&mut order, &['b', 'a']));
        assert!(!settled(&mut order, &[]));
        assert_eq!(order, ['a', 'b', 'c']);
        assert!(settled(&mut order, &['c']));
        assert_eq!(order, ['c', 'a', 'b']);
        assert!(!settled(&mut order, &['c']));
    }

    #[test]
    fn the_window_used_last_comes_first() {
        let mut order = vec!['a', 'b', 'c'];
        bring_to_front(&mut order, &'c');
        assert_eq!(order, ['c', 'a', 'b']);
        bring_to_front(&mut order, &'c');
        assert_eq!(order, ['c', 'a', 'b']);
        bring_to_front(&mut order, &'d');
        assert_eq!(order, ['d', 'c', 'a', 'b']);
    }

    /// Needs a running COSMIC session: prints the windows as cabind would
    /// see them, from a plain terminal.
    #[test]
    #[ignore]
    fn lists_the_windows() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        let (tx, mut rx) = mpsc::channel(64);
        let _commands = start(tx).expect("the wayland thread starts");
        std::thread::sleep(std::time::Duration::from_millis(500));
        let mut last = None;
        while let Ok(list) = rx.try_recv() {
            last = Some(list);
        }
        let list = last.expect("a window list");
        for window in &list {
            println!(
                "{:40} {} {}",
                window.app_id,
                if window.here { "here " } else { "away " },
                window.title
            );
        }
        assert!(!list.is_empty());
    }
}
