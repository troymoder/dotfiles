// SPDX-License-Identifier: GPL-3.0-only

//! The controls pane: the top row of buttons, the brightness slider, the
//! tiles two by two; each with its menu under its row.

use cosmic::{
    Element,
    iced::{Alignment, Background, Length},
    theme, widget,
};

use super::{
    INNER_GAP, Lit, Row, SliderRow, action_button, card, card_header_with, check, danger_button, faint_icon, heading, icon, inherit, menu_row, meter, note, palette, pill, registry, round_label, row_item,
    rule, sparkline::Sparkline, square_button, switch, text, tile, under,
};
use crate::{
    app::{Adjusting, App, Card, Message, Pending, duration},
    backend::{
        camera::{self, Bus},
        dns::{Choice, PROVIDERS},
        gpu::{self, Status},
        graphics::Graphics,
        idle::{self, Timeout},
        power::Profiles,
    },
};

impl App {
    pub(crate) fn main_pane(&self) -> Element<'_, Message> {
        let mut rows: Vec<Element<'_, Message>> = vec![self.buttons_row()];
        self.push_card(&mut rows, &[Card::Battery, Card::Session, Card::PowerMenu]);
        if let Some(slider) = self.brightness_row() {
            rows.push(widget::container(slider).padding([6, 0]).class(inherit()).into());
            self.push_card(&mut rows, &[Card::Display]);
        }
        // The tiles there's hardware for, two to a row, each row's menu
        // under it.
        let mut tiles: Vec<(Element<'_, Message>, Option<Card>)> = Vec::new();
        let has_wifi = self.network.as_ref().is_some_and(|n| n.has_wifi);
        let has_wired = self.network.as_ref().is_some_and(|n| n.wired_device.is_some());
        if has_wired {
            tiles.push((self.wired_tile(), Some(Card::Wired)));
        }
        if has_wifi {
            tiles.push((self.wifi_tile(), Some(Card::Network)));
        }
        if self.bluetooth.is_some() {
            tiles.push((self.bluetooth_tile(), Some(Card::Bluetooth)));
        }
        if self.power.as_ref().is_some_and(|p| p.profiles.is_some()) {
            tiles.push((self.power_tile(), Some(Card::PowerMode)));
        }
        if self.graphics.is_some() {
            tiles.push((self.graphics_tile(), Some(Card::Graphics)));
        }
        if self.network.as_ref().is_some_and(|n| n.dns.is_some()) {
            tiles.push((self.dns_tile(), Some(Card::Dns)));
        }
        if self.cameras.as_ref().is_some_and(|c| !c.list.is_empty() || c.privacy_switch) {
            tiles.push((self.camera_tile(), Some(Card::Camera)));
        }
        tiles.push((self.privacy_tile(), Some(Card::Privacy)));
        if let Some(keyboard) = self.keyboard_tile() {
            tiles.push((keyboard, Some(Card::Keyboard)));
        }
        let mut tiles = tiles.into_iter();
        while let Some((left, left_card)) = tiles.next() {
            let (right, right_card) = tiles.next().map_or((widget::space::horizontal().into(), None), |(t, c)| (t, c));
            rows.push(pair(left, right));
            let owners: Vec<Card> = [left_card, right_card].into_iter().flatten().collect();
            self.push_card(&mut rows, &owners);
        }
        super::pane(widget::column::with_children(rows).spacing(INNER_GAP as u16))
    }

