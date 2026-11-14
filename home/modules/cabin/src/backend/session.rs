// SPDX-License-Identifier: GPL-3.0-only

//! Locking, logging out, suspending, restarting, shutting down, and
//! COSMIC Settings: all programs on the host.

use super::spawn;

pub fn lock() {
    spawn("loginctl", &["lock-session"], &[]);
}

pub fn log_out() {
    spawn("cosmic-osd", &["log-out"], &[]);
}

pub fn suspend() {
    spawn("systemctl", &["suspend"], &[]);
}

pub fn restart() {
    spawn("cosmic-osd", &["restart"], &[]);
}

/// Our own capture overlay, next to this program (the wrapper in the Nix
/// build, the binary in the dev loop), with our environment: it's ours.
pub fn screenshot() {
    let capture = std::env::current_exe().map(|exe| exe.with_file_name("cabin-capture"));
    match capture.and_then(|path| std::process::Command::new(path).spawn()) {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(err) => tracing::error!("cannot run cabin-capture: {err}"),
    }
}

pub fn power_off() {
    spawn("cosmic-osd", &["shutdown"], &[]);
}

/// The dialog that logs out to switch graphics: its own process beside
/// this binary, since an applet can't map a dialog of its own.
pub fn graphics_logout(mode: u32, name: &str) {
    let dialog = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("cabin-graphics-logout")))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "cabin-graphics-logout".to_owned());
    spawn(&dialog, &[&mode.to_string(), name], &[]);
}

/// COSMIC Settings on a page, brought to the front with the activation token
/// the compositor gave if it's already open.
pub fn settings(page: Option<&str>, token: Option<&str>) {
    let env: Vec<(&str, &str)> = token
        .map(|t| ("XDG_ACTIVATION_TOKEN", t))
        .into_iter()
        .collect();
    let args: Vec<&str> = page.into_iter().collect();
    spawn("cosmic-settings", &args, &env);
}

#[zbus::proxy(
    interface = "org.freedesktop.DbusActivation",
    default_service = "com.system76.CosmicWorkspaces",
    default_path = "/com/system76/CosmicWorkspaces"
)]
trait Workspaces {
    fn activate(
        &self,
        platform_data: std::collections::HashMap<&str, zbus::zvariant::Value<'_>>,
    ) -> zbus::Result<()>;
}

/// The workspace overview, toggled: cosmic-workspaces keeps running once
/// started and takes a toggle over the bus, which is instant and can't race
/// a second start; only when it isn't there is it started.
pub async fn overview() {
    let toggled = async {
        let conn = zbus::Connection::session().await?;
        WorkspacesProxy::new(&conn)
            .await?
            .activate(std::collections::HashMap::new())
            .await
    };
    if let Err(err) = toggled.await {
        tracing::debug!("the overview is not running ({err}); starting it");
        spawn("cosmic-workspaces", &[], &[]);
    }
}
