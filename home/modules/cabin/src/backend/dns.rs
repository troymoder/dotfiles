// SPDX-License-Identifier: GPL-3.0-only

//! The DNS servers of the connection in use: the network's own, or a
//! provider's, saved in the connection and applied at once.

use std::{
    collections::HashMap,
    net::{Ipv4Addr, Ipv6Addr},
};

use zbus::zvariant::{OwnedObjectPath, Value};

use super::network::{ActiveProxy, ConnectionProxy, DeviceProxy, Ip4ConfigProxy, Settings};

pub struct Provider {
    pub name: &'static str,
    pub ipv4: &'static [&'static str],
    pub ipv6: &'static [&'static str],
}

pub const PROVIDERS: &[Provider] = &[
    Provider { name: "Cloudflare", ipv4: &["1.1.1.1", "1.0.0.1"], ipv6: &["2606:4700:4700::1111", "2606:4700:4700::1001"] },
    Provider { name: "Quad9", ipv4: &["9.9.9.9", "149.112.112.112"], ipv6: &["2620:fe::fe", "2620:fe::9"] },
    Provider { name: "Google", ipv4: &["8.8.8.8", "8.8.4.4"], ipv6: &["2001:4860:4860::8888", "2001:4860:4860::8844"] },
    Provider { name: "AdGuard", ipv4: &["94.140.14.14", "94.140.15.15"], ipv6: &["2a10:50c0::ad1:ff", "2a10:50c0::ad2:ff"] },
];

/// Whose servers: the network's, a provider's, or some set by hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    Automatic,
    Provider(usize),
    Custom(Vec<String>),
}

impl Choice {
    pub fn name(&self) -> String {
        match self {
            Choice::Automatic => "Automatic".to_owned(),
            Choice::Provider(i) => PROVIDERS[*i].name.to_owned(),
            Choice::Custom(servers) => servers.first().cloned().unwrap_or_else(|| "Custom".to_owned()),
        }
    }

