// SPDX-License-Identifier: GPL-3.0-only

//! The battery and the keyboard backlight through UPower, and the power
//! profile through power-profiles-daemon.

use std::{collections::HashMap, time::Duration};

use cosmic::iced::Subscription;
use zbus::zvariant::OwnedValue;

use super::latest::Latest;

#[derive(Clone, Debug, PartialEq)]
pub struct Battery {
    pub percentage: f64,
    pub charging: bool,
    /// Seconds to empty, or to full while charging.
    pub time_left: Option<u64>,
    /// Watts in or out.
    pub rate: f64,
    /// How much of the design capacity is left, in percent.
    pub health: Option<f64>,
    pub cycles: Option<i32>,
    /// Full, and plugged in.
    pub full: bool,
    /// What the USB-C charger can give, in watts, when one is plugged in.
    pub charger_w: Option<f32>,
    /// Charging stopped short of full to spare the battery, where the
    /// firmware can (UPower, over the EC's charge control).
    pub limit: Option<ChargeLimit>,
    /// The charge over the last `HISTORY_HOURS`, as UPower kept it: seconds
    /// since the epoch and percent, oldest first.
    pub history: Vec<(u32, f64)>,
}

/// The charge limit: whether it's on, and where charging stops then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChargeLimit {
    pub enabled: bool,
    pub end: u32,
}

/// How far back the battery graph looks.
pub const HISTORY_HOURS: u32 = 12;

impl Battery {
    /// Tabler's batteries: charging, or by quarters.
    pub fn icon(&self) -> String {
        if self.charging {
            return "battery-charging".to_owned();
        }
        match self.percentage {
            p if p > 80.0 => "battery-4",
            p if p > 55.0 => "battery-3",
            p if p > 30.0 => "battery-2",
            p if p > 10.0 => "battery-1",
            _ => "battery",
        }
        .to_owned()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Profiles {
    pub active: String,
    pub available: Vec<String>,
}

impl Profiles {
    pub fn name(profile: &str) -> &'static str {
        match profile {
            "performance" => "Performance",
            "power-saver" => "Power saver",
            _ => "Balanced",
        }
    }

    pub fn icon(profile: &str) -> &'static str {
        match profile {
            "performance" => "power-profile-performance-symbolic",
            "power-saver" => "power-profile-power-saver-symbolic",
            _ => "power-profile-balanced-symbolic",
        }
    }
}