    /// The open menu, if it belongs to one of these: under their row.
    fn push_card<'a>(&'a self, rows: &mut Vec<Element<'a, Message>>, owners: &[Card]) {
        if let Some(open) = self.card.filter(|c| owners.contains(c)) {
            rows.push(card(self.card_content(open)));
        }
    }

    fn open(&self, card: Card) -> bool {
        self.card == Some(card)
    }

    // ---- the top row

    fn buttons_row(&self) -> Element<'_, Message> {
        let battery = match self.power.as_ref().and_then(|p| p.battery.as_ref()) {
            Some(battery) => round_label(&battery.icon(), battery.low().then_some(palette::LOW), format!("{:.0} %", battery.percentage), Some(Message::ToggleCard(Card::Battery)), self.open(Card::Battery)),
            None => square_button("battery", None, false),
        };
        widget::row::with_children(vec![
            battery,
            widget::space::horizontal().into(),
            square_button("camera", Some(Message::Screenshot), false),
            square_button("settings", Some(Message::Settings(None)), false),
            square_button("lock", Some(Message::ToggleCard(Card::Session)), self.open(Card::Session)),
            danger_button("power", Some(Message::ToggleCard(Card::PowerMenu))),
        ])
        .spacing(INNER_GAP as u16)
        .align_y(Alignment::Center)
        .into()
    }

    fn brightness_row(&self) -> Option<Element<'_, Message>> {
        let brightness = self.brightness?;
        let (value, dragging) = match &self.adjusting {
            Some(Adjusting::Brightness(level)) => (*level, true),
            _ => (brightness.level as f32 / brightness.max.max(1) as f32, false),
        };
        Some(
            SliderRow::new("sun", value, Message::SetBrightness)
                .snaps([0.25, 0.5, 0.75])
                .label(dragging.then(|| super::audio_pane::percent(value)))
                .on_release(Message::AdjustDone)
                .details(Some(Message::ToggleCard(Card::Display)), self.open(Card::Display))
                .view(),
        )
    }

    // ---- tiles

    fn wired_tile(&self) -> Element<'_, Message> {
        let (on, status) = match &self.network {
            Some(network) => {
                let up: Vec<&str> = network.wired_connections.iter().filter(|c| c.active.is_some()).map(|c| c.name.as_str()).collect();
                let status = match up.len() {
                    0 => network.wired_status(),
                    1 => up[0].to_owned(),
                    n => format!("{n} connections"),
                };
                (network.wired.is_some() || !up.is_empty(), status)
            }
            None => (false, "Unavailable".to_owned()),
        };
        let open = Message::ToggleCard(Card::Wired);
        tile("ethernet", "Wired", Some(status), Lit::when(on), Some(open.clone()), Some(open), self.open(Card::Wired))
    }

    fn wifi_tile(&self) -> Element<'_, Message> {
        let pending = self.pending(|p| matches!(p, Pending::Wifi(_)));
        let (on, status) = match (&self.network, pending) {
            (_, Some(Pending::Wifi(on))) => (*on, if *on { "Turning on…" } else { "Turning off…" }.to_owned()),
            (Some(network), _) => (network.wifi_enabled, network.wifi_status()),
            (None, _) => (false, "Unavailable".to_owned()),
        };
        let icon_name = self.network.as_ref().map_or("wifi-off", |n| n.wifi_icon());
        tile(icon_name, "Wi-Fi", Some(status), Lit::when(on), Some(Message::ToggleWifi), Some(Message::ToggleCard(Card::Network)), self.open(Card::Network))
    }

    fn bluetooth_tile(&self) -> Element<'_, Message> {
        let pending = self.pending(|p| matches!(p, Pending::Bluetooth(_)));
        let (on, status) = match (&self.bluetooth, pending) {
            (_, Some(Pending::Bluetooth(on))) => (*on, if *on { "Turning on…" } else { "Turning off…" }.to_owned()),
            (Some(bluetooth), _) => {
                let connected = bluetooth.connected().count();
                let status = match (bluetooth.powered, connected) {
                    (false, _) => "Off".to_owned(),
                    (true, 0) => "On".to_owned(),
                    (true, 1) => "1 device".to_owned(),
                    (true, n) => format!("{n} devices"),
                };
                (bluetooth.powered, status)
            }
            (None, _) => (false, "Unavailable".to_owned()),
        };
        let toggle = self.bluetooth.is_some().then_some(Message::ToggleBluetooth);
        tile("bluetooth", "Bluetooth", Some(status), Lit::when(on), toggle, Some(Message::ToggleCard(Card::Bluetooth)), self.open(Card::Bluetooth))
    }

    fn power_tile(&self) -> Element<'_, Message> {
        let profiles = self.power.as_ref().and_then(|p| p.profiles.as_ref());
        let active = match self.pending(|p| matches!(p, Pending::Profile(_))) {
            Some(Pending::Profile(profile)) => Some(profile.clone()),
            _ => profiles.map(|p| p.active.clone()),
        };
        let status = active.as_deref().map(Profiles::name).unwrap_or("Unavailable");
        let icon_name = active.as_deref().map_or("scale", Profiles::icon);
        let on = active.as_deref().is_some_and(|p| p != "balanced");
        // A press goes on to the next profile; the arrow lists them.
        let open = Message::ToggleCard(Card::PowerMode);
        tile(icon_name, "Power", Some(status.to_owned()), Lit::when(on), Some(Message::CycleProfile), Some(open), self.open(Card::PowerMode))
    }

    fn graphics_tile(&self) -> Element<'_, Message> {
        let (status, on) = match &self.graphics {
            Some(graphics) if graphics.pending() => (format!("{} after logging out", Graphics::mode_name(graphics.pending_mode)), graphics.mode != 1),
            Some(graphics) if graphics.mode == 1 => ("Integrated".to_owned(), false),
            Some(graphics) => {
                let status = match graphics.power {
                    0 => "dGPU active",
                    1 => "dGPU asleep",
                    2 | 3 => "dGPU off",
                    _ => Graphics::mode_name(graphics.mode),
                };
                (status.to_owned(), true)
            }
            None => ("Unavailable".to_owned(), false),
        };
        let open = self.graphics.is_some().then_some(Message::ToggleCard(Card::Graphics));
        // The dedicated GPU's chip while it's in play, the integrated one's in Integrated mode.
        let icon_name = if self.graphics.as_ref().is_some_and(|g| g.mode == 1) { "gpu-2" } else { "gpu" };
        tile(icon_name, "Graphics", Some(status), Lit::when(on), open.clone(), open, self.open(Card::Graphics))
    }

    fn dns_tile(&self) -> Element<'_, Message> {
        let dns = self.network.as_ref().and_then(|n| n.dns.as_ref());
        let choice = match self.pending(|p| matches!(p, Pending::Dns(_))) {
            Some(Pending::Dns(choice)) => Some(choice.clone()),
            _ => dns.map(|d| d.choice.clone()),
        };
        // The server in use: the network's, by DHCP, or the one set. While
        // a change is under way, the one asked for.
        let status = match (&choice, dns) {
            (None, _) => "No connection".to_owned(),
            (Some(Choice::Provider(i)), _) if dns.is_none_or(|d| d.choice != Choice::Provider(*i)) => PROVIDERS[*i].ipv4[0].to_owned(),
            (Some(choice), Some(dns)) => match dns.servers.first() {
                Some(server) if dns.servers.len() > 1 => format!("{server} +{}", dns.servers.len() - 1),
                Some(server) => server.clone(),
                None => choice.name(),
            },
            (Some(choice), None) => choice.name(),
        };
        let on = choice.as_ref().is_some_and(|c| *c != Choice::Automatic);
        let open = Message::ToggleCard(Card::Dns);
        tile("server", "DNS", Some(status), Lit::when(on), Some(open.clone()), Some(open), self.open(Card::Dns))
    }

    /// Who's using a camera, by app: PipeWire's word first, else the
    /// processes holding the device.
    fn camera_users(&self) -> Vec<String> {
        let mut users: Vec<String> = self.privacy.camera.iter().map(|u| registry::name(&u.app)).collect();
        if let Some(cameras) = &self.cameras {
            for camera in &cameras.list {
                for user in &camera.users {
                    let name = registry::name(&user.name);
                    if !users.contains(&name) {
                        users.push(name);
                    }
                }
            }
        }
        users
    }

    fn camera_tile(&self) -> Element<'_, Message> {
        let cameras = self.cameras.as_ref();
        let users = self.camera_users();
        let (status, on) = match cameras {
            Some(c) if c.list.is_empty() && c.privacy_switch => ("Privacy switch".to_owned(), false),
            Some(_) if users.len() == 1 => (format!("In use · {}", users[0]), true),
            Some(_) if users.len() > 1 => (format!("In use · {} apps", users.len()), true),
            Some(c) => (format!("Idle · {}", plural(c.list.len(), "camera")), false),
            None => ("Looking…".to_owned(), false),
        };
        let open = Message::ToggleCard(Card::Camera);
        tile(if on { "camera" } else { "camera-off" }, "Camera", Some(status), Lit::when(on), Some(open.clone()), Some(open), self.open(Card::Camera))
    }

    fn privacy_tile(&self) -> Element<'_, Message> {
        let privacy = &self.privacy;
        let camera = !self.camera_users().is_empty();
        let mic = !privacy.microphone.is_empty();
        let mic_muted = self.audio.as_ref().and_then(|a| a.input).is_some_and(|i| i.muted);
        let mut parts: Vec<&str> = Vec::new();
        if privacy.sharing_screen() {
            parts.push("Screen");
        }
        if camera {
            parts.push("Camera");
        }
        if mic {
            parts.push(if mic_muted { "Mic muted" } else { "Mic" });
        }
        let status = if privacy.sharing_screen() {
            if parts.len() > 1 { format!("Sharing screen +{}", parts.len() - 1) } else { "Sharing screen".to_owned() }
        } else if parts.is_empty() {
            "Nothing in use".to_owned()
        } else {
            parts.join(" · ")
        };
        let lit = if privacy.sharing_screen() {
            Lit::Coral
        } else if !parts.is_empty() {
            Lit::Amber
        } else {
            Lit::Off
        };
        let open = Message::ToggleCard(Card::Privacy);
        tile("shield-lock", "Privacy", Some(status), lit, Some(open.clone()), Some(open), self.open(Card::Privacy))
    }

    fn keyboard_tile(&self) -> Option<Element<'_, Message>> {
        let keyboard = self.power.as_ref()?.keyboard.as_ref()?;
        let status = level_name(keyboard.level, keyboard.max);
        Some(tile("keyboard", "Keyboard light", Some(status), Lit::when(keyboard.level > 0), Some(Message::ToggleKeyboard), Some(Message::ToggleCard(Card::Keyboard)), self.open(Card::Keyboard)))
    }

    // ---- menus

    fn card_content(&self, card: Card) -> Element<'_, Message> {
        let battery_icon = self.power.as_ref().and_then(|p| p.battery.as_ref()).map(|b| b.icon()).unwrap_or_else(|| "battery".to_owned());
        let wifi_on = self.network.as_ref().is_some_and(|n| n.wifi_enabled);
        let header: Option<(&str, &str)> = match card {
            Card::Battery => Some((battery_icon.as_str(), "Battery")),
            Card::Session => Some(("lock", "Session")),
            Card::PowerMenu => Some(("power", "Power")),
            Card::Network => Some((if wifi_on { "wifi" } else { "wifi-off" }, "Wi-Fi")),
            Card::Wired => Some(("ethernet", "Wired")),
            Card::Bluetooth => Some(("bluetooth", "Bluetooth")),
            Card::PowerMode => Some(("gauge", "Power mode")),
            Card::Graphics => Some(("gpu", "Graphics")),
            Card::Dns => Some(("server", "DNS")),
            Card::Camera => Some(("camera", "Camera")),
            Card::Privacy => Some(("shield-lock", "Privacy")),
            Card::Display => Some(("sun", "Display")),
            Card::Keyboard => Some(("keyboard", "Keyboard light")),
            Card::Output | Card::Input => None,
        };
        let right = match card {
            Card::Battery => self.power.as_ref().and_then(|p| p.battery.as_ref()).map(|b| format!("{:.0} %", b.percentage)),
            _ => None,
        };
        let mut body: Vec<Element<'_, Message>> = header.map(|(icon_name, title)| card_header_with(icon_name, title, right)).into_iter().collect();
        let rows: Vec<Element<'_, Message>> = match card {
            Card::Battery => self.battery_card(),
            Card::Session => vec![
                row_item(Some("lock"), "Lock".to_owned(), None, Some(Message::Lock)),
                row_item(Some("logout"), "Log out".to_owned(), None, Some(Message::LogOut)),
            ],
            Card::PowerMenu => vec![
                row_item(Some("moon"), "Suspend".to_owned(), None, Some(Message::Suspend)),
                row_item(Some("refresh"), "Restart".to_owned(), None, Some(Message::Restart)),
                row_item(Some("power"), "Shut down".to_owned(), None, Some(Message::PowerOff)),
            ],
            Card::Network => self.network_card(),
            Card::Wired => self.wired_card(),
            Card::Bluetooth => self.bluetooth_card(),
            Card::PowerMode => self.profiles_card(),
            Card::Graphics => self.graphics_card(),
            Card::Dns => self.dns_card(),
            Card::Camera => self.camera_card(),
            Card::Privacy => self.privacy_card(),
            Card::Display => self.display_card(),
            Card::Keyboard => match self.power.as_ref().and_then(|p| p.keyboard.as_ref()) {
                Some(keyboard) => {
                    let mut rows = vec![note(format!("{} keyboard", keyboard.name.as_deref().unwrap_or("Built-in")))];
                    rows.extend((0..=keyboard.max).map(|level| {
                        menu_row(None, level_name(level, keyboard.max), None, (level == keyboard.level).then(check), Some(Message::SetKeyboardLevel(level)), level == keyboard.level)
                    }));
                    rows
                }
                None => vec![note("No keyboard backlight")],
            },
            // The audio pane draws its own.
            Card::Output | Card::Input => Vec::new(),
        };
        body.extend(rows);
        widget::column::with_children(body).into()
    }

    fn battery_card(&self) -> Vec<Element<'_, Message>> {
        let mut rows = Vec::new();
        match self.power.as_ref().and_then(|p| p.battery.as_ref()) {
            Some(battery) => {
                let watts = format!("{:.1} W", battery.rate);
                let charger = battery.charger_w.map(|w| format!("USB-C · {w:.0} W charger"));
                let draw = self.system.as_ref().and_then(|s| s.draw.cpu_w).map(|cpu| {
                    let gpu = self.gpus.as_ref().and_then(|g| g.discrete.as_ref()).and_then(|g| match g.status {
                        Status::Active { power_w, .. } => power_w,
                        _ => None,
                    });
                    format!("System draw {:.1} W", cpu + gpu.unwrap_or(0.0) + 3.2)
                });
                if battery.full || battery.held {
                    let (title, sub) = match battery.limit.filter(|l| battery.held && l.enabled) {
                        Some(limit) => ("Not charging", format!("Plugged in · held at {} %", limit.end)),
                        None if battery.held => ("Not charging", "Plugged in · battery idle".to_owned()),
                        None => ("Fully charged", "Plugged in · battery idle".to_owned()),
                    };
                    rows.push(menu_row(Some("plug-connected"), title.to_owned(), Some(sub), None, None, false));
                    if let Some(w) = battery.charger_w {
                        rows.push(menu_row(Some("bolt"), format!("{w:.0} W charger"), Some("USB-C".to_owned()), None, None, false));
                    }
                    if let Some(draw) = draw {
                        rows.push(menu_row(Some("activity"), draw, None, None, None, false));
                    }
                } else if battery.charging {
                    let full_in = battery.time_left.map(|s| format!("{} in {}", battery.target_name(), duration(s))).unwrap_or_else(|| "Plugged in".to_owned());
                    rows.push(menu_row(Some("plug-connected"), "Charging".to_owned(), Some(full_in), None, None, false));
                    if battery.rate > 0.5 {
                        rows.push(menu_row(Some("bolt"), format!("Charging at {watts}"), charger, None, None, false));
                    }
                    if let Some(draw) = draw {
                        rows.push(menu_row(Some("activity"), draw, None, None, None, false));
                    }
                } else {
                    let left = battery.time_left.map(|s| format!("{} remaining", duration(s))).unwrap_or_else(|| "Time left unknown".to_owned());
                    rows.push(menu_row(Some(battery.icon().as_str()), "On battery".to_owned(), Some(left), None, None, false));
                    if battery.rate > 0.5 {
                        rows.push(menu_row(Some("activity"), format!("Drawing {watts}"), Some("Average over the last few minutes".to_owned()), None, None, false));
                    }
                }
                // The charge over the last hours, as UPower kept it.
                if battery.history.len() >= 2 {
                    let levels: Vec<f32> = battery.history.iter().map(|(_, level)| *level as f32).collect();
                    rows.push(
                        Row::new("Battery level")
                            .icon("activity")
                            .sub(format!("Last {} hours", crate::backend::power::HISTORY_HOURS))
                            .extra(under(Sparkline::fixed(&levels, 100.0).view()))
                            .view(),
                    );
                }
                // The charge limit, where the firmware has one: 80 % or full.
                if let Some(limit) = battery.limit {
                    rows.push(rule());
                    rows.push(
                        Row::new(format!("Limit charging to {} %", limit.end))
                            .icon("battery-eco")
                            .sub(if limit.enabled { "On: kinder to a battery that's mostly plugged in" } else { "Off: charging to 100 %" })
                            .trailing(switch(limit.enabled, Message::SetChargeLimit))
                            .on_press(Message::SetChargeLimit(!limit.enabled))
                            .view(),
                    );
                }
                if let Some(health) = battery.health {
                    rows.push(rule());
                    let cycles = battery.cycles.map(|c| format!("{c} charge cycles"));
                    rows.push(menu_row(Some("heart"), format!("Battery health {health:.0} %"), cycles, None, None, false));
                }
            }
            None => rows.push(note("No battery")),
        }
        rows.push(rule());
        rows.push(heading("Power saving"));
        rows.extend(self.idle_rows());
        rows.push(rule());
        rows.push(row_item(Some("settings"), "Power settings".to_owned(), None, Some(Message::Settings(Some("power")))));
        rows
    }

    /// cosmic-idle's timeouts, each a row with its time at the end that
    /// opens COSMIC Settings' choices under it.
    fn idle_rows(&self) -> Vec<Element<'_, Message>> {
        let mut rows = Vec::new();
        for (timeout, current) in Timeout::ALL.into_iter().zip(self.idle) {
            let open = self.idle_open == Some(timeout);
            let trailing = widget::row::with_children(vec![
                text::small(idle::label(current)).class(theme::Text::Custom(super::dimmed)).into(),
                faint_icon(if open { "chevron-down" } else { "chevron-right" }),
            ])
            .spacing(10)
            .align_y(Alignment::Center);
            rows.push(Row::new(timeout.title()).icon(timeout.icon()).trailing(trailing).on_press(Message::ToggleIdle(timeout)).view());
            if open {
                let choices = timeout.choices().iter().map(|m| Some(m * 60_000)).chain([None]);
                rows.extend(choices.map(|ms| {
                    let picked = ms == current;
                    Row::new(idle::label(ms)).indent().trailing_maybe(picked.then(check)).selected(picked).on_press(Message::SetIdle(timeout, ms)).view()
                }));
            }
        }
        rows
    }

    fn network_card(&self) -> Vec<Element<'_, Message>> {
        const OTHERS_SHOWN: usize = 3;
        let mut rows = Vec::new();
        let wifi_on = self.network.as_ref().is_some_and(|n| n.wifi_enabled);
        if !wifi_on {
            rows.push(Row::new("Wi-Fi is off").icon("wifi-off").dim(true).view());
            rows.push(rule());
            rows.push(row_item(Some("settings"), "All networks".to_owned(), None, Some(Message::Settings(Some("wireless")))));
            return rows;
        }
        match &self.networks {
            Some(networks) if !networks.list.is_empty() => {
                let active = self.network.as_ref().and_then(|n| n.wifi.as_ref()).map(|w| w.active.clone());
                let row = |network: &crate::backend::network::AccessPoint| {
                    let mut trailing: Vec<Element<'_, Message>> = Vec::new();
                    if network.secure {
                        trailing.push(faint_icon("lock"));
                    }
                    if network.active {
                        trailing.push(check());
                    }
                    let on_press = match (network.active, active.clone()) {
                        (true, Some(active)) => Message::Disconnect(active),
                        _ => Message::JoinWifi(network.clone()),
                    };
                    menu_row(
                        Some(network.icon()),
                        network.ssid.clone(),
                        None,
                        Some(widget::row::with_children(trailing).spacing(10).align_y(Alignment::Center).into()),
                        Some(on_press),
                        network.active,
                    )
                };
                let (known, others): (Vec<_>, Vec<_>) = networks.list.iter().partition(|n| n.saved.is_some() || n.active);
                rows.extend(known.iter().map(|n| row(n)));
                if !others.is_empty() {
                    if !known.is_empty() {
                        rows.push(rule());
                    }
                    rows.push(heading("Other networks"));
                    let shown = if self.wifi_all { others.len() } else { OTHERS_SHOWN };
                    rows.extend(others.iter().take(shown).map(|n| row(n)));
                    if others.len() > shown {
                        rows.push(pill(format!("{} more", others.len() - shown), Message::ShowAllWifi));
                    }
                }
            }
            Some(_) => rows.push(Row::new("No networks in reach").icon("wifi-off").dim(true).view()),
            None => rows.push(Row::new("Looking…").icon("wifi").dim(true).view()),
        }
        rows.push(rule());
        rows.push(row_item(Some("settings"), "All networks".to_owned(), None, Some(Message::Settings(Some("wireless")))));
        rows
    }

    fn wired_card(&self) -> Vec<Element<'_, Message>> {
        let mut rows = Vec::new();
        match &self.network {
            Some(network) if network.wired_connections.is_empty() => rows.push(Row::new("No wired connections").icon("ethernet").dim(true).view()),
            Some(network) => {
                for connection in &network.wired_connections {
                    let up = connection.active.is_some();
                    let settings = connection.settings.clone();
                    let toggle = switch(up, move |on| Message::SetWired(settings.clone(), on));
                    rows.push(row_item(Some("ethernet"), connection.name.clone(), Some(toggle), Some(Message::SetWired(connection.settings.clone(), !up))));
                }
            }
            None => rows.push(note("No network")),
        }
        rows.push(rule());
        rows.push(row_item(Some("settings"), "Network settings".to_owned(), None, Some(Message::Settings(Some("wired")))));
        rows
    }

    fn bluetooth_card(&self) -> Vec<Element<'_, Message>> {
        let mut rows = Vec::new();
        match &self.bluetooth {
            Some(bluetooth) if !bluetooth.powered => rows.push(Row::new("Bluetooth is off").icon("bluetooth").dim(true).view()),
            Some(bluetooth) if bluetooth.devices.is_empty() => rows.push(Row::new("No paired devices").icon("bluetooth").dim(true).view()),
            Some(bluetooth) => {
                for device in &bluetooth.devices {
                    let pending = self.pending(|p| matches!(p, Pending::Device(path, _) if *path == device.path));
                    let (connected, busy) = match pending {
                        Some(Pending::Device(_, on)) => (*on, true),
                        _ => (device.connected, false),
                    };
                    let mut trailing: Vec<Element<'_, Message>> = Vec::new();
                    if let (Some(battery), true) = (device.battery, connected) {
                        trailing.push(text::small(format!("{battery} %")).class(theme::Text::Custom(|_| cosmic::iced::widget::text::Style { color: Some(palette::rgba(232, 236, 246, 0.8)), ..Default::default() })).into());
                    }
                    if busy {
                        trailing.push(text::sub("…").into());
                    }
                    let path = device.path.clone();
                    trailing.push(switch(connected, move |on| Message::ConnectDevice(path.clone(), on)));
                    let path = device.path.clone();
                    rows.push(row_item(
                        Some(device.icon),
                        device.name.clone(),
                        Some(widget::row::with_children(trailing).spacing(8).align_y(Alignment::Center).into()),
                        Some(Message::ConnectDevice(path, !connected)),
                    ));
                }
            }
            None => rows.push(note("No Bluetooth")),
        }
        rows.push(rule());
        rows.push(row_item(Some("settings"), "Bluetooth settings".to_owned(), None, Some(Message::Settings(Some("bluetooth")))));
        rows
    }

    fn profiles_card(&self) -> Vec<Element<'_, Message>> {
        let Some(profiles) = self.power.as_ref().and_then(|p| p.profiles.as_ref()) else {
            return vec![note("No power profiles")];
        };
        let active = match self.pending(|p| matches!(p, Pending::Profile(_))) {
            Some(Pending::Profile(profile)) => profile.clone(),
            _ => profiles.active.clone(),
        };
        ["performance", "balanced", "power-saver"]
            .into_iter()
            .filter(|p| profiles.available.iter().any(|a| a == p))
            .map(|profile| {
                menu_row(
                    Some(Profiles::icon(profile)),
                    Profiles::name(profile).to_owned(),
                    None,
                    (active == profile).then(check),
                    Some(Message::SetProfile(profile.to_owned())),
                    active == profile,
                )
            })
            .collect()
    }

    fn graphics_card(&self) -> Vec<Element<'_, Message>> {
        let Some(graphics) = &self.graphics else {
            return vec![note("No switchable graphics")];
        };
        let mut rows: Vec<Element<'_, Message>> = graphics
            .supported
            .iter()
            .filter(|mode| **mode <= 1)
            .map(|mode| {
                let current = graphics.mode == *mode;
                menu_row(None, Graphics::mode_name(*mode).to_owned(), None, current.then(check), (!current).then_some(Message::SetGraphics(*mode)), current)
            })
            .collect();
        if graphics.pending() {
            rows.push(note(format!("Log out to switch to {}", Graphics::mode_name(graphics.pending_mode))));
        }
        let Some(gpus) = &self.gpus else {
            return rows;
        };
        rows.push(rule());
        if let Some(gpu) = &gpus.integrated {
            rows.push(Row::new(gpu.name.clone()).icon("gpu-2").sub(if gpu.drives_display { "Integrated · drives the display" } else { "Integrated" }).view());
        }
        let hybrid = graphics.mode != 1;
        if let Some(gpu) = &gpus.discrete {
            rows.push(dgpu_row(gpu, hybrid));
            if hybrid && matches!(gpu.status, Status::Active { .. }) {
                if !gpu.processes.is_empty() {
                    rows.push(heading("Using the dGPU"));
                    for process in &gpu.processes {
                        rows.push(Row::new(registry::name(&process.name)).icon(registry::app_info(&process.name).icon).sub(format!("{} VRAM", gpu::gigabytes(process.vram))).view());
                    }
                }
                if let (Some(used), Some(total)) = (gpu.vram_used, gpu.vram_total) {
                    rows.push(heading("VRAM"));
                    rows.push(Row::new(format!("{} / {}", gpu::gigabytes(used), gpu::gigabytes(total))).icon("stack-2").extra(meter(used as f32 / total.max(1) as f32)).view());
                }
            }
        }
        rows
    }

    /// What the discrete GPU draws, when it's awake and says.
    pub(crate) fn dgpu_watts(&self) -> Option<f32> {
        self.gpus.as_ref().and_then(|g| g.discrete.as_ref()).and_then(|g| match g.status {
            Status::Active { power_w, .. } => power_w,
            _ => None,
        })
    }

    /// The machine's power draw: the battery's word while discharging, else
    /// the CPU and dGPU with a little for the rest.
    pub(crate) fn draw_total(&self) -> Option<f32> {
        let system = self.system.as_ref()?;
        let battery = self.power.as_ref().and_then(|p| p.battery.as_ref());
        system
            .draw
            .total_w
            .or_else(|| battery.filter(|b| !b.charging && b.rate > 0.5).map(|b| b.rate as f32))
            .or_else(|| system.draw.cpu_w.map(|c| c + self.dgpu_watts().unwrap_or(0.0) + 3.2))
    }

    /// Each display as it runs: resolution and refresh rate, variable
    /// refresh, scale. COSMIC Settings changes them.
    fn display_card(&self) -> Vec<Element<'_, Message>> {
        let mut rows: Vec<Element<'_, Message>> = Vec::new();
        let outputs: Vec<&crate::backend::display::Output> = self.displays.iter().filter(|o| o.enabled && o.current().is_some()).collect();
        if outputs.is_empty() {
            rows.push(Row::new("Reading the displays…").icon("device-desktop").dim(true).view());
        }
        for output in outputs {
            let Some(current) = output.current().copied() else {
                continue;
            };
            let native = output.modes.iter().find(|m| m.preferred).filter(|m| m.width != current.width || m.height != current.height);
            let mut about = vec![if output.model.is_empty() { output.name.clone() } else { format!("{} · {}", output.model, output.name) }];
            if let Some(native) = native {
                about.push(format!("native {}×{}", native.width, native.height));
            }
            about.push(format!("scale {:.0} %", output.scale * 100.0));
            rows.push(Row::new(format!("{}×{} at {}", current.width, current.height, current.hertz_label())).icon("device-desktop").sub(about.join(" · ")).view());
        }
        // Night light, through cosmic-nightlight where it's installed.
        if let Some(night) = self.night {
            let sub = match night.on {
                Some(kelvin) => format!("On · {kelvin} K"),
                None => format!("Off · {} K when on", night.temperature),
            };
            rows.push(rule());
            rows.push(
                Row::new("Night light")
                    .icon("moon")
                    .sub(sub)
                    .trailing(switch(night.on.is_some(), |_| Message::ToggleNightLight))
                    .on_press(Message::ToggleNightLight)
                    .view(),
            );
            rows.push(row_item(Some("settings"), "Night light settings".to_owned(), None, Some(Message::NightLightSettings)));
        }
        rows.push(rule());
        rows.push(row_item(Some("settings"), "Display settings".to_owned(), None, Some(Message::Settings(Some("displays")))));
        rows
    }

    fn dns_card(&self) -> Vec<Element<'_, Message>> {
        let Some(dns) = self.network.as_ref().and_then(|n| n.dns.as_ref()) else {
            return vec![note("No connection")];
        };
        let current = match self.pending(|p| matches!(p, Pending::Dns(_))) {
            Some(Pending::Dns(choice)) => choice.clone(),
            _ => dns.choice.clone(),
        };
        // Under Automatic, the network's servers while they're the ones in use.
        let from_network = (dns.choice == Choice::Automatic && !dns.servers.is_empty()).then(|| format!("From the network: {}", dns.servers.join(", ")));
        let mut rows = vec![menu_row(None, "Automatic".to_owned(), from_network, (current == Choice::Automatic).then(check), Some(Message::SetDns(Choice::Automatic)), current == Choice::Automatic)];
        for (i, provider) in PROVIDERS.iter().enumerate() {
            let choice = Choice::Provider(i);
            rows.push(menu_row(None, provider.name.to_owned(), Some(provider.ipv4.join(", ")), (current == choice).then(check), Some(Message::SetDns(choice.clone())), current == choice));
        }
        if let Choice::Custom(servers) = &dns.choice {
            rows.push(row_item(None, format!("Custom: {}", servers.join(", ")), Some(check()), None));
        }
        rows
    }

    /// The camera whose picture to stream: the one on show, if it's real.
    pub(crate) fn preview_camera(&self) -> Option<&camera::Camera> {
        self.selected_camera().filter(|c| c.bus != Bus::Virtual)
    }

    /// The camera picked in the menu: the chosen one, else the first.
    fn selected_camera(&self) -> Option<&camera::Camera> {
        let cameras = self.cameras.as_ref()?;
        self.camera_sel.as_ref().and_then(|id| cameras.list.iter().find(|c| &c.id == id)).or_else(|| cameras.list.first())
    }

    fn camera_card(&self) -> Vec<Element<'_, Message>> {
        let Some(cameras) = &self.cameras else {
            return vec![Row::new("Looking…").icon("camera").dim(true).view()];
        };
        let mut rows: Vec<Element<'_, Message>> = vec![heading("Devices")];
        let selected = self.selected_camera();
        if cameras.privacy_switch && !cameras.list.iter().any(|c| c.bus == Bus::Builtin) {
            rows.push(Row::new("Built-in camera").icon("camera").sub("Privacy switch on").dim(true).view());
        }
        for camera in &cameras.list {
            let users: Vec<String> = camera.users.iter().map(|u| registry::name(&u.name)).collect();
            let state = if users.is_empty() { "Idle".to_owned() } else { format!("In use · {}", users.join(", ")) };
            let is_selected = selected.is_some_and(|s| s.id == camera.id);
            rows.push(
                Row::new(camera.name.clone())
                    .icon(camera_icon(camera.bus))
                    .sub(format!("{state} · {}", camera.node))
                    .trailing_maybe(is_selected.then(check))
                    .selected(is_selected)
                    .on_press(Message::SelectCamera(camera.id.clone()))
                    .view(),
            );
        }
        if cameras.list.is_empty() {
            rows.push(Row::new("No cameras").icon("camera-off").dim(true).view());
        }
        if let Some(camera) = selected {
            rows.push(rule());
            let live = !camera.users.is_empty();
            rows.push(Row::new(camera.name.clone()).icon(camera_icon(camera.bus)).sub(format!("{} · {}", camera.node, camera.driver)).extra(under(camera_preview(camera, live, self.preview.as_ref()))).view());
            rows.extend(self.resolution_rows(camera));
            if live {
                rows.push(heading("In use by"));
                for user in &camera.users {
                    let info = registry::app_info(&user.name);
                    rows.push(Row::new(info.name).icon(info.icon).sub(camera.name.clone()).view());
                }
            }
            if camera.bus == Bus::Virtual {
                rows.push(Row::new("Virtual camera").icon("info-circle").sub("Image controls come from the app feeding it").dim(true).view());
            } else if !camera.controls.is_empty() {
                rows.push(heading("Image"));
                for control in &camera.controls {
                    rows.push(self.control_row(camera, control));
                }
                rows.push(row_item(Some("refresh"), "Reset to defaults".to_owned(), None, Some(Message::ResetCamera(camera.node.clone()))));
            }
        }
        rows
    }

    /// The preview's size: the one in use at the end of a row that opens
    /// the camera's list of sizes.
    fn resolution_rows<'a>(&'a self, camera: &'a camera::Camera) -> Vec<Element<'a, Message>> {
        let current = self.preview_size.or(camera.resolution);
        let shown = current.map_or("Automatic".to_owned(), |(w, h)| format!("{w}×{h}"));
        let trailing = widget::row::with_children(vec![
            text::small(shown).class(theme::Text::Custom(super::dimmed)).into(),
            faint_icon(if self.sizes_open { "chevron-down" } else { "chevron-right" }),
        ])
        .spacing(10)
        .align_y(Alignment::Center);
        let mut rows = vec![Row::new("Resolution").icon("aspect-ratio").trailing(trailing).on_press(Message::ToggleSizes).view()];
        if self.sizes_open {
            let auto = self.preview_size.is_none();
            rows.push(Row::new("Automatic").indent().trailing_maybe(auto.then(check)).selected(auto).on_press(Message::SetPreviewSize(None)).view());
            for size in &camera.frame_sizes {
                let picked = self.preview_size == Some(*size);
                rows.push(Row::new(format!("{}×{}", size.0, size.1)).indent().trailing_maybe(picked.then(check)).selected(picked).on_press(Message::SetPreviewSize(Some(*size))).view());
            }
        }
        rows
    }

    /// One image control: a slider, a switch, or a choice that cycles.
    fn control_row<'a>(&'a self, camera: &'a camera::Camera, control: &'a camera::Control) -> Element<'a, Message> {
        let node = camera.node.clone();
        let id = control.id;
        let value = match &self.adjusting {
            Some(Adjusting::Camera(n, i, v)) if *n == camera.node && *i == id => *v,
            _ => control.value,
        };
        // A read-only control is just a fact: its value at the end, nothing to move.
        if control.read_only {
            let shown = match &control.kind {
                camera::Kind::Boolean { .. } => if value != 0 { "On".to_owned() } else { "Off".to_owned() },
                camera::Kind::Menu { options, .. } => options.get(value.max(0) as usize).cloned().filter(|o| !o.is_empty()).unwrap_or_else(|| value.to_string()),
                camera::Kind::Integer { .. } => value.to_string(),
            };
            return Row::new(control.name.clone()).icon(control.icon).trailing(text::small(shown).class(theme::Text::Custom(super::dimmed))).view();
        }
        match &control.kind {
            camera::Kind::Integer { min, max, .. } => {
                let (min, max) = (*min as f32, (*max).max(min + 1) as f32);
                let fraction = (value as f32 - min) / (max - min);
                let dim = control.inactive;
                let slider = widget::slider(0.0..=1.0, fraction.clamp(0.0, 1.0), move |f| Message::SetCameraControl(node.clone(), id, (min + f * (max - min)).round() as i32))
                    .step(0.001_f32)
                    .height(22.0)
                    .width(Length::Fill)
                    .on_release(Message::AdjustDone)
                    .class(super::slider_class(if dim { super::Tone::Muted } else { super::Tone::Normal }));
                // The value in a bubble over the knob while it's dragged.
                let dragging = matches!(&self.adjusting, Some(Adjusting::Camera(n, i, _)) if *n == camera.node && *i == id);
                let label: Option<Element<'a, Message>> = dragging.then(|| text::sized_bold(format!("{:.0} %", fraction * 100.0), 12.0).into());
                let slider: Element<'a, Message> = if dim {
                    widget::container(slider).class(inherit()).into()
                } else {
                    super::mark::Marked::new(slider, fraction).label(label).into()
                };
                Row::new(control.name.clone()).icon(control.icon).dim(dim).extra(under(slider)).view()
            }
            camera::Kind::Boolean { .. } => {
                let on = value != 0;
                let node2 = node.clone();
                Row::new(control.name.clone())
                    .icon(control.icon)
                    .dim(control.inactive)
                    .trailing(switch(on, move |on| Message::SetCameraControl(node2.clone(), id, i32::from(on))))
                    .on_press(Message::SetCameraControl(node, id, i32::from(!on)))
                    .view()
            }
            camera::Kind::Menu { options, .. } => {
                let current = options.get(value.max(0) as usize).cloned().unwrap_or_else(|| value.to_string());
                // The driver may skip values: the next one it offers.
                let next = (1..=options.len() as i32).map(|step| (value + step).rem_euclid(options.len().max(1) as i32)).find(|i| options.get(*i as usize).is_some_and(|o| !o.is_empty())).unwrap_or(value);
                Row::new(control.name.clone())
                    .icon(control.icon)
                    .dim(control.inactive)
                    .trailing(text::small(current).class(theme::Text::Custom(super::dimmed)))
                    .on_press(Message::SetCameraControl(node, id, next))
                    .view()
            }
        }
    }

    fn privacy_card(&self) -> Vec<Element<'_, Message>> {
        let privacy = &self.privacy;
        let mut rows: Vec<Element<'_, Message>> = Vec::new();
        if privacy.sharing_screen() {
            rows.push(heading("Screen sharing"));
            for share in &privacy.screen {
                let info = registry::app_info(&share.app);
                rows.push(Row::new(info.name).icon(info.icon).sub(share.what.clone()).trailing(action_button("player-stop", "Stop", Message::StopShare(share.node))).view());
            }
        }
        let camera_users: Vec<(String, String)> = {
            let mut users: Vec<(String, String)> = privacy
                .camera
                .iter()
                .map(|u| {
                    let camera = u.node.as_ref().and_then(|n| self.cameras.as_ref()?.list.iter().find(|c| &c.node == n)).map(|c| c.name.clone()).unwrap_or_else(|| "Camera".to_owned());
                    (u.app.clone(), camera)
                })
                .collect();
            if let Some(cameras) = &self.cameras {
                for camera in &cameras.list {
                    for user in &camera.users {
                        if !users.iter().any(|(app, _)| registry::name(app) == registry::name(&user.name)) {
                            users.push((user.name.clone(), camera.name.clone()));
                        }
                    }
                }
            }
            users
        };
        if !camera_users.is_empty() {
            rows.push(heading("Camera"));
            for (app, camera) in camera_users {
                let info = registry::app_info(&app);
                rows.push(Row::new(info.name).icon(info.icon).sub(camera).view());
            }
        }
        if !privacy.microphone.is_empty() {
            rows.push(heading("Microphone"));
            for app in &privacy.microphone {
                let info = registry::app_info(app);
                rows.push(Row::new(info.name).icon(info.icon).view());
            }
            rows.push(self.mute_microphone_row());
        }
        if rows.is_empty() {
            rows.push(Row::new("Nothing is using your camera, microphone or screen").icon("shield-lock").dim(true).view());
        }
        rows
    }

    /// A row with a switch that mutes the microphone.
    pub(crate) fn mute_microphone_row(&self) -> Element<'_, Message> {
        let muted = self.audio.as_ref().and_then(|a| a.input).is_some_and(|i| i.muted);
        Row::new("Mute microphone")
            .icon(if muted { "microphone-off" } else { "microphone" })
            .trailing(switch(muted, Message::SetInputMute))
            .on_press(Message::SetInputMute(!muted))
            .view()
    }
}

