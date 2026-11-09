// SPDX-License-Identifier: GPL-3.0-only

//! Wi-Fi and the cable, through NetworkManager.

use std::{collections::HashMap, time::Duration};

use cosmic::iced::Subscription;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

pub type Settings = HashMap<String, HashMap<String, OwnedValue>>;

/// What's connected, and whether Wi-Fi is on at all.
#[derive(Clone, Debug, PartialEq)]
pub struct Network {
    /// A Wi-Fi adapter is there at all.
    pub has_wifi: bool,
    /// A wired adapter is there at all: which.
    pub wired_device: Option<OwnedObjectPath>,
    /// The saved connections for the cable, and which are up.
    pub wired_connections: Vec<WiredConnection>,
    pub wifi_enabled: bool,
    /// The Wi-Fi network joined, if one is.
    pub wifi: Option<Connected>,
    /// The cable's connection, if one is up.
    pub wired: Option<Connected>,
    /// A connection on its way up.
    pub connecting: bool,
    /// The DNS of the connection in use (Wi-Fi or cable), to show and change.
    pub dns: Option<super::dns::Dns>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WiredConnection {
    pub name: String,
    pub settings: OwnedObjectPath,
    /// Its active connection, when it's up.
    pub active: Option<OwnedObjectPath>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Connected {
    pub name: String,
    /// The active connection, to drop it.
    pub active: OwnedObjectPath,
    /// Its saved settings.
    pub settings: OwnedObjectPath,
    /// Signal, for Wi-Fi.
    pub strength: Option<u8>,
}

impl Network {
    pub fn wifi_status(&self) -> String {
        match &self.wifi {
            Some(wifi) => wifi.name.clone(),
            None if self.connecting => "Connecting…".to_owned(),
            None if self.wifi_enabled => "Not connected".to_owned(),
            None => "Off".to_owned(),
        }
    }

    pub fn wifi_icon(&self) -> &'static str {
        match &self.wifi {
            Some(wifi) => match wifi.strength.unwrap_or(0) {
                s if s >= 80 => "network-wireless-signal-excellent-symbolic",
                s if s >= 55 => "network-wireless-signal-good-symbolic",
                s if s >= 30 => "network-wireless-signal-ok-symbolic",
                s if s > 5 => "network-wireless-signal-weak-symbolic",
                _ => "network-wireless-signal-none-symbolic",
            },
            None if self.wifi_enabled => "network-wireless-offline-symbolic",
            None => "network-wireless-disabled-symbolic",
        }
    }

    pub fn wired_status(&self) -> String {
        match &self.wired {
            Some(wired) => wired.name.clone(),
            None => "Disconnected".to_owned(),
        }
    }
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager"
)]
pub(crate) trait Manager {
    #[zbus(property)]
    fn wireless_enabled(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn set_wireless_enabled(&self, enabled: bool) -> zbus::Result<()>;
    #[zbus(property)]
    fn active_connections(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
    fn get_devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
    fn activate_connection(
        &self,
        connection: &ObjectPath<'_>,
        device: &ObjectPath<'_>,
        specific_object: &ObjectPath<'_>,
    ) -> zbus::Result<OwnedObjectPath>;
    fn add_and_activate_connection(
        &self,
        connection: HashMap<&str, HashMap<&str, Value<'_>>>,
        device: &ObjectPath<'_>,
        specific_object: &ObjectPath<'_>,
    ) -> zbus::Result<(OwnedObjectPath, OwnedObjectPath)>;
    fn deactivate_connection(&self, active: &ObjectPath<'_>) -> zbus::Result<()>;
}

#[zbus::proxy(interface = "org.freedesktop.NetworkManager.Connection.Active", default_service = "org.freedesktop.NetworkManager")]
pub(crate) trait Active {
    #[zbus(property)]
    fn id(&self) -> zbus::Result<String>;
    #[zbus(property, name = "Type")]
    fn kind(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn connection(&self) -> zbus::Result<OwnedObjectPath>;
    /// 1 activating, 2 activated, 3 deactivating.
    #[zbus(property)]
    fn state(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
    #[zbus(property)]
    fn specific_object(&self) -> zbus::Result<OwnedObjectPath>;
    #[zbus(property)]
    fn ip4_config(&self) -> zbus::Result<OwnedObjectPath>;
}

#[zbus::proxy(interface = "org.freedesktop.NetworkManager.IP4Config", default_service = "org.freedesktop.NetworkManager")]
pub(crate) trait Ip4Config {
    #[zbus(property)]
    fn nameserver_data(&self) -> zbus::Result<Vec<HashMap<String, zbus::zvariant::OwnedValue>>>;
}

#[zbus::proxy(interface = "org.freedesktop.NetworkManager.Device", default_service = "org.freedesktop.NetworkManager")]
pub(crate) trait Device {
    /// 1 ethernet, 2 Wi-Fi.
    #[zbus(property)]
    fn device_type(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn interface(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn hw_address(&self) -> zbus::Result<String>;
    fn reapply(&self, connection: HashMap<&str, HashMap<&str, Value<'_>>>, version_id: u64, flags: u32) -> zbus::Result<()>;
}

#[zbus::proxy(interface = "org.freedesktop.NetworkManager.Device.Wireless", default_service = "org.freedesktop.NetworkManager")]
trait Wireless {
    #[zbus(property)]
    fn active_access_point(&self) -> zbus::Result<OwnedObjectPath>;
    #[zbus(property)]
    fn access_points(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
    fn request_scan(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<()>;
}

#[zbus::proxy(interface = "org.freedesktop.NetworkManager.AccessPoint", default_service = "org.freedesktop.NetworkManager")]
trait Ap {
    #[zbus(property)]
    fn ssid(&self) -> zbus::Result<Vec<u8>>;
    #[zbus(property)]
    fn strength(&self) -> zbus::Result<u8>;
    #[zbus(property)]
    fn flags(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn wpa_flags(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn rsn_flags(&self) -> zbus::Result<u32>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Settings",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager/Settings"
)]
trait SettingsManager {
    fn list_connections(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
}

#[zbus::proxy(interface = "org.freedesktop.NetworkManager.Settings.Connection", default_service = "org.freedesktop.NetworkManager")]
pub(crate) trait Connection {
    fn get_settings(&self) -> zbus::Result<Settings>;
    fn update(&self, properties: HashMap<&str, HashMap<&str, Value<'_>>>) -> zbus::Result<()>;
}

fn none() -> ObjectPath<'static> {
    ObjectPath::from_static_str_unchecked("/")
}

async fn read(conn: &zbus::Connection) -> zbus::Result<Network> {
    let nm = ManagerProxy::builder(conn).cache_properties(zbus::proxy::CacheProperties::No).build().await?;
    let mut network = Network {
        has_wifi: false,
        wired_device: None,
        wired_connections: Vec::new(),
        wifi_enabled: nm.wireless_enabled().await.unwrap_or(false),
        wifi: None,
        wired: None,
        connecting: false,
        dns: None,
    };
    // The Ethernet ports there are, by interface and address: a saved wired
    // connection bound to another (a dock's, a USB adapter's) isn't shown.
    let mut ports: Vec<Port> = Vec::new();
    for device in nm.get_devices().await.unwrap_or_default() {
        let Ok(builder) = DeviceProxy::builder(conn).path(device.clone()) else {
            continue;
        };
        let Ok(proxy) = builder.build().await else {
            continue;
        };
        match proxy.device_type().await {
            Ok(1) => {
                network.wired_device.get_or_insert(device.clone());
                ports.push(Port { interface: proxy.interface().await.unwrap_or_default(), address: proxy.hw_address().await.unwrap_or_default().to_uppercase() });
            }
            Ok(2) => network.has_wifi = true,
            _ => {}
        }
    }
    for path in nm.active_connections().await.unwrap_or_default() {
        let Ok(active) = ActiveProxy::builder(conn).path(path.clone())?.cache_properties(zbus::proxy::CacheProperties::No).build().await else {
            continue;
        };
        let (Ok(kind), Ok(state)) = (active.kind().await, active.state().await) else {
            continue;
        };
        if state == 1 {
            network.connecting = true;
        }
        if state != 2 && state != 1 {
            continue;
        }
        let connected = Connected {
            name: active.id().await.unwrap_or_default(),
            active: path.clone(),
            settings: active.connection().await.unwrap_or_else(|_| none().into()),
            strength: None,
        };
        match kind.as_str() {
            "802-11-wireless" => {
                let mut connected = connected;
                if let Ok(ap) = active.specific_object().await {
                    if ap.as_str() != "/" {
                        if let Ok(ap) = ApProxy::builder(conn).path(ap)?.build().await {
                            connected.strength = ap.strength().await.ok();
                        }
                    }
                }
                network.wifi_enabled = true;
                network.wifi = Some(connected);
            }
            "802-3-ethernet" => network.wired = Some(connected),
            _ => {}
        }
    }
    if network.wired_device.is_some() {
        network.wired_connections = wired_connections(conn, network.wired.as_ref(), &ports).await;
    }
    // The DNS of the cable, else of the Wi-Fi.
    if let Some(physical) = network.wired.as_ref().or(network.wifi.as_ref()) {
        network.dns = super::dns::current(conn, physical.active.clone(), physical.settings.clone()).await.ok();
    }
    Ok(network)
}

pub async fn now() -> Option<Network> {
    let conn = zbus::Connection::system().await.ok()?;
    read(&conn).await.ok()
}

pub fn subscription() -> Subscription<Network> {
    fn rules() -> Vec<zbus::MatchRule<'static>> {
        super::signal(Some("org.freedesktop.NetworkManager"), None, Some("org.freedesktop.DBus.Properties"), Some("PropertiesChanged")).into_iter().collect()
    }
    Subscription::run(|| super::watch(Duration::from_secs(20), || super::bus_changes(true, rules), now))
}

pub async fn set_wifi(enabled: bool) -> zbus::Result<()> {
    let conn = zbus::Connection::system().await?;
    ManagerProxy::new(&conn).await?.set_wireless_enabled(enabled).await
}

/// The cable brought up: a saved connection, or whatever NetworkManager
/// picks for the device.
pub async fn connect_wired(device: OwnedObjectPath, connection: Option<OwnedObjectPath>) -> zbus::Result<()> {
    let conn = zbus::Connection::system().await?;
    let connection = connection.map(ObjectPath::from).unwrap_or_else(none);
    ManagerProxy::new(&conn).await?.activate_connection(&connection, &device, &none()).await.map(|_| ())
}

/// An active connection dropped.
pub async fn disconnect(active: OwnedObjectPath) -> zbus::Result<()> {
    let conn = zbus::Connection::system().await?;
    ManagerProxy::new(&conn).await?.deactivate_connection(&active).await
}

/// A Wi-Fi network in reach.
#[derive(Clone, Debug, PartialEq)]
pub struct AccessPoint {
    pub path: OwnedObjectPath,
    pub ssid: String,
    pub strength: u8,
    pub secure: bool,
    pub active: bool,
    /// A saved connection for it.
    pub saved: Option<OwnedObjectPath>,
}

impl AccessPoint {
    pub fn icon(&self) -> &'static str {
        match self.strength {
            s if s >= 80 => "network-wireless-signal-excellent-symbolic",
            s if s >= 55 => "network-wireless-signal-good-symbolic",
            s if s >= 30 => "network-wireless-signal-ok-symbolic",
            s if s > 5 => "network-wireless-signal-weak-symbolic",
            _ => "network-wireless-signal-none-symbolic",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Networks {
    pub device: OwnedObjectPath,
    pub list: Vec<AccessPoint>,
}

async fn wifi_device(conn: &zbus::Connection) -> zbus::Result<Option<OwnedObjectPath>> {
    for device in ManagerProxy::new(conn).await?.get_devices().await? {
        let proxy = DeviceProxy::builder(conn).path(device.clone())?.build().await?;
        if proxy.device_type().await? == 2 {
            return Ok(Some(device));
        }
    }
    Ok(None)
}

/// The cable's saved connections, with the one in use marked.
/// An Ethernet port present: its interface name and hardware address.
struct Port {
    interface: String,
    address: String,
}

/// Whether a wired connection can run on one of the ports there are: it is
/// bound to none (any port will do), or to one by name or address.
fn has_port(settings: &Settings, ports: &[Port]) -> bool {
    let interface = settings.get("connection").and_then(|g| g.get("interface-name")).and_then(|v| String::try_from(v.clone()).ok()).unwrap_or_default();
    let address = settings
        .get("802-3-ethernet")
        .and_then(|g| g.get("mac-address"))
        .and_then(|v| Vec::<u8>::try_from(v.clone()).ok())
        .map(|bytes| bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":"))
        .unwrap_or_default();
    ports.iter().any(|port| (interface.is_empty() || interface == port.interface) && (address.is_empty() || address == port.address))
}

async fn wired_connections(conn: &zbus::Connection, up: Option<&Connected>, ports: &[Port]) -> Vec<WiredConnection> {
    let mut list = Vec::new();
    let Ok(settings) = SettingsManagerProxy::new(conn).await else {
        return list;
    };
    for path in settings.list_connections().await.unwrap_or_default() {
        let Ok(builder) = ConnectionProxy::builder(conn).path(path.clone()) else {
            continue;
        };
        let Ok(connection) = builder.build().await else {
            continue;
        };
        let Ok(settings) = connection.get_settings().await else {
            continue;
        };
        let Some(group) = settings.get("connection") else {
            continue;
        };
        let kind = group.get("type").and_then(|v| String::try_from(v.clone()).ok()).unwrap_or_default();
        if kind != "802-3-ethernet" {
            continue;
        }
        let active = up.filter(|c| c.settings == path).map(|c| c.active.clone());
        if active.is_none() && !has_port(&settings, ports) {
            continue;
        }
        let name = group.get("id").and_then(|v| String::try_from(v.clone()).ok()).unwrap_or_else(|| "Wired".to_owned());
        list.push(WiredConnection { name, settings: path, active });
    }
    list.sort_by(|a, b| b.active.is_some().cmp(&a.active.is_some()).then_with(|| a.name.cmp(&b.name)));
    list
}

/// Saved Wi-Fi connections, by SSID.
async fn saved_wifi(conn: &zbus::Connection) -> HashMap<String, OwnedObjectPath> {
    let mut saved = HashMap::new();
    let Ok(settings) = SettingsManagerProxy::new(conn).await else {
        return saved;
    };
    for path in settings.list_connections().await.unwrap_or_default() {
        let Ok(connection) = ConnectionProxy::builder(conn).path(path.clone()).and_then(|b| Ok(b.build())) else {
            continue;
        };
        let Ok(connection) = connection.await else {
            continue;
        };
        let Ok(settings) = connection.get_settings().await else {
            continue;
        };
        let ssid = settings
            .get("802-11-wireless")
            .and_then(|w| w.get("ssid"))
            .and_then(|v| Vec::<u8>::try_from(v.clone()).ok());
        if let Some(ssid) = ssid {
            saved.insert(String::from_utf8_lossy(&ssid).into_owned(), path);
        }
    }
    saved
}

/// The networks in reach: the one joined first, then saved ones, then by
/// signal. `scan` asks for a fresh look first.
pub async fn networks(scan: bool) -> zbus::Result<Option<Networks>> {
    let conn = zbus::Connection::system().await?;
    let Some(device) = wifi_device(&conn).await? else {
        return Ok(None);
    };
    let wireless = WirelessProxy::builder(&conn).path(device.clone())?.cache_properties(zbus::proxy::CacheProperties::No).build().await?;
    if scan {
        // Refused when one just ran; the list is fresh then anyway.
        _ = wireless.request_scan(HashMap::new()).await;
    }
    let active = wireless.active_access_point().await?;
    let saved = saved_wifi(&conn).await;
    let mut list: Vec<AccessPoint> = Vec::new();
    for path in wireless.access_points().await? {
        let ap = ApProxy::builder(&conn).path(path.clone())?.build().await?;
        let ssid = String::from_utf8_lossy(&ap.ssid().await.unwrap_or_default()).into_owned();
        if ssid.is_empty() {
            continue;
        }
        let strength = ap.strength().await.unwrap_or(0);
        let secure = ap.flags().await.unwrap_or(0) & 1 != 0 || ap.wpa_flags().await.unwrap_or(0) != 0 || ap.rsn_flags().await.unwrap_or(0) != 0;
        let is_active = path == active;
        // One row per network, from its strongest access point.
        if let Some(known) = list.iter_mut().find(|n| n.ssid == ssid) {
            known.active |= is_active;
            if strength > known.strength {
                known.strength = strength;
                known.path = path;
            }
            continue;
        }
        list.push(AccessPoint { saved: saved.get(&ssid).cloned(), path, ssid, strength, secure, active: is_active });
    }
    list.sort_by_key(|n| (!n.active, n.saved.is_none(), std::cmp::Reverse(n.strength)));
    Ok(Some(Networks { device, list }))
}

pub enum Joined {
    Connecting,
    /// Wants a password, which only COSMIC Settings asks for.
    NeedsSettings,
}

pub async fn join(device: OwnedObjectPath, network: AccessPoint) -> zbus::Result<Joined> {
    let conn = zbus::Connection::system().await?;
    let nm = ManagerProxy::new(&conn).await?;
    if let Some(saved) = &network.saved {
        nm.activate_connection(saved, &device, &network.path).await?;
        return Ok(Joined::Connecting);
    }
    if network.secure {
        return Ok(Joined::NeedsSettings);
    }
    nm.add_and_activate_connection(HashMap::new(), &device, &network.path).await?;
    Ok(Joined::Connecting)
}

#[cfg(test)]
mod live_tests {
    /// `cargo test --release --lib network::live_tests -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn wired_connections_here() {
        let network = super::now().await.expect("NetworkManager answers");
        for connection in &network.wired_connections {
            println!("wired: {} (up: {})", connection.name, connection.active.is_some());
        }
    }
}
