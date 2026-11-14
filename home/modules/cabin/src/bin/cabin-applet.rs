// SPDX-License-Identifier: GPL-3.0-only

//! A button on the panel, by kind. All but the workspace dots draw what
//! cabind tells them and ask it to open their panel.

use clap::{Parser, ValueEnum};

/// One of Cabin's buttons on the COSMIC panel.
#[derive(Parser)]
#[command(name = "cabin-applet", version)]
struct Cli {
    #[arg(long, value_enum)]
    kind: Kind,
}

#[derive(Clone, Copy, ValueEnum)]
enum Kind {
    /// The status icons and the privacy pill: the controls and audio panels.
    Status,
    /// The date and time: the date menu.
    Clock,
    /// CPU and memory bars: the System panel.
    System,
    /// The workspace dots: the overview, and the wheel to switch.
    Workspaces,
}

fn main() -> cosmic::iced::Result {
    cabin::hand_over_to_dev();
    cabin::prefer_integrated_gpu();
    let cli = Cli::parse();
    cabin::init_logging();
    match cli.kind {
        Kind::Status => cosmic::applet::run::<cabin::applet::Status>(()),
        Kind::Clock => cosmic::applet::run::<cabin::applet::Clock>(()),
        Kind::System => cosmic::applet::run::<cabin::applet::System>(()),
        Kind::Workspaces => cosmic::applet::run::<cabin::workspaces::WorkspacesApp>(()),
    }
}