/// The integrated GPU: its load, its own memory and the system memory it
/// has mapped (where an APU keeps most of its data).
pub(super) fn igpu_row<'a>(gpu: &gpu::Gpu) -> Element<'a, Message> {
    let util = match gpu.status {
        Status::Active { util_percent, .. } => util_percent,
        _ => None,
    };
    let title = match util {
        Some(util) => format!("{} {util} %", gpu.name),
        None => gpu.name.clone(),
    };
    let mut parts = Vec::new();
    if let (Some(used), Some(total)) = (gpu.vram_used, gpu.vram_total) {
        parts.push(format!("VRAM {} / {}", gpu::gigabytes(used), gpu::gigabytes(total)));
    }
    if let (Some(used), Some(total)) = (gpu.shared_used, gpu.shared_total) {
        parts.push(format!("shared {} / {}", gpu::gigabytes(used), gpu::gigabytes(total)));
    }
    if let Status::Active { temp_c: Some(temp), .. } = gpu.status {
        parts.push(format!("{temp:.0} °C"));
    }
    let row = Row::new(title).icon("gpu-2").sub(if parts.is_empty() { "Integrated".to_owned() } else { parts.join(" · ") });
    match util {
        Some(util) => row.extra(meter(util as f32 / 100.0)).view(),
        None => row.view(),
    }
}