/// The keyboard backlight: its level, its highest, and whose keyboard.
#[derive(Clone, Debug, PartialEq)]
pub struct Keyboard {
    pub level: i32,
    pub max: i32,
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Power {
    pub battery: Option<Battery>,
    pub profiles: Option<Profiles>,
    pub keyboard: Option<Keyboard>,
}

#[zbus::proxy(
    interface = "org.freedesktop.UPower.Device",
    default_service = "org.freedesktop.UPower",
    default_path = "/org/freedesktop/UPower/devices/DisplayDevice"
)]
trait DisplayDevice {
    #[zbus(property)]
    fn is_present(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn percentage(&self) -> zbus::Result<f64>;
    /// 1 charging, 2 discharging, 4 fully charged.
    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn time_to_empty(&self) -> zbus::Result<i64>;
    #[zbus(property)]
    fn time_to_full(&self) -> zbus::Result<i64>;
    #[zbus(property)]
    fn energy_rate(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn capacity(&self) -> zbus::Result<f64>;
}

#[zbus::proxy(interface = "org.freedesktop.UPower", default_service = "org.freedesktop.UPower", default_path = "/org/freedesktop/UPower")]
trait UPower {
    fn enumerate_devices(&self) -> zbus::Result<Vec<zbus::zvariant::OwnedObjectPath>>;
}

#[zbus::proxy(interface = "org.freedesktop.UPower.Device", default_service = "org.freedesktop.UPower")]
trait AnyDevice {
    #[zbus(property, name = "Type")]
    fn kind(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn charge_cycles(&self) -> zbus::Result<i32>;
    /// `kind` is "charge" or "rate"; so many seconds back, in so many points.
    fn get_history(&self, kind: &str, timespan: u32, resolution: u32) -> zbus::Result<Vec<(u32, f64, u32)>>;
    #[zbus(property)]
    fn charge_threshold_supported(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn charge_threshold_enabled(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn charge_end_threshold(&self) -> zbus::Result<u32>;
    /// Polkit lets the active user do this unasked.
    fn enable_charge_threshold(&self, enabled: bool) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "net.hadess.PowerProfiles",
    default_service = "net.hadess.PowerProfiles",
    default_path = "/net/hadess/PowerProfiles"
)]
trait PowerProfiles {
    #[zbus(property)]
    fn active_profile(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn set_active_profile(&self, profile: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn profiles(&self) -> zbus::Result<Vec<HashMap<String, OwnedValue>>>;
}

#[zbus::proxy(
    interface = "org.freedesktop.UPower.KbdBacklight",
    default_service = "org.freedesktop.UPower",
    default_path = "/org/freedesktop/UPower/KbdBacklight"
)]
trait KbdBacklight {
    fn get_brightness(&self) -> zbus::Result<i32>;
    fn get_max_brightness(&self) -> zbus::Result<i32>;
    fn set_brightness(&self, value: i32) -> zbus::Result<()>;
}

async fn battery(conn: &zbus::Connection) -> Option<Battery> {
    let device = DisplayDeviceProxy::builder(conn)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .ok()?;
    if !device.is_present().await.ok()? {
        return None;
    }
    let state = device.state().await.ok()?;
    let charging = state == 1 || state == 4;
    let seconds = if charging { device.time_to_full().await } else { device.time_to_empty().await };
    // Charge cycles and the history are the real battery's, not the display device's.
    let mut cycles = None;
    let mut history = Vec::new();
    let mut limit = None;
    if let Ok(upower) = UPowerProxy::new(conn).await {
        for path in upower.enumerate_devices().await.unwrap_or_default() {
            let Ok(builder) = AnyDeviceProxy::builder(conn).path(path) else {
                continue;
            };
            let Ok(any) = builder.build().await else {
                continue;
            };
            if any.kind().await.ok() == Some(2) {
                cycles = any.charge_cycles().await.ok().filter(|c| *c > 0);
                history = any
                    .get_history("charge", HISTORY_HOURS * 3600, 60)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|(_, value, _)| *value > 0.0)
                    .map(|(time, value, _)| (time, value))
                    .collect();
                history.sort_by_key(|(time, _)| *time);
                if any.charge_threshold_supported().await.unwrap_or(false) {
                    let end = any.charge_end_threshold().await.ok().filter(|end| (1..100).contains(end)).unwrap_or(80);
                    limit = Some(ChargeLimit { enabled: any.charge_threshold_enabled().await.unwrap_or(false), end });
                }
                break;
            }
        }
    }
    Some(Battery {
        percentage: device.percentage().await.ok()?,
        charging,
        time_left: seconds.ok().filter(|s| *s > 0).map(|s| s as u64),
        rate: device.energy_rate().await.unwrap_or(0.0),
        health: device.capacity().await.ok().filter(|c| *c > 0.0),
        cycles,
        full: state == 4,
        charger_w: charger_watts(),
        limit,
        history,
    })
}

/// The charger's rating: the strongest profile it advertises over Power
/// Delivery (the kernel lists the standard ones up to 20 V), else the most the
/// online USB-C source could give. Not what's drawn now.
fn charger_watts() -> Option<f32> {
    let read = |path: std::path::PathBuf| std::fs::read_to_string(path).ok().and_then(|s| s.trim().parse::<f64>().ok());
    // Only when a source is actually plugged in.
    let online = std::fs::read_dir("/sys/class/power_supply")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("ucsi-source-psy")))
        .find(|p| read(p.join("online")) == Some(1.0))?;
    let mut best: Option<f64> = None;
    if let Ok(partners) = std::fs::read_dir("/sys/class/typec") {
        for partner in partners.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().ends_with("-partner"))) {
            let Ok(profiles) = std::fs::read_dir(partner.join("usb_power_delivery/source-capabilities")) else {
                continue;
            };
            for profile in profiles.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().contains("fixed_supply")) {
                let (Some(millivolts), Some(milliamps)) = (read(profile.join("voltage")), read(profile.join("maximum_current"))) else {
                    continue;
                };
                let watts = millivolts / 1000.0 * milliamps / 1000.0;
                best = Some(best.map_or(watts, |b: f64| b.max(watts)));
            }
        }
    }
    let watts = match best {
        Some(watts) => watts,
        None => {
            let volts = read(online.join("voltage_max")).or_else(|| read(online.join("voltage_now")))? / 1e6;
            let amps = read(online.join("current_max"))? / 1e6;
            volts * amps
        }
    };
    (watts > 1.0).then_some(watts as f32)
}

