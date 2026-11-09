// SPDX-License-Identifier: GPL-3.0-only

//! The date menu's state: a wide panel with the notifications at the left
//! and, at the right, the date, a calendar, the day's events, the weather,
//! and the world's clocks. The daemon owns it; the clock button on the
//! panel keeps the time and forwards the notifications it's handed.

use std::time::Duration;

use chrono::{DateTime, Datelike, Duration as Days, Local, NaiveDate, Weekday};
use cosmic::{
    Element, Task, app,
    iced::{self, Alignment, Background, Length, Subscription},
    theme, widget,
};

use crate::{
    backend::{
        self,
        ics::Calendar,
        notifications::{self, Notification},
        weather::{self, Weather},
    },
    bus,
    ui::{self, GAP, PANE_PADDING, icon, palette, text},
};

pub const WIDTH: f32 = 760.0;
const RIGHT_COLUMN: f32 = 316.0;

/// The other places whose time shows.
const CLOCKS: &[(&str, chrono_tz::Tz)] = &[
    ("London", chrono_tz::Europe::London),
    ("New York", chrono_tz::America::New_York),
    ("Toronto", chrono_tz::America::Toronto),
    ("California", chrono_tz::America::Los_Angeles),
    ("Taiwan", chrono_tz::Asia::Taipei),
];

#[derive(Clone, Debug)]
pub enum Message {
    Tick,
    Done,
    Events(Calendar),
    Weather(Weather),
    Notification(notifications::Event),
    Dismiss(u32),
    Clear,
    Action(u32, String),
    SetDnd(bool),
    PreviousMonth,
    NextMonth,
    Select(NaiveDate),
    Today,
    ToggleUnit,
    /// A pinned notice's button.
    Notice(backend::nixos::NoticeAction),
    /// A frame of dismissed cards going.
    Frame,
}

pub struct Clock {
    /// On screen: the time ticks each second, not each 20.
    pub shown: bool,
    /// An action taken hands off to its app: the daemon closes the panel.
    pub wants_close: bool,
    now: DateTime<Local>,
    /// The month on view: its first day.
    view: NaiveDate,
    selected: NaiveDate,
    events: Calendar,
    weather: Option<Weather>,
    fahrenheit: bool,
    notifications: Vec<Notification>,
    /// What the system pins above them: a reboot, an update. The daemon
    /// keeps them in step with the system's state.
    pub notices: Vec<backend::nixos::Notice>,
    /// Dismissed cards still going (fading, sliding out), and since when.
    leaving: Vec<(u32, std::time::Instant)>,
    do_not_disturb: bool,
}

const ACTIONS: ui::notifications::Actions<Message> = ui::notifications::Actions {
    dismiss: Message::Dismiss,
    clear: || Message::Clear,
    action: Message::Action,
    toggle_dnd: Message::SetDnd,
    notice: Message::Notice,
};

fn first_of_month(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap_or(date)
}

fn months_on(first: NaiveDate, by: i32) -> NaiveDate {
    let total = first.year() * 12 + first.month0() as i32 + by;
    NaiveDate::from_ymd_opt(total.div_euclid(12), (total.rem_euclid(12) + 1) as u32, 1).unwrap_or(first)
}

impl Clock {
    /// Opened: back to today.
    pub fn opened(&mut self) {
        self.now = Local::now();
        self.selected = self.today();
        self.view = first_of_month(self.today());
    }

    /// What the clock button draws next to the time.
    pub fn bar(&self, open: bool) -> bus::ClockBar {
        bus::ClockBar { open, dnd: self.do_not_disturb, unread: !self.notifications.is_empty() || !self.notices.is_empty() }
    }

    fn today(&self) -> NaiveDate {
        self.now.date_naive()
    }

    fn temperature(&self, celsius: f64) -> String {
        if self.fahrenheit { format!("{:.0}°", celsius * 9.0 / 5.0 + 32.0) } else { format!("{celsius:.0}°") }
    }

