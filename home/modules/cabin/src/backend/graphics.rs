// SPDX-License-Identifier: GPL-3.0-only

//! Which GPU is in use, through supergfxd.

use std::time::Duration;

use cosmic::iced::Subscription;

/// supergfxd's state, in its own numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Graphics {
    /// GfxMode: 0 Hybrid, 1 Integrated, 2 NvidiaNoModeset, 3 Vfio, 4 AsusEgpu, 5 AsusMuxDgpu.
    pub mode: u32,
    /// GfxPower: 0 active, 1 suspended, 2 off, 3 disabled, 4 mux, 5 unknown.
    pub power: u32,
    pub pending_mode: u32,
    /// UserActionRequired: 0 log out, 1 reboot, 2 switch to integrated first, 3 disable eGPU, 4 nothing.
    pub pending_action: u32,
    pub supported: Vec<u32>,
}

impl Graphics {
    pub fn mode_name(mode: u32) -> &'static str {
        match mode {
            0 => "Hybrid",
            1 => "Integrated",
            2 => "NVIDIA",
            3 => "VFIO",
            4 => "eGPU",
            5 => "Dedicated",
            _ => "Unknown",
        }
    }

    pub fn power_name(&self) -> &'static str {
        match self.power {
            0 => "dGPU awake",
            1 => "dGPU asleep",
            2 => "dGPU off",
            3 => "dGPU disabled",
            4 => "MUX",
            _ => "",
        }
    }

    /// A switch asked for, waiting on a logout or reboot.
    pub fn pending(&self) -> bool {
        self.pending_mode != self.mode && self.pending_action != 4
    }

    pub fn icon(&self) -> &'static str {
        match self.power {
            // The dedicated GPU awake, or the integrated one doing the work.
            0 => "gpu",
            _ => "gpu-2",
        }
    }
}

#[zbus::proxy(
    interface = "org.supergfxctl.Daemon",
    default_service = "org.supergfxctl.Daemon",
    default_path = "/org/supergfxctl/Gfx"
)]
trait SuperGfx {
    fn mode(&self) -> zbus::Result<u32>;
    fn power(&self) -> zbus::Result<u32>;
    fn pending_mode(&self) -> zbus::Result<u32>;
    fn pending_user_action(&self) -> zbus::Result<u32>;
    fn supported(&self) -> zbus::Result<Vec<u32>>;
}

/// `None` without the daemon, or while it holds its lock waiting for a
/// logout and answers nothing.
pub async fn now() -> Option<Option<Graphics>> {
    let conn = zbus::Connection::system().await.ok()?;
    let dbus = zbus::fdo::DBusProxy::new(&conn).await.ok()?;
    if !dbus.name_has_owner("org.supergfxctl.Daemon".try_into().ok()?).await.ok()? {
        return Some(None);
    }
    let gfx = SuperGfxProxy::new(&conn).await.ok()?;
    let read = async {
        zbus::Result::Ok(Graphics {
            mode: gfx.mode().await?,
            power: gfx.power().await?,
            pending_mode: gfx.pending_mode().await?,
            pending_action: gfx.pending_user_action().await?,
            supported: gfx.supported().await?,
        })
    };
    match tokio::time::timeout(Duration::from_secs(2), read).await {
        Ok(Ok(graphics)) => Some(Some(graphics)),
        // Busy: keep what was known.
        _ => None,
    }
}

pub fn subscription() -> Subscription<Option<Graphics>> {
    fn rules() -> Vec<zbus::MatchRule<'static>> {
        super::signal(None, Some("/org/supergfxctl/Gfx"), None, None).into_iter().collect()
    }
    Subscription::run(|| super::watch(Duration::from_secs(15), || super::bus_changes(true, rules), now))
}
