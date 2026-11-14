// SPDX-License-Identifier: GPL-3.0-only

//! Do not disturb: cosmic-notifications' own setting, read and written as
//! COSMIC Settings does.

use cosmic::cosmic_config::{Config, ConfigGet, ConfigSet};

const ID: &str = "com.system76.CosmicNotifications";
const KEY: &str = "do_not_disturb";

pub fn get() -> bool {
    Config::new(ID, 1)
        .and_then(|c| c.get::<bool>(KEY))
        .unwrap_or(false)
}

pub fn set(on: bool) {
    if let Err(err) = Config::new(ID, 1).and_then(|c| c.set(KEY, on)) {
        tracing::warn!("cannot set do not disturb: {err}");
    }
}
