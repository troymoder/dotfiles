// SPDX-License-Identifier: GPL-3.0-only

//! Power saving: when the screen goes dark and when the machine suspends
//! after it's left alone. cosmic-idle's own settings, read and written as
//! COSMIC Settings does, with the same choices.

use cosmic::cosmic_config::{Config, ConfigGet, ConfigSet};

const ID: &str = "com.system76.CosmicIdle";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timeout {
    ScreenOff,
    SuspendOnAc,
    SuspendOnBattery,
}

impl Timeout {
    pub const ALL: [Self; 3] = [Self::ScreenOff, Self::SuspendOnAc, Self::SuspendOnBattery];

    fn key(self) -> &'static str {
        match self {
            Self::ScreenOff => "screen_off_time",
            Self::SuspendOnAc => "suspend_on_ac_time",
            Self::SuspendOnBattery => "suspend_on_battery_time",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::ScreenOff => "Turn off the screen",
            Self::SuspendOnAc => "Suspend when plugged in",
            Self::SuspendOnBattery => "Suspend on battery",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::ScreenOff => "device-laptop-off",
            Self::SuspendOnAc => "plug-connected",
            Self::SuspendOnBattery => "battery-2",
        }
    }

    /// What COSMIC Settings offers, in minutes; Never after them.
    pub fn choices(self) -> &'static [u32] {
        match self {
            Self::ScreenOff => &[2, 5, 10, 15, 30],
            Self::SuspendOnAc | Self::SuspendOnBattery => &[15, 20, 25, 30, 45, 60, 80, 90, 100, 120],
        }
    }

    /// cosmic-idle's own default, while the key was never written.
    fn default_ms(self) -> Option<u32> {
        match self {
            Self::ScreenOff | Self::SuspendOnBattery => Some(15 * 60 * 1000),
            Self::SuspendOnAc => Some(30 * 60 * 1000),
        }
    }
}

/// Each timeout in milliseconds, `None` for never; in `Timeout::ALL` order.
pub type Idle = [Option<u32>; 3];

pub fn read() -> Idle {
    let config = Config::new(ID, 1).ok();
    Timeout::ALL.map(|t| config.as_ref().and_then(|c| c.get::<Option<u32>>(t.key()).ok()).unwrap_or_else(|| t.default_ms()))
}

pub fn set(timeout: Timeout, ms: Option<u32>) {
    if let Err(err) = Config::new(ID, 1).and_then(|c| c.set(timeout.key(), ms)) {
        tracing::warn!("cannot set {}: {err}", timeout.key());
    }
}

/// As COSMIC Settings words it: whole hours as hours.
pub fn label(ms: Option<u32>) -> String {
    let Some(ms) = ms else {
        return "Never".to_owned();
    };
    let minutes = ms / 60_000;
    match minutes {
        60 => "1 hour".to_owned(),
        m if m % 60 == 0 => format!("{} hours", m / 60),
        1 => "1 minute".to_owned(),
        m => format!("{m} minutes"),
    }
}
