// SPDX-License-Identifier: GPL-3.0-only

//! cabin-ec on the system bus (system/modules/cabin-ec): what only root can
//! read, as far as the panels need it. The charger as the Framework EC agreed
//! it (past the 20 V the kernel hears of), and the CPU package's power.
//! Nothing where it doesn't run.

#[zbus::proxy(interface = "dev.troy.Cabin.EC1", default_service = "dev.troy.Cabin.EC", default_path = "/dev/troy/Cabin/EC")]
trait CabinEc {
    fn charger_watts(&self) -> zbus::Result<f64>;
    fn cpu_watts(&self) -> zbus::Result<f64>;
}

async fn proxy(conn: &zbus::Connection) -> Option<CabinEcProxy<'_>> {
    CabinEcProxy::builder(conn).cache_properties(zbus::proxy::CacheProperties::No).build().await.ok()
}

/// What the EC agreed with the charger, when one is plugged in.
pub async fn charger_watts(conn: &zbus::Connection) -> Option<f32> {
    let watts = proxy(conn).await?.charger_watts().await.ok()?;
    (watts > 1.0).then_some(watts as f32)
}

/// The CPU package's power, averaged over a second or more.
pub async fn cpu_watts(conn: &zbus::Connection) -> Option<f32> {
    let watts = proxy(conn).await?.cpu_watts().await.ok()?;
    (watts >= 0.0).then_some(watts as f32)
}