/// The dedicated GPU: asleep or off, or awake with its load, temperature,
/// power draw and memory.
pub(super) fn dgpu_row<'a>(gpu: &gpu::Gpu, hybrid: bool) -> Element<'a, Message> {
    let row = Row::new(gpu.name.clone()).icon("gpu");
    match (&gpu.status, hybrid) {
        (_, false) => row.sub("Powered off").dim(true).view(),
        (Status::Active { util_percent, temp_c, power_w }, true) => {
            let mut parts = vec!["Active".to_owned()];
            if let Some(util) = util_percent {
                parts.push(format!("{util} %"));
            }
            if let Some(temp) = temp_c {
                parts.push(format!("{temp:.0} °C"));
            }
            if let Some(power) = power_w {
                parts.push(format!("{power:.1} W"));
            }
            if let (Some(used), Some(total)) = (gpu.vram_used, gpu.vram_total) {
                parts.push(format!("VRAM {} / {}", gpu::gigabytes(used), gpu::gigabytes(total)));
            }
            let row = row.sub(parts.join(" · "));
            match util_percent {
                Some(util) => row.extra(meter(*util as f32 / 100.0)).view(),
                None => row.view(),
            }
        }
        (Status::Asleep, true) => row.sub("Asleep · 0 W").dim(true).view(),
        (Status::Off, true) => row.sub("Powered off").dim(true).view(),
    }
}

