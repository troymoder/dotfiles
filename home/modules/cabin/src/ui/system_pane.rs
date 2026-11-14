// SPDX-License-Identifier: GPL-3.0-only

//! The System panel, under its own panel button: CPU, memory, disk,
//! network, the GPUs, power and thermals, the busiest programs, and NixOS.
//! Each section folds to a line that keeps its live summary; the header
//! folds or unfolds them all. The panel's width never changes.

use cosmic::{
    Element,
    iced::{Alignment, Background, Color, Length},
    theme,
    widget::{self, button},
};

use super::{
    PANE_WIDTH, Row, custom_button, faint_icon, icon,
    main_pane::{dgpu_row, igpu_row, plural, thousands},
    meter, palette, registry,
    sparkline::Sparkline,
    text, under,
};
use crate::{
    app::{App, Message},
    backend::{
        gpu::Status,
        system::{self, Kind},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Cpu,
    Memory,
    Disk,
    Network,
    Gpus,
    Power,
    Processes,
    NixOs,
}

impl Section {
    pub const ALL: [Section; 8] = [
        Self::Cpu,
        Self::Memory,
        Self::Disk,
        Self::Network,
        Self::Gpus,
        Self::Power,
        Self::Processes,
        Self::NixOs,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Memory => "Memory",
            Self::Disk => "Disk",
            Self::Network => "Network",
            Self::Gpus => "GPUs",
            Self::Power => "Power & thermals",
            Self::Processes => "Top processes",
            Self::NixOs => "NixOS",
        }
    }
}

/// Pale amber: a summary that wants attention.
const WARN: Color = palette::rgb_(0xf6, 0xcf, 0x94);

