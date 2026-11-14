// SPDX-License-Identifier: GPL-3.0-only

//! The built-in display's brightness, through cosmic-settings-daemon.

use std::time::Duration;

use cosmic::iced::Subscription;

use super::latest::Latest;

/// The level and its highest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Brightness {
    pub level: i32,
    pub max: i32,
}

#[zbus::proxy(
    interface = "com.system76.CosmicSettingsDaemon",
    default_service = "com.system76.CosmicSettingsDaemon",
    default_path = "/com/system76/CosmicSettingsDaemon"
)]
trait SettingsDaemon {
    #[zbus(property)]
    fn display_brightness(&self) -> zbus::Result<i32>;
    #[zbus(property)]
    fn set_display_brightness(&self, value: i32) -> zbus::Result<()>;
    #[zbus(property)]
    fn max_display_brightness(&self) -> zbus::Result<i32>;
}

pub async fn now() -> Option<Brightness> {
    let conn = zbus::Connection::session().await.ok()?;
    let daemon = SettingsDaemonProxy::builder(&conn)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .ok()?;
    let max = daemon.max_display_brightness().await.ok()?;
    if max <= 0 {
        return None;
    }
    Some(Brightness {
        level: daemon.display_brightness().await.ok()?,
        max,
    })
}

pub fn subscription() -> Subscription<Brightness> {
    fn rules() -> Vec<zbus::MatchRule<'static>> {
        super::signal(
            None,
            Some("/com/system76/CosmicSettingsDaemon"),
            Some("org.freedesktop.DBus.Properties"),
            Some("PropertiesChanged"),
        )
        .into_iter()
        .collect()
    }
    Subscription::run(|| {
        super::watch(
            Duration::from_secs(10),
            || super::bus_changes(false, rules),
            now,
        )
    })
}

pub async fn set(level: i32) {
    static LATEST: Latest<i32> = Latest::new();
    LATEST
        .send(level, |level| async move {
            let set = async {
                let conn = zbus::Connection::session().await?;
                SettingsDaemonProxy::new(&conn)
                    .await?
                    .set_display_brightness(level)
                    .await
            };
            if let Err(err) = set.await {
                tracing::warn!("cannot set the brightness: {err}");
            }
        })
        .await;
}
