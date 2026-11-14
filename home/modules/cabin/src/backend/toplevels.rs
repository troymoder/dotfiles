// SPDX-License-Identifier: GPL-3.0-only

//! Bringing a running app's window forward, through COSMIC's toplevel
//! protocols: a one-shot connection, used and dropped.

use std::os::{
    fd::{FromRawFd, RawFd},
    unix::net::UnixStream,
};

use cosmic::cctk::{
    self,
    cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1::ZcosmicToplelevelManagementCapabilitiesV1,
    sctk::{
        self,
        activation::{ActivationHandler, ActivationState, RequestData},
        registry::{ProvidesRegistryState, RegistryState},
        seat::{Capability, SeatHandler, SeatState},
    },
    toplevel_info::{ToplevelInfoHandler, ToplevelInfoState},
    toplevel_management::{ToplevelManagerHandler, ToplevelManagerState},
    wayland_client::{
        Connection, QueueHandle, WEnum, globals::registry_queue_init, protocol::wl_seat::WlSeat,
    },
    wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
};

/// How many roundtrips the toplevel list takes to fill in: one brings the
/// list and binds the cosmic handles, the next their state, which cctk waits
/// for before it shows a toplevel; one to spare. (The info protocol's `done`
/// is not something to wait for: cosmic-comp does not send it.)
const ROUNDTRIPS: usize = 6;

/// Raises the running window of the app with this id (e.g.
/// "com.system76.CosmicSettings"), through COSMIC's toplevel management on
/// the privileged host socket. Whether one was found.
pub async fn activate(app_id: String) -> bool {
    match tokio::task::spawn_blocking(move || activate_blocking(&app_id)).await {
        Ok(found) => found,
        Err(err) => {
            tracing::debug!("toplevels: activation did not finish: {err}");
            false
        }
    }
}

/// The compositor: over the socket the panel hands a privileged applet,
/// which the toplevel protocols need, else over `WAYLAND_DISPLAY`.
fn connect() -> Option<Connection> {
    // One connection for the life of the process: the socket is a stream the
    // compositor knows by its object ids, so it can't be opened twice.
    static PRIVILEGED: std::sync::OnceLock<Option<Connection>> = std::sync::OnceLock::new();
    let privileged = PRIVILEGED.get_or_init(|| {
        let fd = std::env::var("X_PRIVILEGED_WAYLAND_SOCKET")
            .ok()?
            .parse::<RawFd>()
            .ok()?;
        // SAFETY: the panel opened this fd for us alone; nothing else in the
        // process reads the variable, and this runs once.
        let socket = unsafe { UnixStream::from_raw_fd(fd) };
        Connection::from_socket(socket)
            .inspect_err(|err| tracing::debug!("toplevels: privileged socket: {err}"))
            .ok()
    });
    privileged.clone().or_else(|| {
        Connection::connect_to_env()
            .inspect_err(|err| tracing::debug!("toplevels: no wayland connection: {err}"))
            .ok()
    })
}

/// An app id as compared: case and a `.desktop` suffix do not count.
fn key(app_id: &str) -> String {
    app_id.trim_end_matches(".desktop").to_ascii_lowercase()
}

fn activate_blocking(app_id: &str) -> bool {
    let Some(conn) = connect() else {
        return false;
    };
    let (globals, mut queue) = match registry_queue_init::<State>(&conn) {
        Ok(init) => init,
        Err(err) => {
            tracing::debug!("toplevels: no registry: {err}");
            return false;
        }
    };
    let qh = queue.handle();
    let registry = RegistryState::new(&globals);
    let (Some(info), Some(manager)) = (
        ToplevelInfoState::try_new(&registry, &qh),
        ToplevelManagerState::try_new(&registry, &qh),
    ) else {
        tracing::debug!("toplevels: the compositor offers no toplevel info or management here");
        return false;
    };
    let mut state = State {
        seats: SeatState::new(&globals, &qh),
        registry,
        info,
        manager,
    };
    for _ in 0..ROUNDTRIPS {
        if let Err(err) = queue.roundtrip(&mut state) {
            tracing::debug!("toplevels: {err}");
            return false;
        }
    }
    let Some(seat) = state.seats.seats().next() else {
        tracing::debug!("toplevels: no seat");
        return false;
    };
    let wanted = key(app_id);
    let Some(handle) = state
        .info
        .toplevels()
        .find(|t| key(&t.app_id) == wanted)
        .and_then(|t| t.cosmic_toplevel.clone())
    else {
        let seen: Vec<&str> = state.info.toplevels().map(|t| t.app_id.as_str()).collect();
        tracing::debug!("toplevels: no window of {app_id}; open: {seen:?}");
        return false;
    };
    state.manager.manager.activate(&handle, &seat);
    // Delivered before the connection goes.
    match queue.roundtrip(&mut state) {
        Ok(_) => true,
        Err(err) => {
            tracing::debug!("toplevels: activating {app_id}: {err}");
            false
        }
    }
}