impl App {
    pub(crate) fn system_pane(&self) -> Element<'_, Message> {
        let Some(system) = &self.system else {
            let reading = Row::new("Reading…").icon("activity").dim(true).view();
            return super::pane(widget::column::with_children(vec![
                self.system_header(None),
                reading,
            ]));
        };
        let mut rows: Vec<Element<'_, Message>> =
            vec![self.system_header(Some(format!("{:.0} %", system.cpu.percent)))];
        for section in Section::ALL {
            let Some((summary, warn, body)) = self.section(section, system) else {
                continue;
            };
            rows.push(self.fold(section, summary, warn, body));
        }
        super::pane(widget::column::with_children(rows).spacing(2))
    }

    /// The header: the chip, the name, the CPU, and the fold-all button.
    fn system_header(&self, right: Option<String>) -> Element<'_, Message> {
        let all_closed = Section::ALL
            .iter()
            .all(|s| self.sections_closed.contains(s));
        let chip = widget::container(icon("activity", 19))
            .center(Length::Fixed(40.0))
            .class(theme::Container::custom(|_| widget::container::Style {
                background: Some(Background::Color(palette::ACCENT)),
                icon_color: Some(palette::ON_ACCENT),
                text_color: Some(palette::ON_ACCENT),
                border: cosmic::iced::Border {
                    radius: 12.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }));
        let fold_all = button::custom(
            widget::container(icon(
                if all_closed {
                    "arrows-diagonal"
                } else {
                    "arrows-diagonal-minimize-2"
                },
                15,
            ))
            .center(Length::Fill)
            .class(super::inherit()),
        )
        .width(Length::Fixed(30.0))
        .height(Length::Fixed(30.0))
        .padding(0)
        .class(custom_button(|hovered, _| button::Style {
            background: Some(Background::Color(if hovered {
                palette::SURFACE_HOVER
            } else {
                Color::TRANSPARENT
            })),
            text_color: Some(palette::TEXT),
            icon_color: Some(palette::TEXT),
            border_radius: 9.0.into(),
            ..button::Style::new()
        }))
        .on_press(Message::ToggleAllSections);
        let mut row = widget::row::with_capacity(5)
            .spacing(14)
            .align_y(Alignment::Center)
            .push(chip)
            .push(text::heading("System"))
            .push(widget::space::horizontal());
        if let Some(right) = right {
            row = row.push(text::sized_bold(right, 20.0));
        }
        row = row.push(fold_all);
        widget::container(row)
            .width(Length::Fill)
            .padding([4, 4, 10, 4])
            .into()
    }

    /// A section's header, and under it while unfolded, its rows in a pool.
    fn fold<'a>(
        &'a self,
        section: Section,
        summary: String,
        warn: bool,
        body: Vec<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let open = !self.sections_closed.contains(&section);
        let summary_color = if warn { WARN } else { palette::MUTED };
        let head = widget::row::with_children(vec![
            text::sized_bold(section.title(), 13.0).into(),
            widget::space::horizontal().into(),
            text::sized(summary, 12.0)
                .class(theme::Text::Color(summary_color))
                .into(),
            faint_icon(super::chevron(open)),
        ])
        .spacing(10)
        .align_y(Alignment::Center);
        let head = button::custom(head)
            .width(Length::Fill)
            .padding([10, 14])
            .class(custom_button(|hovered, _| button::Style {
                background: Some(Background::Color(if hovered {
                    palette::ROW_HOVER
                } else {
                    Color::TRANSPARENT
                })),
                text_color: Some(palette::TEXT),
                icon_color: Some(palette::TEXT),
                border_radius: 12.0.into(),
                ..button::Style::new()
            }))
            .on_press(Message::ToggleSection(section));
        if !open {
            return head.into();
        }
        let mut column = vec![head.into()];
        column.extend(body);
        widget::container(widget::column::with_children(column))
            .width(Length::Fill)
            .padding([0, 0, 4, 0])
            .class(theme::Container::custom(|_| widget::container::Style {
                background: Some(Background::Color(palette::rgba(12, 18, 40, 0.22))),
                text_color: Some(palette::TEXT),
                icon_color: Some(palette::TEXT),
                border: cosmic::iced::Border {
                    radius: 14.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }))
            .into()
    }

    /// A section's summary, whether it wants attention, and its rows; none
    /// for what this machine doesn't have.
    fn section<'a>(
        &'a self,
        section: Section,
        system: &'a system::System,
    ) -> Option<(String, bool, Vec<Element<'a, Message>>)> {
        Some(match section {
            Section::Cpu => {
                let cpu = &system.cpu;
                let mut summary = format!("{:.0} %", cpu.percent);
                let mut sub = vec![plural(cpu.threads, "thread")];
                if let Some(temp) = cpu.temp_c {
                    summary.push_str(&format!(" · {temp:.0} °C"));
                    sub.push(format!("{temp:.0} °C"));
                }
                let floor = cpu.history.iter().copied().fold(40.0_f32, f32::max) * 1.15;
                let row = Row::new(cpu.model.clone())
                    .icon("cpu")
                    .sub(sub.join(" · "))
                    .extra(under(Sparkline::new(&cpu.history, floor).view()))
                    .view();
                (summary, false, vec![row])
            }
            Section::Memory => {
                let memory = &system.memory;
                let row = Row::new(format!(
                    "{} used of {}",
                    system::bytes(memory.used),
                    system::bytes(memory.total)
                ))
                .icon("stack-2")
                .sub(format!(
                    "Swap {} / {}",
                    system::bytes(memory.swap_used),
                    system::bytes(memory.swap_total)
                ))
                .extra(meter(memory.used as f32 / memory.total.max(1) as f32))
                .view();
                (
                    format!(
                        "{} / {}",
                        system::bytes(memory.used),
                        system::bytes(memory.total)
                    ),
                    false,
                    vec![row],
                )
            }
            Section::Disk => {
                let disk = &system.disk;
                let row = Row::new(format!(
                    "{} used of {}",
                    system::bytes(disk.used),
                    system::bytes(disk.total)
                ))
                .icon("database")
                .sub(format!("/ · {} free", system::bytes(disk.free)))
                .extra(meter(disk.used as f32 / disk.total.max(1) as f32))
                .view();
                (
                    format!("{} free", system::bytes(disk.free)),
                    disk.free < disk.total / 20,
                    vec![row],
                )
            }
            Section::Network => {
                let net = &system.network;
                (
                    format!(
                        "↓ {}  ↑ {}",
                        system::rate(net.down_rate),
                        system::rate(net.up_rate)
                    ),
                    false,
                    self.network_rows(net),
                )
            }
            Section::Gpus => {
                let gpus = self.gpus.as_ref()?;
                let hybrid = self.graphics.as_ref().is_none_or(|g| g.mode != 1);
                let mut parts = Vec::new();
                let mut rows = Vec::new();
                if let Some(gpu) = &gpus.integrated {
                    parts.push(match gpu.status {
                        Status::Active {
                            util_percent: Some(util),
                            ..
                        } => format!("iGPU {util} %"),
                        _ => "iGPU".to_owned(),
                    });
                    rows.push(igpu_row(gpu));
                }
                if let Some(gpu) = &gpus.discrete {
                    parts.push(match (&gpu.status, hybrid) {
                        (_, false) | (Status::Off, _) => "dGPU off".to_owned(),
                        (Status::Asleep, _) => "dGPU asleep".to_owned(),
                        (
                            Status::Active {
                                util_percent: Some(util),
                                ..
                            },
                            _,
                        ) => format!("dGPU {util} %"),
                        (Status::Active { .. }, _) => "dGPU active".to_owned(),
                    });
                    rows.push(dgpu_row(gpu, hybrid));
                }
                if rows.is_empty() {
                    return None;
                }
                (parts.join(" · "), false, rows)
            }
            Section::Power => {
                let mut rows = Vec::new();
                let mut summary = Vec::new();
                if let Some(total) = self.draw_total() {
                    summary.push(format!("{total:.1} W"));
                    let mut parts = Vec::new();
                    if let Some(cpu) = system.draw.cpu_w {
                        parts.push(format!("CPU {cpu:.1} W"));
                    }
                    if let Some(gpu) = self.dgpu_watts() {
                        parts.push(format!("dGPU {gpu:.1} W"));
                    }
                    let mut row = Row::new(format!("System draw {total:.1} W"))
                        .icon("bolt")
                        .sub_maybe((!parts.is_empty()).then(|| parts.join(" · ")));
                    if self.draw_history.len() >= 2 {
                        let floor =
                            self.draw_history.iter().copied().fold(10.0_f32, f32::max) * 1.15;
                        row = row.extra(under(Sparkline::new(&self.draw_history, floor).view()));
                    }
                    rows.push(row.view());
                }
                if let Some(rpm) = system.fan_rpm {
                    summary.push(if rpm == 0 {
                        "fan off".to_owned()
                    } else {
                        format!("{} RPM", thousands(rpm))
                    });
                    let mut temps = Vec::new();
                    if let Some(temp) = system.cpu.temp_c {
                        temps.push(format!("CPU {temp:.0} °C"));
                    }
                    if let Some(temp) = system.gpu_temp_c {
                        temps.push(format!("GPU {temp:.0} °C"));
                    }
                    let title = if rpm == 0 {
                        "Fan stopped".to_owned()
                    } else {
                        format!("Fan {} RPM", thousands(rpm))
                    };
                    rows.push(
                        Row::new(title)
                            .icon("propeller")
                            .sub_maybe((!temps.is_empty()).then(|| temps.join(" · ")))
                            .view(),
                    );
                }
                if rows.is_empty() {
                    return None;
                }
                (summary.join(" · "), false, rows)
            }
            Section::Processes => {
                if system.processes.is_empty() {
                    return Some((
                        "Reading…".to_owned(),
                        false,
                        vec![Row::new("Reading…").icon("activity").dim(true).view()],
                    ));
                }
                let rows = system
                    .processes
                    .iter()
                    .map(|process| {
                        let info = registry::app_info(&process.name);
                        Row::new(info.name)
                            .icon(info.icon)
                            .sub(format!(
                                "{:.0} % CPU · {}",
                                process.cpu,
                                system::bytes(process.memory)
                            ))
                            .view()
                    })
                    .collect();
                let top = &system.processes[0];
                (
                    format!("{} {:.0} %", registry::name(&top.name), top.cpu),
                    false,
                    rows,
                )
            }
            Section::NixOs => {
                let nixos = system.nixos.as_ref()?;
                let mut rows = Vec::new();
                if nixos.reboot_pending {
                    let sub = match &nixos.kernels {
                        Some((booted, current)) => format!("Kernel {booted} → {current}"),
                        None => "The system switched to a new kernel or initrd".to_owned(),
                    };
                    rows.push(
                        Row::new("Reboot to finish update")
                            .icon("alert-triangle")
                            .sub(sub)
                            .warn(true)
                            .view(),
                    );
                }
                let mut parts = Vec::new();
                if let Some(days) = nixos.built_days {
                    parts.push(format!("Built {}", crate::backend::nixos::days(days)));
                }
                let stale = nixos
                    .nixpkgs_days
                    .is_some_and(|days| days > crate::backend::nixos::STALE_DAYS);
                if let Some(days) = nixos.nixpkgs_days {
                    parts.push(if days <= 0 {
                        "nixpkgs lock from today".to_owned()
                    } else {
                        format!(
                            "nixpkgs lock {days} day{} old",
                            if days == 1 { "" } else { "s" }
                        )
                    });
                }
                rows.push(
                    Row::new(format!("Generation {}", nixos.generation))
                        .icon("stack-2")
                        .sub(parts.join(" · "))
                        .warn(stale)
                        .view(),
                );
                let summary = if nixos.reboot_pending {
                    "Reboot needed".to_owned()
                } else if stale {
                    "Time to update".to_owned()
                } else {
                    format!("Generation {}", nixos.generation)
                };
                (summary, nixos.reboot_pending || stale, rows)
            }
        })
    }

    /// The connection in use, today's totals and the live rates graphed;
    /// its chevron lists every interface.
    fn network_rows<'a>(&'a self, net: &'a system::Network) -> Vec<Element<'a, Message>> {
        let connection = self
            .network
            .as_ref()
            .and_then(|n| n.wifi.as_ref().or(n.wired.as_ref()))
            .map(|c| c.name.clone());
        let mut title: Vec<String> = net.active.iter().cloned().collect();
        match &connection {
            Some(connection) => title.push(connection.clone()),
            None if net.active.is_none() => title.push("Offline".to_owned()),
            None => {}
        }
        let rates = widget::row::with_children(vec![
            text::small(format!("↓ {}", system::rate(net.down_rate)))
                .class(theme::Text::Color(palette::ACCENT))
                .into(),
            text::small(format!("↑ {}", system::rate(net.up_rate)))
                .class(theme::Text::Color(palette::EVENT))
                .into(),
        ])
        .spacing(14);
        let down: Vec<f32> = net.down_history.iter().map(|v| *v as f32).collect();
        let up: Vec<f32> = net.up_history.iter().map(|v| *v as f32).collect();
        let graph = widget::column::with_children(vec![
            under(rates),
            under(Sparkline::pair(&down, &up).view()),
        ]);
        let mut rows = vec![
            Row::new(title.join(" · "))
                .icon("arrows-down-up")
                .sub(format!(
                    "{} down · {} up today",
                    system::bytes(net.today_rx),
                    system::bytes(net.today_tx)
                ))
                .extra(graph)
                .trailing(faint_icon(if self.net_all {
                    "chevron-down"
                } else {
                    "chevron-right"
                }))
                .on_press(Message::ToggleNetAll)
                .view(),
        ];
        if !self.net_all {
            return rows;
        }
        for interface in &net.interfaces {
            let (icon_name, kind) = match interface.kind {
                Kind::Wifi => ("wifi", "Wi-Fi"),
                Kind::Ethernet => ("ethernet", "Ethernet"),
                Kind::Vpn => (
                    "shield-check",
                    if interface.name.starts_with("tailscale") {
                        "Tailscale"
                    } else {
                        "VPN"
                    },
                ),
                Kind::Bridge => ("stack-2", "Bridge"),
                Kind::Loopback | Kind::Other => ("network", "Interface"),
            };
            let mut parts = vec![kind.to_owned()];
            if !interface.up {
                parts.push(if interface.kind == Kind::Ethernet {
                    "no cable".to_owned()
                } else {
                    "down".to_owned()
                });
            } else {
                if interface.kind == Kind::Wifi {
                    if let Some(connection) = &connection {
                        parts.push(connection.clone());
                    }
                }
                if let Some(speed) = interface.speed_mbps {
                    parts.push(if speed >= 1000 {
                        format!("{} Gb/s", speed / 1000)
                    } else {
                        format!("{speed} Mb/s")
                    });
                }
                if let Some(address) = &interface.address {
                    parts.push(address.clone());
                }
                if interface.down_rate + interface.up_rate < 1.0 {
                    parts.push("idle".to_owned());
                } else {
                    parts.push(format!(
                        "↓ {}  ↑ {}",
                        system::rate(interface.down_rate),
                        system::rate(interface.up_rate)
                    ));
                }
            }
            rows.push(
                Row::new(interface.name.clone())
                    .icon(icon_name)
                    .sub(parts.join(" · "))
                    .dim(!interface.up)
                    .indent()
                    .view(),
            );
        }
        rows
    }
}

/// The panel's width, whatever is folded.
pub const WIDTH: f32 = PANE_WIDTH;