    // ---- the panel

    /// The panel's insides, under its grip: the daemon frames and places it.
    pub fn view(&self) -> Element<'_, Message> {
        // The notifications take the calendar column's height, with the line
        // between them along their right edge.
        let gone_after = ui::notifications::LEAVE_MS as f32;
        let leaving: Vec<(u32, f32)> = self.leaving.iter().map(|(id, since)| (*id, (since.elapsed().as_millis() as f32 / gone_after).min(1.0))).collect();
        let left = widget::container(ui::notifications::view(&self.notifications, &self.notices, self.do_not_disturb, &ACTIONS, &leaving))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(PANE_PADDING)
            .class(theme::Container::custom(|_| widget::container::Style {
                border: iced::Border { width: 0.0, ..Default::default() },
                ..Default::default()
            }));
        let divider = widget::container(widget::space::vertical())
            .width(Length::Fixed(1.0))
            .height(Length::Fill)
            .class(theme::Container::custom(|_| widget::container::Style { background: Some(Background::Color(palette::SEP)), ..Default::default() }));
        let left = widget::row::with_children(vec![left.into(), divider.into()]).height(Length::Fill);
        let right = widget::container(
            widget::column::with_children(vec![self.date_header(), self.calendar(), self.events_box(), self.weather_box(), self.clocks_box()]).spacing(GAP as u16),
        )
        .width(Length::Fixed(RIGHT_COLUMN))
        .padding(PANE_PADDING);
        widget::container(ui::pair::Pair::new(left, right, RIGHT_COLUMN).min_height(452.0)).width(Length::Fixed(WIDTH)).into()
    }

    fn date_header(&self) -> Element<'_, Message> {
        let selected = self.selected;
        let header = widget::column::with_children(vec![
            text::body(selected.format("%A").to_string()).class(theme::Text::Custom(|_| iced::widget::text::Style { color: Some(palette::rgba(232, 236, 246, 0.72)), ..Default::default() })).into(),
            text::sized_bold(selected.format("%-d %B %Y").to_string(), 20.0).into(),
        ]);
        widget::mouse_area(widget::container(header).padding([2, 4, 4, 4]).width(Length::Fill))
            .on_press(Message::Today)
            .interaction(iced::mouse::Interaction::Pointer)
            .into()
    }

    fn box_<'a>(content: impl Into<Element<'a, Message>>, padding: impl Into<iced::Padding>) -> Element<'a, Message> {
        widget::container(content)
            .width(Length::Fill)
            .padding(padding)
            .class(theme::Container::custom(|_| widget::container::Style {
                background: Some(Background::Color(palette::CARD)),
                text_color: Some(palette::TEXT),
                icon_color: Some(palette::TEXT),
                border: iced::Border { radius: 16.0.into(), ..Default::default() },
                ..Default::default()
            }))
            .into()
    }

    fn nav_button(name: &str, message: Message) -> Element<'_, Message> {
        widget::button::custom(widget::container(icon(name, 16)).center(Length::Fill).class(ui::inherit()))
            .width(Length::Fixed(30.0))
            .height(Length::Fixed(30.0))
            .padding(0)
            .class(theme::Button::Custom {
                active: Box::new(|_, _| round_style(false)),
                disabled: Box::new(|_| round_style(false)),
                hovered: Box::new(|_, _| round_style(true)),
                pressed: Box::new(|_, _| round_style(true)),
            })
            .on_press(message)
            .into()
    }

    fn calendar(&self) -> Element<'_, Message> {
        let today = self.today();
        let month = if self.view.year() == today.year() { self.view.format("%B").to_string() } else { self.view.format("%B %Y").to_string() };
        let header = widget::row::with_children(vec![
            Self::nav_button("chevron-left", Message::PreviousMonth),
            widget::container(text::title(month)).center_x(Length::Fill).into(),
            Self::nav_button("chevron-right", Message::NextMonth),
        ])
        .align_y(Alignment::Center);
        let mut rows: Vec<Element<'_, Message>> = vec![header.into()];
        let names = widget::row::with_children(
            ["M", "T", "W", "T", "F", "S", "S"]
                .into_iter()
                .map(|d| {
                    widget::container(text::sub(d).class(theme::Text::Custom(|_| iced::widget::text::Style { color: Some(palette::FAINT), ..Default::default() })))
                        .width(Length::FillPortion(1))
                        .center_x(Length::Fill)
                        .padding([4, 0])
                        .into()
                })
                .collect::<Vec<Element<'_, Message>>>(),
        );
        rows.push(names.into());
        // Six weeks from the Monday on or before the first.
        let offset = self.view.weekday().num_days_from_monday() as i64;
        let start = self.view - Days::days(offset);
        for week in 0..6 {
            let mut cells: Vec<Element<'_, Message>> = Vec::with_capacity(7);
            for day in 0..7 {
                let date = start + Days::days(week * 7 + day);
                cells.push(widget::container(self.day_button(date)).width(Length::FillPortion(1)).center_x(Length::Fill).into());
            }
            rows.push(widget::row::with_children(cells).spacing(2).into());
        }
        Self::box_(widget::column::with_children(rows).spacing(2), 10)
    }

    fn day_button(&self, date: NaiveDate) -> Element<'_, Message> {
        let today = self.today();
        let is_today = date == today;
        let selected = date == self.selected && !is_today;
        let out = date.month() != self.view.month();
        let weekend = matches!(date.weekday(), Weekday::Sat | Weekday::Sun);
        let has_event = self.events.contains_key(&date);
        let color = if is_today {
            palette::ON_ACCENT
        } else if out {
            palette::rgba(232, 236, 246, 0.35)
        } else if weekend {
            palette::rgba(232, 236, 246, 0.7)
        } else {
            palette::TEXT
        };
        let label = if is_today { text::sized_bold(date.day().to_string(), 12.5) } else { text::sized(date.day().to_string(), 12.5) };
        let label = label.class(theme::Text::Custom(match (is_today, out, weekend) {
            (true, _, _) => |_| iced::widget::text::Style { color: Some(palette::ON_ACCENT), ..Default::default() },
            (_, true, _) => |_| iced::widget::text::Style { color: Some(palette::rgba(232, 236, 246, 0.35)), ..Default::default() },
            (_, _, true) => |_| iced::widget::text::Style { color: Some(palette::rgba(232, 236, 246, 0.7)), ..Default::default() },
            _ => |_| iced::widget::text::Style { color: Some(palette::TEXT), ..Default::default() },
        }));
        let _ = color;
        let style = move |hovered: bool| widget::button::Style {
            background: Some(Background::Color(if is_today {
                if hovered { palette::ACCENT_HOVER } else { palette::ACCENT }
            } else if hovered {
                palette::ROW_SELECTED
            } else {
                iced::Color::TRANSPARENT
            })),
            border_radius: 17.0.into(),
            border_width: if selected { 1.5 } else { 0.0 },
            border_color: palette::ACCENT,
            ..widget::button::Style::new()
        };
        let button = widget::button::custom(widget::container(label).center(Length::Fill).class(ui::inherit()))
            .width(Length::Fixed(34.0))
            .height(Length::Fixed(34.0))
            .padding(0)
            .class(theme::Button::Custom {
                active: Box::new(move |_, _| style(false)),
                disabled: Box::new(move |_| style(false)),
                hovered: Box::new(move |_, _| style(true)),
                pressed: Box::new(move |_, _| style(true)),
            })
            .on_press(Message::Select(date));
        if !has_event {
            return button.into();
        }
        // A dot under the number for a day with something on.
        let dot_color = if is_today { palette::RED } else { palette::EVENT };
        let dot = widget::container(widget::container(widget::space::horizontal()).width(Length::Fixed(4.0)).height(Length::Fixed(4.0)).class(theme::Container::custom(move |_| {
            widget::container::Style { background: Some(Background::Color(dot_color)), border: iced::Border { radius: 2.0.into(), ..Default::default() }, ..Default::default() }
        })))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::End)
        .padding([0, 0, 4, 0]);
        iced::widget::stack([button.into(), dot.into()]).width(Length::Fixed(34.0)).height(Length::Fixed(34.0)).into()
    }

    fn events_box(&self) -> Element<'_, Message> {
        let label = if self.selected == self.today() { "Today".to_owned() } else { self.selected.format("%A %-d %B").to_string() };
        let mut rows: Vec<Element<'_, Message>> = vec![
            text::small(label).class(theme::Text::Custom(|_| iced::widget::text::Style { color: Some(palette::MUTED), ..Default::default() })).into(),
        ];
        match self.events.get(&self.selected) {
            Some(events) if !events.is_empty() => {
                for event in events {
                    let bar = widget::container(widget::space::vertical())
                        .width(Length::Fixed(3.0))
                        .height(Length::Fixed(16.0))
                        .class(theme::Container::custom(|_| widget::container::Style {
                            background: Some(Background::Color(palette::EVENT)),
                            border: iced::Border { radius: 2.0.into(), ..Default::default() },
                            ..Default::default()
                        }));
                    rows.push(
                        widget::row::with_children(vec![
                            bar.into(),
                            widget::container(text::body(event.when()).class(theme::Text::Custom(|_| iced::widget::text::Style { color: Some(palette::MUTED), ..Default::default() })))
                                .width(Length::Fixed(44.0))
                                .into(),
                            text::body(event.summary.clone()).into(),
                        ])
                        .spacing(10)
                        .align_y(Alignment::Center)
                        .padding([3, 0])
                        .into(),
                    );
                }
            }
            _ => rows.push(text::body("No events").class(theme::Text::Custom(|_| iced::widget::text::Style { color: Some(palette::rgba(232, 236, 246, 0.55)), ..Default::default() })).into()),
        }
        Self::box_(widget::column::with_children(rows).spacing(4), [12, 14])
    }

    fn weather_box(&self) -> Element<'_, Message> {
        let unit = widget::button::custom(text::small(if self.fahrenheit { "°F" } else { "°C" }))
            .padding([3, 8])
            .class(theme::Button::Custom {
                active: Box::new(|_, _| unit_style(false)),
                disabled: Box::new(|_| unit_style(false)),
                hovered: Box::new(|_, _| unit_style(true)),
                pressed: Box::new(|_, _| unit_style(true)),
            })
            .on_press(Message::ToggleUnit);
        let place = widget::row::with_children(vec![
            widget::container(icon("map-pin", 14)).class(theme::Container::custom(|_| widget::container::Style { icon_color: Some(palette::MUTED), ..Default::default() })).into(),
            text::small(self.weather.as_ref().map_or_else(|| "Locating…".to_owned(), |w| w.place.clone())).class(theme::Text::Custom(|_| iced::widget::text::Style { color: Some(palette::MUTED), ..Default::default() })).into(),
            widget::space::horizontal().into(),
            unit.into(),
        ])
        .spacing(4)
        .align_y(Alignment::Center);
        let mut rows: Vec<Element<'_, Message>> = vec![place.into()];
        match &self.weather {
            Some(weather) => {
                let (description, icon_name) = weather::describe(weather.code);
                let now = widget::row::with_children(vec![
                    widget::container(icon(icon_name, 34)).class(theme::Container::custom(|_| widget::container::Style { icon_color: Some(palette::KNOB), ..Default::default() })).into(),
                    text::sized_bold(self.temperature(weather.celsius), 32.0).into(),
                    widget::column::with_children(vec![
                        text::sized(description, 12.5).into(),
                        widget::row::with_children(vec![
                            widget::container(icon("wind", 12)).class(theme::Container::custom(|_| widget::container::Style { icon_color: Some(palette::MUTED), ..Default::default() })).into(),
                            text::sized(format!("{} {:.0} km/h", weather.wind_from, weather.wind_kmh), 12.5).class(theme::Text::Custom(|_| iced::widget::text::Style { color: Some(palette::MUTED), ..Default::default() })).into(),
                        ])
                        .spacing(4)
                        .align_y(Alignment::Center)
                        .into(),
                    ])
                    .into(),
                ])
                .spacing(12)
                .align_y(Alignment::Center);
                rows.push(widget::container(now).padding([8, 0, 0, 0]).into());
                let hours: Vec<Element<'_, Message>> = weather
                    .hours
                    .iter()
                    .enumerate()
                    .map(|(i, hour)| {
                        let (_, name) = weather::describe(hour.code);
                        widget::container(
                            widget::column::with_children(vec![
                                text::small(hour.label.clone()).class(theme::Text::Custom(|_| iced::widget::text::Style { color: Some(palette::rgba(232, 236, 246, 0.6)), ..Default::default() })).into(),
                                icon(name, 18).into(),
                                text::small(self.temperature(hour.celsius)).into(),
                            ])
                            .spacing(5)
                            .align_x(Alignment::Center),
                        )
                        .width(Length::FillPortion(1))
                        .padding([6, 0])
                        .center_x(Length::Fill)
                        .class(theme::Container::custom(move |_| widget::container::Style {
                            background: (i == 0).then_some(Background::Color(palette::ROW_HOVER)),
                            border: iced::Border { radius: 10.0.into(), ..Default::default() },
                            ..Default::default()
                        }))
                        .into()
                    })
                    .collect();
                rows.push(widget::container(widget::row::with_children(hours).spacing(4)).padding([10, 0, 0, 0]).into());
            }
            None => rows.push(text::body("Looking…").class(theme::Text::Custom(|_| iced::widget::text::Style { color: Some(palette::FAINT), ..Default::default() })).into()),
        }
        Self::box_(widget::column::with_children(rows), [12, 14])
    }

    fn clocks_box(&self) -> Element<'_, Message> {
        let rows: Vec<Element<'_, Message>> = CLOCKS
            .iter()
            .map(|(name, tz)| {
                let time = self.now.with_timezone(tz).format("%H:%M").to_string();
                widget::row::with_children(vec![
                    text::body(*name).width(Length::Fill).into(),
                    text::body(time).class(theme::Text::Custom(|_| iced::widget::text::Style { color: Some(palette::rgba(232, 236, 246, 0.8)), ..Default::default() })).into(),
                ])
                .padding([3, 0])
                .into()
            })
            .collect();
        Self::box_(widget::column::with_children(rows), [12, 14])
    }
}