fn camera_icon(bus: Bus) -> &'static str {
    match bus {
        Bus::Builtin => "camera",
        Bus::Usb => "usb",
        Bus::Virtual => "video",
    }
}

/// The camera's picture, a few a second; a dark box with a figure until
/// the first arrives. "Live" when an app has the camera, and the picture's size.
fn camera_preview<'a>(camera: &camera::Camera, live: bool, frame: Option<&widget::image::Handle>) -> Element<'a, Message> {
    // Words over the picture sit on a dark plate: the picture may be bright.
    let plate = |content: Element<'a, Message>, live: bool| -> Element<'a, Message> {
        widget::container(content)
            .padding([3, 8])
            .class(theme::Container::custom(move |_| widget::container::Style {
                background: Some(Background::Color(if live { palette::AMBER } else { palette::rgba(8, 12, 26, 0.72) })),
                text_color: Some(if live { palette::ON_AMBER } else { palette::TEXT }),
                border: cosmic::iced::Border { radius: 99.0.into(), ..Default::default() },
                ..Default::default()
            }))
            .into()
    };
    let badge = plate(text::sized_bold(if live { "Live" } else { "Preview" }, 11.0).into(), live);
    let figure = widget::container(icon(if camera.bus == Bus::Virtual { "video" } else { "user" }, 64))
        .center(Length::Fill)
        .class(theme::Container::custom(|_| widget::container::Style { icon_color: Some(palette::rgba(232, 236, 246, 0.25)), ..Default::default() }));
    // No picture while another app streams the camera: say so.
    let caption: Option<Element<'a, Message>> = (frame.is_none() && live).then(|| {
        widget::container(plate(text::sized("No preview while another app has the camera", 11.0).into(), false))
            .align_left(Length::Fill)
            .align_bottom(Length::Fill)
            .padding([8, 10])
            .into()
    });
    let picture: Element<'a, Message> = match frame {
        Some(handle) => widget::image(handle.clone())
            .content_fit(cosmic::iced::ContentFit::Cover)
            .width(Length::Fill)
            .height(Length::Fixed(150.0))
            .border_radius([12.0; 4])
            .into(),
        None => figure.into(),
    };
    let mut layers: Vec<Element<'a, Message>> = vec![picture, widget::container(badge).padding(10).into()];
    if let Some(caption) = caption {
        layers.push(caption);
    }
    if let Some((w, h)) = camera.resolution {
        layers.push(
            widget::container(plate(text::sized(format!("{w}×{h}"), 11.0).into(), false))
                .align_right(Length::Fill)
                .align_bottom(Length::Fill)
                .padding([8, 10])
                .into(),
        );
    }
    widget::container(cosmic::iced::widget::stack(layers).width(Length::Fill).height(Length::Fixed(150.0)))
        .width(Length::Fill)
        .height(Length::Fixed(150.0))
        .class(theme::Container::custom(move |_| widget::container::Style {
            background: Some(Background::Color(palette::rgb_(0x12, 0x18, 0x30))),
            border: cosmic::iced::Border { radius: 12.0.into(), width: if live { 2.0 } else { 0.0 }, color: palette::AMBER },
            ..Default::default()
        }))
        .into()
}

/// A backlight level by name: off, low, medium, high; or its number.
fn level_name(level: i32, max: i32) -> String {
    match (level, max) {
        (0, _) => "Off".to_owned(),
        (1, 3) => "Low".to_owned(),
        (2, 3) => "Medium".to_owned(),
        (3, 3) => "High".to_owned(),
        (l, 1) if l >= 1 => "On".to_owned(),
        (l, m) => format!("{l} of {m}"),
    }
}

/// "1 camera", "3 cameras".
pub(super) fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

/// 2400 as "2,400".
pub(super) fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Two tiles side by side.
fn pair<'a>(left: Element<'a, Message>, right: Element<'a, Message>) -> Element<'a, Message> {
    widget::row::with_children(vec![
        widget::container(left).width(Length::FillPortion(1)).into(),
        widget::container(right).width(Length::FillPortion(1)).into(),
    ])
    .spacing(INNER_GAP as u16)
    .into()
}
