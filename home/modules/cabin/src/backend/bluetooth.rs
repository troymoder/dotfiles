// SPDX-License-Identifier: GPL-3.0-only

//! Bluetooth through BlueZ: the adapter's power, and the devices paired
//! with it.

use std::{collections::HashMap, time::Duration};

use cosmic::iced::Subscription;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub path: OwnedObjectPath,
    pub name: String,
    pub icon: &'static str,
    pub connected: bool,
    pub battery: Option<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bluetooth {
    pub adapter: OwnedObjectPath,
    pub powered: bool,
    /// Paired devices, connected ones first.
    pub devices: Vec<Device>,
}

impl Bluetooth {
    pub fn connected(&self) -> impl Iterator<Item = &Device> {
        self.devices.iter().filter(|d| d.connected)
    }

    pub fn status(&self) -> String {
        if !self.powered {
            return "Off".to_owned();
        }
        let names: Vec<&str> = self.connected().map(|d| d.name.as_str()).collect();
        match names.len() {
            0 => "On".to_owned(),
            1 => names[0].to_owned(),
            n => format!("{n} devices"),
        }
    }
}

#[zbus::proxy(interface = "org.freedesktop.DBus.ObjectManager", default_service = "org.bluez", default_path = "/")]
trait ObjectManager {
    fn get_managed_objects(&self) -> zbus::Result<HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>>;
}

#[zbus::proxy(interface = "org.bluez.Adapter1", default_service = "org.bluez")]
trait Adapter {
    #[zbus(property)]
    fn set_powered(&self, powered: bool) -> zbus::Result<()>;
}

#[zbus::proxy(interface = "org.bluez.Device1", default_service = "org.bluez")]
trait Device1 {
    fn connect(&self) -> zbus::Result<()>;
    fn disconnect(&self) -> zbus::Result<()>;
}

fn device_icon(kind: &str) -> &'static str {
    match kind {
        "audio-headset" | "audio-headphones" => "audio-headphones-symbolic",
        "audio-card" => "audio-speakers-symbolic",
        "input-keyboard" => "input-keyboard-symbolic",
        "input-mouse" => "input-mouse-symbolic",
        "input-gaming" => "input-gaming-symbolic",
        "input-tablet" => "input-tablet-symbolic",
        "phone" => "phone-symbolic",
        "computer" => "computer-symbolic",
        _ => "bluetooth-symbolic",
    }
}

fn string(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props.get(key).and_then(|v| String::try_from(v.clone()).ok())
}

fn flag(props: &HashMap<String, OwnedValue>, key: &str) -> bool {
    props.get(key).and_then(|v| bool::try_from(v).ok()).unwrap_or(false)
}

pub async fn now() -> Option<Option<Bluetooth>> {
    let conn = zbus::Connection::system().await.ok()?;
    let dbus = zbus::fdo::DBusProxy::new(&conn).await.ok()?;
    if !dbus.name_has_owner("org.bluez".try_into().ok()?).await.ok()? {
        return Some(None);
    }
    let objects = ObjectManagerProxy::new(&conn).await.ok()?.get_managed_objects().await.ok()?;
    let (adapter, props) = objects.iter().find_map(|(path, ifaces)| ifaces.get("org.bluez.Adapter1").map(|p| (path.clone(), p)))?;
    let powered = flag(props, "Powered");
    let mut devices: Vec<Device> = objects
        .iter()
        .filter_map(|(path, ifaces)| {
            let props = ifaces.get("org.bluez.Device1")?;
            if !flag(props, "Paired") {
                return None;
            }
            let battery = ifaces.get("org.bluez.Battery1").and_then(|b| b.get("Percentage")).and_then(|v| u8::try_from(v).ok());
            Some(Device {
                path: path.clone(),
                name: string(props, "Alias").or_else(|| string(props, "Name")).unwrap_or_else(|| "Device".to_owned()),
                icon: device_icon(&string(props, "Icon").unwrap_or_default()),
                connected: flag(props, "Connected"),
                battery,
            })
        })
        .collect();
    devices.sort_by(|a, b| b.connected.cmp(&a.connected).then_with(|| a.name.cmp(&b.name)));
    Some(Some(Bluetooth { adapter, powered, devices }))
}

pub fn subscription() -> Subscription<Option<Bluetooth>> {
    fn rules() -> Vec<zbus::MatchRule<'static>> {
        ["PropertiesChanged", "InterfacesAdded", "InterfacesRemoved"]
            .into_iter()
            .filter_map(|member| super::signal(Some("org.bluez"), None, None, Some(member)))
            .collect()
    }
    Subscription::run(|| super::watch(Duration::from_secs(30), || super::bus_changes(true, rules), now))
}

pub async fn set_powered(adapter: OwnedObjectPath, powered: bool) -> zbus::Result<()> {
    let conn = zbus::Connection::system().await?;
    AdapterProxy::builder(&conn).path(adapter)?.build().await?.set_powered(powered).await
}

/// Connects or disconnects; BlueZ answers once it's done or has given up.
pub async fn connect(device: OwnedObjectPath, connect: bool) -> zbus::Result<()> {
    let conn = zbus::Connection::system().await?;
    let device = Device1Proxy::builder(&conn).path(device)?.build().await?;
    if connect { device.connect().await } else { device.disconnect().await }
}
