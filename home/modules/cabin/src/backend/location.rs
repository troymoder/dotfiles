// SPDX-License-Identifier: GPL-3.0-only

//! Where the machine is: the coordinates from GeoClue (city accuracy), the
//! place's name from OpenStreetMap's Nominatim. Asked at most every few
//! hours; Cape Town when either can't say.
//!
//! GeoClue only answers apps it knows: the system config lists the clock's
//! desktop id (`services.geoclue2.appConfig."dev.troy.Cabin.Clock"`).

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use futures::StreamExt;
use zbus::zvariant::OwnedObjectPath;

#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// Where the weather is for when GeoClue can't say.
fn fallback() -> Place {
    Place {
        name: "Cape Town".to_owned(),
        latitude: -33.9249,
        longitude: 18.4241,
    }
}

/// How long a place found is kept before asking again.
const KEEP: Duration = Duration::from_secs(3 * 3600);

/// GeoClue's accuracy levels: country 1, city 4, neighbourhood 5, street 6.
const ACCURACY_CITY: u32 = 4;

#[zbus::proxy(
    interface = "org.freedesktop.GeoClue2.Manager",
    default_service = "org.freedesktop.GeoClue2",
    default_path = "/org/freedesktop/GeoClue2/Manager"
)]
trait Manager {
    fn get_client(&self) -> zbus::Result<OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "org.freedesktop.GeoClue2.Client",
    default_service = "org.freedesktop.GeoClue2"
)]
trait Client {
    fn start(&self) -> zbus::Result<()>;
    fn stop(&self) -> zbus::Result<()>;
    #[zbus(property)]
    fn location(&self) -> zbus::Result<OwnedObjectPath>;
    #[zbus(property)]
    fn set_desktop_id(&self, id: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn set_requested_accuracy_level(&self, level: u32) -> zbus::Result<()>;
    #[zbus(property)]
    fn set_distance_threshold(&self, metres: u32) -> zbus::Result<()>;
    #[zbus(signal)]
    fn location_updated(
        &self,
        old: zbus::zvariant::ObjectPath<'_>,
        new: zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.freedesktop.GeoClue2.Location",
    default_service = "org.freedesktop.GeoClue2"
)]
trait Location {
    #[zbus(property)]
    fn latitude(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn longitude(&self) -> zbus::Result<f64>;
}

/// The coordinates GeoClue gives, once: a client is made, started, read
/// from its first update and stopped.
async fn coordinates() -> zbus::Result<(f64, f64)> {
    let conn = zbus::Connection::system().await?;
    let client_path = ManagerProxy::new(&conn).await?.get_client().await?;
    let client = ClientProxy::builder(&conn)
        .path(client_path)?
        .build()
        .await?;
    client.set_desktop_id(crate::CLOCK_ID).await?;
    client.set_requested_accuracy_level(ACCURACY_CITY).await?;
    client.set_distance_threshold(5000).await?;
    let mut updates = client.receive_location_updated().await?;
    client.start().await?;
    // A location already known is in the property; else the first update.
    let path = match client.location().await {
        Ok(path) if path.as_str() != "/" => path,
        _ => {
            let update = tokio::time::timeout(Duration::from_secs(30), updates.next())
                .await
                .map_err(|_| zbus::Error::Failure("GeoClue gave no location in 30 s".to_owned()))?
                .ok_or_else(|| zbus::Error::Failure("GeoClue's update stream ended".to_owned()))?;
            OwnedObjectPath::from(update.args()?.new.clone())
        }
    };
    let location = LocationProxy::builder(&conn).path(path)?.build().await?;
    let found = (location.latitude().await?, location.longitude().await?);
    let _ = client.stop().await;
    Ok(found)
}

/// The place's name, from Nominatim: the city, else the town, the village,
/// the county or the state.
async fn name_of(latitude: f64, longitude: f64) -> Option<String> {
    let url = format!(
        "https://nominatim.openstreetmap.org/reverse?lat={latitude:.4}&lon={longitude:.4}&format=jsonv2&zoom=10&accept-language=en"
    );
    // Nominatim asks every client to name itself.
    let output = tokio::process::Command::new("curl")
        .args([
            "-sfL",
            "--max-time",
            "10",
            "-A",
            "cabin-clock/0.1 (COSMIC panel applet)",
            &url,
        ])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let address = &json["address"];
    ["city", "town", "village", "municipality", "county", "state"]
        .iter()
        .find_map(|key| address[*key].as_str())
        .or_else(|| json["name"].as_str())
        .map(str::to_owned)
}

/// Where the machine is now, from what was found in the last hours or
/// asked again; Cape Town when GeoClue can't say.
pub async fn current() -> Place {
    static KNOWN: Mutex<Option<(Instant, Place)>> = Mutex::new(None);
    if let Some((at, place)) = KNOWN.lock().ok().and_then(|known| known.clone()) {
        if at.elapsed() < KEEP {
            return place;
        }
    }
    let place = match coordinates().await {
        Ok((latitude, longitude)) => {
            let name = name_of(latitude, longitude)
                .await
                .unwrap_or_else(|| format!("{latitude:.2}, {longitude:.2}"));
            Place {
                name,
                latitude,
                longitude,
            }
        }
        Err(err) => {
            tracing::warn!("no location from GeoClue: {err}");
            return fallback();
        }
    };
    if let Ok(mut known) = KNOWN.lock() {
        *known = Some((Instant::now(), place.clone()));
    }
    place
}