fn round_style(hovered: bool) -> widget::button::Style {
    widget::button::Style {
        background: Some(Background::Color(if hovered { palette::ROW_SELECTED } else { iced::Color::TRANSPARENT })),
        text_color: Some(palette::TEXT),
        icon_color: Some(palette::TEXT),
        border_radius: 15.0.into(),
        ..widget::button::Style::new()
    }
}

fn unit_style(hovered: bool) -> widget::button::Style {
    widget::button::Style {
        background: Some(Background::Color(palette::rgba(160, 190, 235, if hovered { 0.18 } else { 0.10 }))),
        text_color: Some(palette::TEXT),
        border_radius: 8.0.into(),
        ..widget::button::Style::new()
    }
}

impl Clock {
    pub fn new() -> Self {
        let now = Local::now();
        let today = now.date_naive();
        Self {
            shown: false,
            wants_close: false,
            now,
            view: first_of_month(today),
            selected: today,
            events: Calendar::new(),
            weather: None,
            fahrenheit: false,
            notifications: notifications::load(),
            notices: Vec::new(),
            leaving: Vec::new(),
            do_not_disturb: backend::dnd::get(),
        }
    }

    pub fn update(&mut self, message: Message) -> app::Task<Message> {
        match message {
            Message::Tick => {
                let was = self.today();
                self.now = Local::now();
                if self.today() != was && self.selected == was {
                    self.selected = self.today();
                    self.view = first_of_month(self.today());
                }
            }
            Message::Done => {}
            Message::Events(events) => self.events = events,
            Message::Weather(weather) => self.weather = Some(weather),
            Message::Notification(event) => match event {
                notifications::Event::New(notification) => {
                    self.notifications.retain(|n| n.id != notification.id || n.restored);
                    self.notifications.insert(0, notification);
                    self.notifications.truncate(50);
                    notifications::save(&self.notifications);
                }
                notifications::Event::Closed(id) => {
                    self.notifications.retain(|n| n.id != id || n.restored);
                    notifications::save(&self.notifications);
                }
                notifications::Event::Dismissed => {}
            },
            // Dismissed cards go first (fade, slide out), then leave the list.
            Message::Dismiss(id) => self.leave(&[id]),
            Message::Clear => {
                let ids: Vec<u32> = self.notifications.iter().map(|n| n.id).collect();
                self.leave(&ids);
            }
            Message::Frame => {
                let gone_after = std::time::Duration::from_millis(ui::notifications::LEAVE_MS);
                let gone: Vec<u32> = self.leaving.iter().filter(|(_, since)| since.elapsed() >= gone_after).map(|(id, _)| *id).collect();
                if gone.is_empty() {
                    return Task::none();
                }
                self.leaving.retain(|(id, _)| !gone.contains(id));
                let live: Vec<u32> = self.notifications.iter().filter(|n| gone.contains(&n.id) && !n.restored).map(|n| n.id).collect();
                self.notifications.retain(|n| !gone.contains(&n.id));
                notifications::save(&self.notifications);
                return Task::future(async move {
                    notifications::dismiss(live).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::Action(id, action) => {
                // Taken by the clock button, which holds the notification
                // daemon's socket; the app it raises takes over.
                self.notifications.retain(|n| n.id != id);
                notifications::save(&self.notifications);
                self.wants_close = true;
                return Task::future(async move {
                    bus::ask_invoke(id, action).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::SetDnd(on) => {
                self.do_not_disturb = on;
                backend::dnd::set(on);
            }
            Message::PreviousMonth => self.view = months_on(self.view, -1),
            Message::NextMonth => self.view = months_on(self.view, 1),
            Message::Select(date) => {
                self.selected = date;
                if date.month() != self.view.month() || date.year() != self.view.year() {
                    self.view = first_of_month(date);
                }
            }
            Message::Today => {
                self.selected = self.today();
                self.view = first_of_month(self.today());
            }
            Message::ToggleUnit => self.fahrenheit = !self.fahrenheit,
            Message::Notice(backend::nixos::NoticeAction::Restart) => {
                // COSMIC's own dialog asks first.
                backend::session::restart();
                self.wants_close = true;
            }
        }
        Task::none()
    }

    /// These cards set going, those not already.
    fn leave(&mut self, ids: &[u32]) {
        let now = std::time::Instant::now();
        for id in ids {
            if !self.leaving.iter().any(|(l, _)| l == id) {
                self.leaving.push((*id, now));
            }
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            iced::time::every(Duration::from_secs(if self.shown { 1 } else { 20 })).map(|_| Message::Tick),
            backend::ics::subscription().map(Message::Events),
            backend::weather::subscription().map(Message::Weather),
        ];
        // About a frame each, while cards are going.
        if !self.leaving.is_empty() {
            subscriptions.push(iced::time::every(Duration::from_millis(16)).map(|_| Message::Frame));
        }
        Subscription::batch(subscriptions)
    }
}
