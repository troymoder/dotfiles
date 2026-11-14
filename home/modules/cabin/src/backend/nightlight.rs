// SPDX-License-Identifier: GPL-3.0-only

//! Night light, through cosmic-nightlight (COSMIC has none of its own yet):
//! what's on screen from the record it keeps in the runtime directory, its
//! warmth from its config, and its own `--toggle` to switch it, which keeps
//! its applet and schedule in step.

/// What's on screen: the tint's temperature in kelvin, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NightLight {
    pub on: Option<u32>,
    /// The warmth it's set to, shown while it's off.
    pub temperature: u32,
}

/// `<state> <brightness> <displays>`, the state "off" or kelvin.
fn parse_shown(text: &str) -> Option<Option<u32>> {
    match text.split_whitespace().next()? {
        "off" => Some(None),
        kelvin => kelvin.parse().ok().map(Some),
    }
}

/// `None` without cosmic-nightlight installed.
pub fn read() -> Option<NightLight> {
    let installed = std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| dir.join("cosmic-nightlight").exists())
    });
    if !installed {
        return None;
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
    let on = std::fs::read_to_string(format!("{runtime}/cosmic-nightlight-shown"))
        .ok()
        .as_deref()
        .and_then(parse_shown)
        .flatten();
    let home = std::env::var("HOME").unwrap_or_default();
    let config = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{home}/.config"));
    let temperature = std::fs::read_to_string(format!(
        "{config}/cosmic/io.github.cosmic_nightlight/v1/temperature"
    ))
    .ok()
    .and_then(|t| t.trim().parse().ok())
    .unwrap_or(4500);
    Some(NightLight { on, temperature })
}

/// The tint on or off, as its keyboard shortcut does. It takes a second or
/// two: the screen flickers while it's written.
pub async fn toggle() {
    let result = tokio::process::Command::new("cosmic-nightlight")
        .arg("--toggle")
        .env_remove("LD_LIBRARY_PATH")
        .output()
        .await;
    match result {
        Ok(out) if out.status.success() => {}
        Ok(out) => tracing::warn!(
            "cosmic-nightlight --toggle: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(err) => tracing::warn!("cannot run cosmic-nightlight: {err}"),
    }
}

/// Its own settings: warmth, brightness, schedule.
pub fn settings() {
    super::spawn("cosmic-nightlight", &["--settings"], &[]);
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_the_record() {
        assert_eq!(super::parse_shown("4500 1 eDP-1\n"), Some(Some(4500)));
        assert_eq!(super::parse_shown("off 1 eDP-1"), Some(None));
        assert_eq!(super::parse_shown(""), None);
    }
}
