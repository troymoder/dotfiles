// SPDX-License-Identifier: GPL-3.0-only

//! The weather, from Open-Meteo, where the machine is (`location`).

use std::time::Duration;

use cosmic::iced::Subscription;

#[derive(Clone, Debug, PartialEq)]
pub struct Hour {
    /// "16:00", or "Now".
    pub label: String,
    pub celsius: f64,
    pub code: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Weather {
    /// Where it is for: "Cape Town".
    pub place: String,
    pub celsius: f64,
    pub code: u32,
    pub wind_kmh: f64,
    pub wind_from: String,
    pub hours: Vec<Hour>,
}

/// What a WMO code means, and the icon for it.
pub fn describe(code: u32) -> (&'static str, &'static str) {
    match code {
        0 => ("Clear", "sun"),
        1 => ("Mostly clear", "sun"),
        2 => ("Partly cloudy", "cloud"),
        3 => ("Overcast", "cloud"),
        45 | 48 => ("Fog", "cloud-fog"),
        51..=57 => ("Drizzle", "cloud-rain"),
        61..=67 | 80..=82 => ("Rain", "cloud-rain"),
        71..=77 | 85 | 86 => ("Snow", "cloud-snow"),
        95..=99 => ("Thunderstorm", "cloud-storm"),
        _ => ("Unsettled", "cloud"),
    }
}

fn compass(degrees: f64) -> &'static str {
    const POINTS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    POINTS[(((degrees + 22.5) / 45.0).floor() as usize) % 8]
}

pub async fn now() -> Option<Weather> {
    let place = super::location::current().await;
    let (latitude, longitude) = (place.latitude, place.longitude);
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={latitude:.4}&longitude={longitude:.4}&current=temperature_2m,weather_code,wind_speed_10m,wind_direction_10m&hourly=temperature_2m,weather_code&timezone=auto&forecast_days=2"
    );
    let output = tokio::process::Command::new("curl").args(["-sfL", "--max-time", "10", &url]).output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let current = &json["current"];
    let now_time = current["time"].as_str()?;
    let times = json["hourly"]["time"].as_array()?;
    let temps = json["hourly"]["temperature_2m"].as_array()?;
    let codes = json["hourly"]["weather_code"].as_array()?;
    // The next hours from the current one on, every two hours.
    let start = times.iter().position(|t| t.as_str().is_some_and(|t| t >= now_time)).unwrap_or(0);
    let mut hours = vec![Hour { label: "Now".to_owned(), celsius: current["temperature_2m"].as_f64()?, code: current["weather_code"].as_u64()? as u32 }];
    for i in (start + 2..times.len()).step_by(2).take(4) {
        let (Some(t), Some(c), Some(code)) = (times[i].as_str(), temps[i].as_f64(), codes[i].as_u64()) else {
            continue;
        };
        hours.push(Hour { label: t.get(11..16).unwrap_or("").to_owned(), celsius: c, code: code as u32 });
    }
    Some(Weather {
        place: place.name,
        celsius: current["temperature_2m"].as_f64()?,
        code: current["weather_code"].as_u64()? as u32,
        wind_kmh: current["wind_speed_10m"].as_f64().unwrap_or(0.0),
        wind_from: compass(current["wind_direction_10m"].as_f64().unwrap_or(0.0)).to_owned(),
        hours,
    })
}

pub fn subscription() -> Subscription<Weather> {
    Subscription::run(|| super::watch(Duration::from_secs(1800), super::no_changes, now))
}