async fn profiles(conn: &zbus::Connection) -> Option<Profiles> {
    let daemon = PowerProfilesProxy::builder(conn)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .ok()?;
    let available = daemon
        .profiles()
        .await
        .ok()?
        .iter()
        .filter_map(|p| p.get("Profile").and_then(|v| String::try_from(v.clone()).ok()))
        .collect();
    Some(Profiles { active: daemon.active_profile().await.ok()?, available })
}

async fn keyboard(conn: &zbus::Connection) -> Option<Keyboard> {
    let backlight = KbdBacklightProxy::new(conn).await.ok()?;
    let max = backlight.get_max_brightness().await.ok()?;
    if max <= 0 {
        return None;
    }
    Some(Keyboard { level: backlight.get_brightness().await.ok()?, max, name: keyboard_name() })
}

/// Whose keyboard the backlight is, from the LED's name
/// (`framework::kbd_backlight`): UPower doesn't say.
fn keyboard_name() -> Option<String> {
    let led = std::fs::read_dir("/sys/class/leds")
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .find(|name| name.ends_with("kbd_backlight"))?;
    let vendor = led.split("::").next().filter(|v| !v.is_empty())?;
    let mut chars = vendor.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

pub async fn now() -> Option<Power> {
    let conn = zbus::Connection::system().await.ok()?;
    Some(Power { battery: battery(&conn).await, profiles: profiles(&conn).await, keyboard: keyboard(&conn).await })
}

pub fn subscription() -> Subscription<Power> {
    fn rules() -> Vec<zbus::MatchRule<'static>> {
        [
            super::signal(None, Some("/org/freedesktop/UPower/devices/DisplayDevice"), Some("org.freedesktop.DBus.Properties"), Some("PropertiesChanged")),
            // The battery itself: its charge limit turned on or off.
            super::signal(None, Some("/org/freedesktop/UPower/devices/battery_BAT1"), Some("org.freedesktop.DBus.Properties"), Some("PropertiesChanged")),
            super::signal(None, Some("/net/hadess/PowerProfiles"), Some("org.freedesktop.DBus.Properties"), Some("PropertiesChanged")),
            super::signal(None, Some("/org/freedesktop/UPower/KbdBacklight"), Some("org.freedesktop.UPower.KbdBacklight"), Some("BrightnessChanged")),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
    Subscription::run(|| super::watch(Duration::from_secs(30), || super::bus_changes(true, rules), now))
}

/// The charge limit on or off, on the battery that has one.
pub async fn set_charge_limit(enabled: bool) {
    let set = async {
        let conn = zbus::Connection::system().await?;
        for path in UPowerProxy::new(&conn).await?.enumerate_devices().await? {
            let device = AnyDeviceProxy::builder(&conn).path(path)?.build().await?;
            if device.kind().await.ok() == Some(2) && device.charge_threshold_supported().await.unwrap_or(false) {
                return device.enable_charge_threshold(enabled).await;
            }
        }
        Err(zbus::Error::Failure("no battery with a charge limit".to_owned()))
    };
    if let Err(err) = set.await {
        tracing::warn!("cannot set the charge limit: {err}");
    }
}

pub async fn set_profile(profile: String) {
    let set = async {
        let conn = zbus::Connection::system().await?;
        PowerProfilesProxy::new(&conn).await?.set_active_profile(&profile).await
    };
    if let Err(err) = set.await {
        tracing::warn!("cannot set the power profile: {err}");
    }
}

pub async fn set_keyboard(level: i32) {
    static LATEST: Latest<i32> = Latest::new();
    LATEST
        .send(level, |level| async move {
            let set = async {
                let conn = zbus::Connection::system().await?;
                KbdBacklightProxy::new(&conn).await?.set_brightness(level).await
            };
            if let Err(err) = set.await {
                tracing::warn!("cannot set the keyboard backlight: {err}");
            }
        })
        .await;
}