struct State {
    registry: RegistryState,
    seats: SeatState,
    info: ToplevelInfoState,
    manager: ToplevelManagerState,
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
        tracing::debug!(
            "toplevels: new {:?}",
            self.info
                .info(handle)
                .map(|t| (t.app_id.clone(), t.cosmic_toplevel.is_some()))
        );
    }
    fn update_toplevel(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        handle: &ExtForeignToplevelHandleV1,
    ) {
        tracing::debug!(
            "toplevels: update {:?}",
            self.info.info(handle).map(|t| (
                t.app_id.clone(),
                t.cosmic_toplevel.is_some(),
                t.state.len()
            ))
        );
    }
    fn toplevel_closed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ExtForeignToplevelHandleV1,
    ) {
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
sctk::delegate_seat!(State);
sctk::delegate_registry!(State);

#[cfg(test)]
mod tests {
    /// Needs a running compositor and `cosmic-settings`. From a terminal
    /// there is no privileged socket, so the plain one is tried: cosmic-comp
    /// then lists the toplevels but keeps their cosmic state to itself, and
    /// cctk shows none, so this prints `false`; it is `true` from the panel.
    #[tokio::test]
    #[ignore]
    async fn raises_settings() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();
        let found = super::activate("com.system76.CosmicSettings".into()).await;
        println!("activate(com.system76.CosmicSettings) -> {found}");
    }
}

/// An activation token for starting `app_id`, asked of the compositor over
/// the privileged socket: a window it then opens may take the focus. Tokens
/// from the applet's own surfaces don't count, as those live on the panel.
pub async fn activation_token(app_id: String) -> Option<String> {
    tokio::task::spawn_blocking(move || token_blocking(app_id))
        .await
        .ok()
        .flatten()
}

fn token_blocking(app_id: String) -> Option<String> {
    let conn = connect()?;
    let (globals, mut queue) = registry_queue_init::<TokenState>(&conn)
        .inspect_err(|err| tracing::debug!("toplevels: no registry: {err}"))
        .ok()?;
    let qh = queue.handle();
    let mut state = TokenState {
        registry: RegistryState::new(&globals),
        seats: SeatState::new(&globals, &qh),
        activation: ActivationState::bind::<TokenState>(&globals, &qh)
            .inspect_err(|err| tracing::debug!("toplevels: no activation: {err}"))
            .ok()?,
        token: None,
    };
    queue.roundtrip(&mut state).ok()?;
    let seat = state.seats.seats().next();
    state.activation.request_token_with_data(
        &qh,
        RequestData {
            app_id: Some(app_id),
            seat_and_serial: seat.map(|seat| (seat, 0)),
            surface: None,
        },
    );
    for _ in 0..ROUNDTRIPS {
        queue.roundtrip(&mut state).ok()?;
        if state.token.is_some() {
            break;
        }
    }
    state.token
}

struct TokenState {
    registry: RegistryState,
    seats: SeatState,
    activation: ActivationState,
    token: Option<String>,
}

impl ProvidesRegistryState for TokenState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    sctk::registry_handlers![SeatState];
}

impl SeatHandler for TokenState {
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

impl ActivationHandler for TokenState {
    type RequestData = RequestData;
    fn new_token(&mut self, token: String, _: &RequestData) {
        self.token = Some(token);
    }
}

sctk::delegate_seat!(TokenState);
sctk::delegate_registry!(TokenState);
sctk::delegate_activation!(TokenState);