    fn of(automatic: bool, servers: &[String]) -> Choice {
        if automatic || servers.is_empty() {
            return Choice::Automatic;
        }
        match PROVIDERS.iter().position(|p| servers.iter().any(|s| p.ipv4.contains(&s.as_str()))) {
            Some(i) => Choice::Provider(i),
            None => Choice::Custom(servers.to_vec()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dns {
    /// The connection's saved settings.
    pub settings: OwnedObjectPath,
    /// Its active connection, to reapply through.
    pub active: OwnedObjectPath,
    pub choice: Choice,
    /// The servers in use now: the network's (by DHCP) or the ones set.
    pub servers: Vec<String>,
}

/// Whether the network's servers are taken, and which are set.
fn read(settings: &Settings) -> (bool, Vec<String>) {
    let Some(ipv4) = settings.get("ipv4") else {
        return (true, Vec::new());
    };
    let automatic = !ipv4.get("ignore-auto-dns").and_then(|v| bool::try_from(v).ok()).unwrap_or(false);
    // Newer NetworkManager says them as text too.
    let data: Option<Vec<String>> = ipv4.get("dns-data").and_then(|v| <Vec<String>>::try_from(v.try_clone().ok()?).ok());
    let servers = match data {
        Some(data) if !data.is_empty() => data,
        _ => ipv4
            .get("dns")
            .and_then(|v| <Vec<u32>>::try_from(v.try_clone().ok()?).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|n| Ipv4Addr::from(n.to_ne_bytes()).to_string())
            .collect(),
    };
    (automatic, servers)
}

pub async fn current(conn: &zbus::Connection, active: OwnedObjectPath, settings: OwnedObjectPath) -> zbus::Result<Dns> {
    let connection = ConnectionProxy::builder(conn).path(settings.clone())?.build().await?;
    let (automatic, servers) = read(&connection.get_settings().await?);
    let in_use = in_use(conn, &active).await.unwrap_or_default();
    Ok(Dns { settings, active, choice: Choice::of(automatic, &servers), servers: in_use })
}

/// The IPv4 servers the connection resolves through now, as NetworkManager
/// applied them.
async fn in_use(conn: &zbus::Connection, active: &OwnedObjectPath) -> zbus::Result<Vec<String>> {
    let active = ActiveProxy::builder(conn).path(active.clone())?.build().await?;
    let config = active.ip4_config().await?;
    if config.as_str() == "/" {
        return Ok(Vec::new());
    }
    let config = Ip4ConfigProxy::builder(conn).path(config)?.build().await?;
    Ok(config.nameserver_data().await?.iter().filter_map(|server| String::try_from(server.get("address")?.try_clone().ok()?).ok()).collect())
}

fn ipv4_number(address: Ipv4Addr) -> u32 {
    u32::from_ne_bytes(address.octets())
}

/// The connection's settings with the choice's servers in them.
fn with_choice(settings: Settings, choice: &Choice) -> HashMap<String, HashMap<String, Value<'static>>> {
    let (ipv4, ipv6): (Vec<Ipv4Addr>, Vec<Ipv6Addr>) = match choice {
        Choice::Automatic => (Vec::new(), Vec::new()),
        Choice::Provider(i) => (
            PROVIDERS[*i].ipv4.iter().filter_map(|a| a.parse().ok()).collect(),
            PROVIDERS[*i].ipv6.iter().filter_map(|a| a.parse().ok()).collect(),
        ),
        Choice::Custom(servers) => (
            servers.iter().filter_map(|a| a.parse().ok()).collect(),
            servers.iter().filter_map(|a| a.parse().ok()).collect(),
        ),
    };
    let by_hand = !matches!(choice, Choice::Automatic);
    let mut settings: HashMap<String, HashMap<String, Value<'static>>> = settings
        .into_iter()
        .map(|(group, values)| (group, values.into_iter().map(|(key, value)| (key, Value::from(value))).collect()))
        .collect();
    let group = settings.entry("ipv4".to_owned()).or_default();
    let knows_data = group.contains_key("dns-data");
    group.insert("dns".to_owned(), Value::from(ipv4.iter().map(|a| ipv4_number(*a)).collect::<Vec<u32>>()));
    if knows_data {
        group.insert("dns-data".to_owned(), Value::from(ipv4.iter().map(ToString::to_string).collect::<Vec<String>>()));
    }
    group.insert("ignore-auto-dns".to_owned(), Value::from(by_hand));
    // IPv6 only where the connection uses it.
    if let Some(group) = settings.get_mut("ipv6") {
        let method = group.get("method").and_then(|v| String::try_from(v.try_clone().ok()?).ok()).unwrap_or_default();
        if !matches!(method.as_str(), "ignore" | "disabled") {
            let knows_data = group.contains_key("dns-data");
            group.insert("dns".to_owned(), Value::from(ipv6.iter().map(|a| a.octets().to_vec()).collect::<Vec<Vec<u8>>>()));
            if knows_data {
                group.insert("dns-data".to_owned(), Value::from(ipv6.iter().map(ToString::to_string).collect::<Vec<String>>()));
            }
            group.insert("ignore-auto-dns".to_owned(), Value::from(by_hand && !ipv6.is_empty()));
        }
    }
    settings
}

/// The choice saved in the connection and applied to its devices now.
pub async fn set(dns: Dns, choice: Choice) -> zbus::Result<()> {
    let conn = zbus::Connection::system().await?;
    let connection = ConnectionProxy::builder(&conn).path(dns.settings.clone())?.build().await?;
    let settings = with_choice(connection.get_settings().await?, &choice);
    let borrowed: HashMap<&str, HashMap<&str, Value<'_>>> = settings
        .iter()
        .map(|(group, values)| {
            (group.as_str(), values.iter().map(|(key, value)| (key.as_str(), value.try_clone().expect("no file descriptors"))).collect())
        })
        .collect();
    // Without secrets, NetworkManager keeps the ones it has.
    connection.update(borrowed).await?;
    let active = super::network::ActiveProxy::builder(&conn).path(dns.active.clone())?.build().await?;
    for device in active.devices().await.unwrap_or_default() {
        let proxy = DeviceProxy::builder(&conn).path(device.clone())?.build().await?;
        if let Err(err) = proxy.reapply(HashMap::new(), 0, 0).await {
            tracing::warn!("cannot apply the DNS servers at once, reconnecting: {err}");
            let none = zbus::zvariant::ObjectPath::from_static_str_unchecked("/");
            super::network::ManagerProxy::new(&conn).await?.activate_connection(&dns.settings, &device, &none).await?;
        }
    }
    Ok(())
}
