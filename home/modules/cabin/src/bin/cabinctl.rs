// SPDX-License-Identifier: GPL-3.0-only

//! cabinctl: cabind from a shell, as the panel buttons reach it.

use clap::{Parser, Subcommand, ValueEnum};

/// Drive Cabin's panels from a shell, over the session bus.
#[derive(Parser)]
#[command(name = "cabinctl", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Open a panel, or close it if it's what's open.
    Toggle { panel: Panel },
    /// Open a panel, with one of its menus open (controls only: wifi,
    /// bluetooth, privacy, camera, battery, …).
    Open { panel: Panel, menu: Option<String> },
    /// Close what's open; pinned panels stay.
    Close,
    /// The volume up or down by steps of 5 %, with the on-screen bar.
    Volume {
        #[arg(allow_hyphen_values = true)]
        steps: f64,
    },
    /// What a panel button draws, as JSON.
    Bar { kind: Bar },
    /// The window switcher up, or its pick moved on: what Alt+Tab and
    /// Alt+Shift+Tab run. Letting go of the modifier raises the pick.
    Switch {
        direction: Direction,
        /// Cards with a picture of each window (Super+Tab), not icons.
        #[arg(long)]
        previews: bool,
    },
    /// The workspace overview up, or down if it's up: what Super runs.
    Overview,
}

#[derive(Clone, Copy, ValueEnum)]
enum Direction {
    Next,
    Prev,
}

#[derive(Clone, Copy, ValueEnum)]
enum Panel {
    Controls,
    Date,
    System,
}

impl Panel {
    fn name(self) -> &'static str {
        match self {
            Panel::Controls => "controls",
            Panel::Date => "date",
            Panel::System => "system",
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Bar {
    Status,
    Clock,
    System,
}

impl Bar {
    fn name(self) -> &'static str {
        match self {
            Bar::Status => "status",
            Bar::Clock => "clock",
            Bar::System => "system",
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    cabin::hand_over_to_dev();
    let cli = Cli::parse();
    let Some(daemon) = cabin::bus::daemon().await else {
        eprintln!(
            "cabinctl: cannot reach {} on the session bus",
            cabin::bus::NAME
        );
        std::process::exit(1);
    };
    let result = match cli.command {
        Command::Toggle { panel } => daemon.toggle(panel.name()).await,
        Command::Open { panel, menu } => {
            daemon
                .open(panel.name(), menu.as_deref().unwrap_or_default())
                .await
        }
        Command::Close => daemon.close().await,
        Command::Volume { steps } => daemon.volume(steps).await,
        Command::Overview => daemon.overview().await,
        Command::Bar { kind } => daemon.bar(kind.name()).await.map(|bar| println!("{bar}")),
        Command::Switch {
            direction,
            previews,
        } => {
            daemon
                .switch(
                    if matches!(direction, Direction::Prev) {
                        -1
                    } else {
                        1
                    },
                    previews,
                )
                .await
        }
    };
    if let Err(err) = result {
        eprintln!("cabinctl: {err}");
        std::process::exit(1);
    }
}
