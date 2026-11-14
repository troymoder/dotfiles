// SPDX-License-Identifier: GPL-3.0-only

//! The system's generation and how old its nixpkgs is: a nudge to update
//! without opening a terminal. Plain file reads, cheap enough to do often.

use std::path::Path;

use chrono::{Local, NaiveDate};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NixOs {
    /// The system profile's generation: 141.
    pub generation: u32,
    /// How long ago it was built, in days.
    pub built_days: Option<i64>,
    /// How old the nixpkgs it was built from is, in days, by the date in its
    /// version ("26.11.20261106.4fe25e1").
    pub nixpkgs_days: Option<i64>,
    /// A newer generation is in place than the one booted: a reboot finishes
    /// the switch (kernel, firmware, services that don't restart).
    pub reboot_pending: bool,
    /// The kernel booted and the one a reboot brings, when they differ:
    /// ("6.12.8", "6.12.10").
    pub kernels: Option<(String, String)>,
}

/// A kernel's version from its store path: ".../xxx-linux-6.12.10/bzImage".
fn kernel_version(path: &Path) -> Option<String> {
    let dir = path.parent()?.file_name()?.to_str()?;
    Some(dir.split_once("-linux-")?.1.to_owned())
}

/// The date in a NixOS version string: its third part, YYYYMMDD.
fn version_date(version: &str) -> Option<NaiveDate> {
    let date = version.trim().split('.').nth(2)?;
    NaiveDate::parse_from_str(date.get(..8)?, "%Y%m%d").ok()
}

pub fn read() -> Option<NixOs> {
    let link = std::fs::read_link("/nix/var/nix/profiles/system").ok()?;
    let generation: u32 = link
        .to_str()?
        .strip_prefix("system-")?
        .strip_suffix("-link")?
        .parse()
        .ok()?;
    let today = Local::now().date_naive();
    let built_days = std::fs::symlink_metadata(Path::new("/nix/var/nix/profiles").join(&link))
        .and_then(|meta| meta.modified())
        .ok()
        .map(|time| (today - chrono::DateTime::<Local>::from(time).date_naive()).num_days());
    let nixpkgs_days = std::fs::read_to_string("/run/current-system/nixos-version")
        .ok()
        .as_deref()
        .and_then(version_date)
        .map(|date| (today - date).num_days());
    // The booted system and the current one on different kernels or initrds:
    // only a reboot brings the new ones in.
    let differs = |part: &str| match (
        std::fs::canonicalize(format!("/run/booted-system/{part}")),
        std::fs::canonicalize(format!("/run/current-system/{part}")),
    ) {
        (Ok(booted), Ok(current)) => booted != current,
        _ => false,
    };
    let reboot_pending = differs("kernel") || differs("initrd");
    let kernel = |system: &str| {
        std::fs::canonicalize(format!("{system}/kernel"))
            .ok()
            .as_deref()
            .and_then(kernel_version)
    };
    let kernels = match (kernel("/run/booted-system"), kernel("/run/current-system")) {
        (Some(booted), Some(current)) if booted != current => Some((booted, current)),
        _ => None,
    };
    Some(NixOs {
        generation,
        built_days,
        nixpkgs_days,
        reboot_pending,
        kernels,
    })
}

/// How old the nixpkgs lock may get before it's time to update.
pub const STALE_DAYS: i64 = 14;

/// Something the system pins at the top of the notifications until it's
/// done: it can't be dismissed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub icon: &'static str,
    pub title: String,
    pub body: String,
    /// A button on it, and what it does.
    pub action: Option<(&'static str, NoticeAction)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeAction {
    Restart,
}

/// What the system's state asks for: a reboot to finish a switch, an
/// update when the nixpkgs lock is old.
pub fn notices(nixos: &NixOs) -> Vec<Notice> {
    let mut notices = Vec::new();
    if nixos.reboot_pending {
        let body = match &nixos.kernels {
            Some((booted, current)) => format!("Kernel {booted} → {current}"),
            None => "The system switched to a new kernel or initrd".to_owned(),
        };
        notices.push(Notice {
            icon: "alert-triangle",
            title: "Reboot to finish update".to_owned(),
            body,
            action: Some(("Restart", NoticeAction::Restart)),
        });
    }
    if let Some(age) = nixos.nixpkgs_days.filter(|age| *age > STALE_DAYS) {
        let mut body = format!("The nixpkgs lock is {age} days old");
        if let Some(built) = nixos.built_days {
            body.push_str(&format!(
                " · generation {} built {}",
                nixos.generation,
                days(built)
            ));
        }
        notices.push(Notice {
            icon: "refresh",
            title: "Time to update".to_owned(),
            body,
            action: None,
        });
    }
    notices
}

/// Days as words: "today", "yesterday", "12 days".
pub fn days(n: i64) -> String {
    match n {
        ..=0 => "today".to_owned(),
        1 => "yesterday".to_owned(),
        n => format!("{n} days ago"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_from_versions() {
        assert_eq!(
            version_date("26.11.20261106.4fe25e1\n"),
            NaiveDate::from_ymd_opt(2026, 11, 6)
        );
        assert_eq!(version_date("garbage"), None);
        assert_eq!(days(12), "12 days ago");
        assert_eq!(
            kernel_version(Path::new("/nix/store/abc-linux-6.12.10/bzImage")).as_deref(),
            Some("6.12.10")
        );
    }
}
