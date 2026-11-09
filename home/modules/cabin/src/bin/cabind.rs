// SPDX-License-Identifier: GPL-3.0-only

//! The daemon: every panel's state, and the panels themselves on a
//! layer-shell overlay. Started with the session; the panel buttons and
//! cabinctl reach it on the session bus as dev.troy.Cabin.

fn main() -> cosmic::iced::Result {
    cabin::prefer_integrated_gpu();
    cabin::init_logging();
    cabin::daemon::run()
}
