// SPDX-License-Identifier: GPL-3.0-only

//! The displays: their modes, the one in use, and variable refresh, as
//! `cosmic-randr` lists them (in KDL).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    /// Refresh rate in millihertz: 165000.
    pub millihertz: u32,
    pub current: bool,
    pub preferred: bool,
}

impl Mode {
    pub fn hertz(&self) -> f32 {
        self.millihertz as f32 / 1000.0
    }

    /// "165 Hz", "59.95 Hz".
    pub fn hertz_label(&self) -> String {
        let hz = self.hertz();
        if (hz - hz.round()).abs() < 0.05 {
            format!("{:.0} Hz", hz)
        } else {
            format!("{:.2} Hz", hz)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    /// The connector: "eDP-1".
    pub name: String,
    /// Make and model: "BOE NE160QDM-NZ6".
    pub model: String,
    pub enabled: bool,
    pub scale: f32,
    pub vrr_supported: bool,
    /// "false", "automatic" or "true", as cosmic-randr has it.
    pub vrr: String,
    pub modes: Vec<Mode>,
}

impl Output {
    pub fn current(&self) -> Option<&Mode> {
        self.modes.iter().find(|m| m.current)
    }
}

/// The value of `key=` in a KDL line: `current=#true`, `model="BOE"`.
fn attribute<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let at = line.find(&format!("{key}="))? + key.len() + 1;
    let rest = &line[at..];
    if let Some(quoted) = rest.strip_prefix('"') {
        return quoted.split('"').next();
    }
    rest.split_whitespace().next()
}

/// The first quoted string on a line: `output "eDP-1" …`, `adaptive_sync "automatic"`.
fn quoted(line: &str) -> Option<&str> {
    line.split('"').nth(1)
}

pub fn parse(kdl: &str) -> Vec<Output> {
    let mut outputs: Vec<Output> = Vec::new();
    for line in kdl.lines().map(str::trim) {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("output") => outputs.push(Output {
                name: quoted(line).unwrap_or_default().to_owned(),
                model: String::new(),
                enabled: attribute(line, "enabled") == Some("#true"),
                scale: 1.0,
                vrr_supported: false,
                vrr: "false".to_owned(),
                modes: Vec::new(),
            }),
            Some(word) => {
                let Some(output) = outputs.last_mut() else {
                    continue;
                };
                match word {
                    "description" => {
                        let make = attribute(line, "make").unwrap_or_default();
                        let model = attribute(line, "model").unwrap_or_default();
                        output.model = format!("{make} {model}").trim().to_owned();
                    }
                    "scale" => {
                        output.scale = words.next().and_then(|s| s.parse().ok()).unwrap_or(1.0)
                    }
                    "adaptive_sync_support" => output.vrr_supported = words.next() == Some("#true"),
                    "adaptive_sync" => output.vrr = quoted(line).unwrap_or("false").to_owned(),
                    "mode" => {
                        let numbers: Vec<u32> =
                            words.take(3).filter_map(|n| n.parse().ok()).collect();
                        if let [width, height, millihertz] = numbers[..] {
                            output.modes.push(Mode {
                                width,
                                height,
                                millihertz,
                                current: attribute(line, "current") == Some("#true"),
                                preferred: attribute(line, "preferred") == Some("#true"),
                            });
                        }
                    }
                    _ => {}
                }
            }
            None => {}
        }
    }
    outputs
}

/// The outputs as cosmic-randr lists them; empty when it can't say.
pub async fn now() -> Vec<Output> {
    let output = tokio::process::Command::new("cosmic-randr")
        .args(["list", "--kdl"])
        .env_remove("LD_LIBRARY_PATH")
        .output()
        .await;
    match output {
        Ok(output) if output.status.success() => parse(&String::from_utf8_lossy(&output.stdout)),
        Ok(output) => {
            tracing::warn!(
                "cosmic-randr list failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            Vec::new()
        }
        Err(err) => {
            tracing::warn!("cannot run cosmic-randr: {err}");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = r#"output "eDP-1" enabled=#true {
  description make="BOE" model="NE160QDM-NZ6"
  physical 340 220
  position 0 0
  scale 1.25
  transform "normal"
  adaptive_sync_support #true
  adaptive_sync "automatic"
  xwayland_primary #true
  modes {
    mode 2560 1600 165000 current=#true preferred=#true
    mode 2560 1600 60002
    mode 1920 1200 165000
    mode 1920 1080 165000
  }
}"#;

    #[test]
    fn reads_the_listing() {
        let outputs = parse(LISTING);
        assert_eq!(outputs.len(), 1);
        let output = &outputs[0];
        assert_eq!(output.name, "eDP-1");
        assert_eq!(output.model, "BOE NE160QDM-NZ6");
        assert!(output.vrr_supported);
        assert_eq!(output.vrr, "automatic");
        assert_eq!(output.scale, 1.25);
        assert_eq!(
            output.current().map(|m| (m.width, m.millihertz)),
            Some((2560, 165000))
        );
        assert_eq!(
            output.current().map(Mode::hertz_label).as_deref(),
            Some("165 Hz")
        );
        assert_eq!(output.modes[1].hertz_label(), "60 Hz");
    }
}
