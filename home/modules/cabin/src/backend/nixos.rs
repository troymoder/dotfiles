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

/// A derivation the Nix daemon is building right now.
#[derive(Clone, Debug, PartialEq)]
pub struct Build {
    /// Its builder, nix-daemon's child.
    pub pid: u32,
    /// The derivation's name: "linux-6.12.10".
    pub name: String,
    /// When its builder started.
    pub started: std::time::SystemTime,
    /// Its processes' share of the whole machine's CPU, in percent.
    pub cpu: f32,
    /// Its processes' resident memory, in bytes.
    pub memory: u64,
    /// The program using the most of its CPU: "cc1plus". None while it
    /// uses none.
    pub busiest: Option<String>,
}

/// The derivation a sandboxed builder (a child of nix-daemon) works on,
/// from its `/proc/<pid>/mountinfo`, which anyone may read: its root is
/// the chroot "/nix/store/<hash>-<name>.drv.chroot".
pub fn build_name(mountinfo: &str) -> Option<String> {
    mountinfo.lines().find_map(|line| {
        let mut fields = line.split(' ').skip(3);
        let (root, point) = (fields.next()?, fields.next()?);
        if point != "/" {
            return None;
        }
        let chroot = root.rsplit('/').next()?.strip_suffix(".drv.chroot")?;
        Some(chroot.split_once('-')?.1.to_owned())
    })
}

/// btop with the CPU over its process list as a tree, saving nothing: the
/// user's own settings stay as they are. Not the process list alone: btop
/// 1.4.8 then paints its "No boxes shown!" screen over it at the first
/// click (an unchanged list redraws to nothing, and only the CPU box's
/// clock keeps a frame from being empty). No GPU readout: it could wake
/// the sleeping dGPU.
const BTOP_CONFIG: &str = "shown_boxes = \"cpu proc\"\nproc_tree = true\nshow_gpu_info = \"Off\"\n\
    save_config_on_exit = false\nupdate_ms = 1000\n";

/// A terminal with btop on a build's processes: every build runs as its
/// own build user (nixbld1, …), so they're that user's.
pub fn inspect(build: &Build) {
    use std::os::unix::fs::MetadataExt;
    // The build may have just ended.
    let Ok(uid) = std::fs::metadata(format!("/proc/{}", build.pid)).map(|meta| meta.uid()) else {
        return;
    };
    let uid = uid.to_string();
    let user = std::fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|passwd| {
            passwd.lines().find_map(|line| {
                let mut fields = line.split(':');
                let name = fields.next()?;
                (fields.nth(1)? == uid).then(|| name.to_owned())
            })
        })
        .unwrap_or(uid);
    let config = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("cabin-btop.conf");
    if let Err(err) = std::fs::write(&config, BTOP_CONFIG) {
        return tracing::error!("cannot write {}: {err}", config.display());
    }
    // A regex, so nixbld1 doesn't take in nixbld10's processes too.
    super::spawn(
        "ghostty",
        &[
            &format!("--title=Building {}", build.name),
            "-e",
            "btop",
            "-c",
            &config.to_string_lossy(),
            "-f",
            &format!("!^{user}$"),
        ],
        &[],
    );
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

    #[test]
    fn builds_from_mounts() {
        let mounts = "1476 1446 254:1 /nix/store/3z680dlqpjp9xfs94wqpbf352p206r2y-cabin-probe-1.drv.chroot / rw,relatime - xfs /dev/mapper/vg0-root rw\n\
            1477 1476 254:1 /nix/store/3z680dlqpjp9xfs94wqpbf352p206r2y-cabin-probe-1.drv.chroot/nix/store /nix/store rw - xfs /dev/mapper/vg0-root rw\n";
        assert_eq!(build_name(mounts).as_deref(), Some("cabin-probe-1"));
        assert_eq!(
            build_name("22 1 0:21 / / rw,relatime - btrfs /dev/nvme0n1p2 rw\n"),
            None
        );
    }
}
