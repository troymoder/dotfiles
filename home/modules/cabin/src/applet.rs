// SPDX-License-Identifier: GPL-3.0-only

//! The buttons on the panel, thin: each draws what the daemon tells it and
//! asks the daemon to open its panel. The status icons (the wheel over
//! them sets the volume), the clock, and the System button with its live
//! CPU and memory bars. White on the transparent panel, with a shadow so
//! they read on any wallpaper.

use chrono::{DateTime, Local};
use cosmic::{
    Element, Task, app,
    iced::{self, Alignment, Background, Color, Length, Subscription, mouse::ScrollDelta},
    theme, widget,
};

use crate::{
    backend::{self, notifications},
    bus,
    ui::{icon, palette, text},
};

/// The shadow under what's on the panel.
const SHADOW: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.5 };

/// Something drawn twice: its shadow a pixel lower, then itself. Only
/// itself takes room, so the panel stays as thick as it was.
fn shadowed<'a, M: 'a>(shadow: Element<'a, M>, top: Element<'a, M>) -> Element<'a, M> {
    Element::new(Shadowed { shadow, top })
}

struct Shadowed<'a, M> {
    shadow: Element<'a, M>,
    top: Element<'a, M>,
}

impl<M> iced::advanced::Widget<M, cosmic::Theme, cosmic::Renderer> for Shadowed<'_, M> {
    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        vec![iced::advanced::widget::Tree::new(&self.top), iced::advanced::widget::Tree::new(&self.shadow)]
    }

    fn diff(&mut self, tree: &mut iced::advanced::widget::Tree) {
        tree.diff_children(&mut [&mut self.top, &mut self.shadow]);
    }

    fn size(&self) -> iced::Size<Length> {
        self.top.as_widget().size()
    }

    fn layout(&mut self, tree: &mut iced::advanced::widget::Tree, renderer: &cosmic::Renderer, limits: &iced::advanced::layout::Limits) -> iced::advanced::layout::Node {
        let top = self.top.as_widget_mut().layout(&mut tree.children[0], renderer, limits);
        let shadow = self.shadow.as_widget_mut().layout(&mut tree.children[1], renderer, limits);
        iced::advanced::layout::Node::with_children(top.size(), vec![top, shadow])
    }

    fn draw(
        &self,
        tree: &iced::advanced::widget::Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let mut children = layout.children();
        let (Some(top), Some(shadow)) = (children.next(), children.next()) else {
            return;
        };
        renderer.with_translation(iced::Vector::new(0.0, 1.0), |renderer| {
            self.shadow.as_widget().draw(&tree.children[1], renderer, theme, style, shadow, cursor, viewport);
        });
        self.top.as_widget().draw(&tree.children[0], renderer, theme, style, top, cursor, viewport);
    }
}

fn bar_icon<'a, M: 'a>(name: &str, size: u16) -> Element<'a, M> {
    let shadow = widget::container(icon(name, size)).class(theme::Container::custom(|_| widget::container::Style { icon_color: Some(SHADOW), ..Default::default() }));
    let top = widget::container(icon(name, size)).class(theme::Container::custom(|_| widget::container::Style { icon_color: Some(Color::WHITE), ..Default::default() }));
    shadowed(shadow.into(), top.into())
}

fn bar_text<'a, M: 'a>(content: String, size: f32, bold: bool) -> Element<'a, M> {
    let make = |color: Color| -> Element<'a, M> {
        let t = if bold { text::sized_bold(content.clone(), size) } else { text::sized(content.clone(), size) };
        t.wrapping(iced::widget::text::Wrapping::None).class(theme::Text::Color(color)).into()
    };
    shadowed(make(SHADOW), make(Color::WHITE))
}

/// A panel button: a shade under the pointer, and while its panel is open.
fn button_class(open: bool) -> theme::Button {
    let style = move |hovered: bool| widget::button::Style {
        background: Some(Background::Color(if hovered || open { palette::alpha(Color::WHITE, 0.16) } else { Color::TRANSPARENT })),
        text_color: Some(Color::WHITE),
        icon_color: Some(Color::WHITE),
        border_radius: 12.0.into(),
        ..widget::button::Style::new()
    };
    theme::Button::Custom {
        active: Box::new(move |_, _| style(false)),
        disabled: Box::new(move |_| style(false)),
        hovered: Box::new(move |_, _| style(true)),
        pressed: Box::new(move |_, _| style(true)),
    }
}

fn padding(core: &app::Core) -> [u16; 2] {
    let (major, minor) = core.applet.suggested_padding(true);
    if core.applet.is_horizontal() { [minor, major] } else { [major, minor] }
}

/// A call to the daemon, as a task.
fn ask<F, Fut>(what: &'static str, f: F) -> app::Task<Message>
where
    F: FnOnce(bus::CabinProxy<'static>) -> Fut + Send + 'static,
    Fut: Future<Output = zbus::Result<()>> + Send,
{
    Task::future(async move {
        bus::call(what, f).await;
        cosmic::action::app(Message::Done)
    })
}

#[derive(Clone, Debug)]
pub enum Message {
    Bar(String),
    Toggle,
    /// The privacy pill: the controls, with the Privacy menu open.
    Privacy,
    Scroll(ScrollDelta),
    Surface(cosmic::surface::Action),
    /// The daemon asks to open Settings, at this page or where it was.
    Settings(String),
    Tick,
    Notification(notifications::Event),
    Invoke(u32, String),
    Done,
}

// ---- the status icons

pub struct Status {
    core: app::Core,
    bar: bus::StatusBar,
}

impl Status {
    fn with_tooltip<'a>(&self, content: impl Into<Element<'a, Message>>, tip: String) -> Element<'a, Message> {
        self.core.applet.applet_tooltip(content, tip, self.bar.open, Message::Surface, None).into()
    }

    /// The pill while something watches or listens: amber, coral while the
    /// screen is shared.
    fn privacy_pill(&self, size: u16) -> Option<Element<'_, Message>> {
        let pill = self.bar.privacy.as_ref()?;
        let mut icons: Vec<Element<'_, Message>> = Vec::new();
        if pill.screen {
            icons.push(icon("screen-share", size).into());
        }
        if pill.camera {
            icons.push(icon("camera", size).into());
        }
        if pill.mic {
            icons.push(icon(if pill.mic_muted { "microphone-off" } else { "microphone" }, size).into());
        }
        let (background, on) = if pill.screen { (palette::OVER, palette::ON_OVER) } else { (palette::AMBER, palette::ON_AMBER) };
        let style = move |_: &cosmic::Theme| widget::button::Style {
            background: Some(Background::Color(background)),
            text_color: Some(on),
            icon_color: Some(on),
            border_radius: 12.0.into(),
            ..widget::button::Style::new()
        };
        let button = widget::button::custom(widget::row::with_children(icons).spacing(7).align_y(Alignment::Center))
            .padding([4, 10])
            .class(theme::Button::Custom {
                active: Box::new(move |_, t| style(t)),
                disabled: Box::new(style),
                hovered: Box::new(move |_, t| style(t)),
                pressed: Box::new(move |_, t| style(t)),
            })
            .on_press(Message::Privacy);
        Some(self.with_tooltip(button, pill.tip.clone()))
    }

    /// Raised if it's running, and told the page; else started with a
    /// token from the compositor, so it may take the focus.
    fn settings(page: String) -> app::Task<Message> {
        Task::future(async move {
            const SETTINGS: &str = "com.system76.CosmicSettings";
            let page = (!page.is_empty()).then_some(page);
            if backend::toplevels::activate(SETTINGS.to_owned()).await {
                if page.is_some() {
                    backend::session::settings(page.as_deref(), None);
                }
            } else {
                let token = backend::toplevels::activation_token(SETTINGS.to_owned()).await;
                backend::session::settings(page.as_deref(), token.as_deref());
            }
            cosmic::action::app(Message::Done)
        })
    }
}

impl cosmic::Application for Status {
    type Message = Message;
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    const APP_ID: &'static str = crate::APP_ID;

    fn init(core: app::Core, _flags: ()) -> (Self, app::Task<Message>) {
        (Self { core, bar: bus::StatusBar::default() }, text::load_font().map(|()| cosmic::action::app(Message::Done)))
    }

    fn core(&self) -> &app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut app::Core {
        &mut self.core
    }

    fn style(&self) -> Option<iced::theme::Style> {
        Some(cosmic::applet::style())
    }

    fn update(&mut self, message: Message) -> app::Task<Message> {
        match message {
            Message::Bar(json) => match serde_json::from_str(&json) {
                Ok(bar) => self.bar = bar,
                Err(err) => tracing::warn!("cabind sent a status bar this can't read: {err}"),
            },
            Message::Toggle => return ask("open the controls", |d| async move { d.toggle("controls").await }),
            Message::Privacy => return ask("open the Privacy menu", |d| async move { d.open("controls", "privacy").await }),
            Message::Scroll(delta) => {
                // A notch of the wheel is a step; a touchpad's 30 px too.
                let steps = match delta {
                    ScrollDelta::Lines { y, .. } => y as f64,
                    ScrollDelta::Pixels { y, .. } => y as f64 / 30.0,
                };
                return ask("set the volume", move |d| async move { d.volume(steps).await });
            }
            Message::Surface(action) => return cosmic::task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(action))),
            Message::Settings(page) => return Self::settings(page),
            Message::Tick | Message::Notification(_) | Message::Invoke(..) | Message::Done => {}
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([bus::bar("status").map(Message::Bar), bus::settings_requests().map(Message::Settings)])
    }

    fn view(&self) -> Element<'_, Message> {
        let (size, _) = self.core.applet.suggested_size(true);
        let mut items: Vec<Element<'_, Message>> = self.bar.icons.iter().map(|(name, tip)| self.with_tooltip(bar_icon(name, size), tip.clone())).collect();
        if let Some(level) = self.bar.headphones {
            items.push(self.with_tooltip(bar_text(format!("{level} %"), 12.0, false), format!("Headphones battery {level} %")));
        }
        if let Some((name, percent, tip)) = &self.bar.battery {
            let shown = widget::row::with_children(vec![bar_icon(name, size), bar_text(format!("{percent:.0} %"), 12.0, false)]).spacing(6).align_y(Alignment::Center);
            items.push(self.with_tooltip(shown, tip.clone()));
        }
        if items.is_empty() {
            // The daemon not answering yet: something to press all the same.
            items.push(bar_icon("settings", size));
        }
        let row = widget::row::with_children(items).spacing(6).align_y(Alignment::Center);
        let button = widget::button::custom(row).padding(padding(&self.core)).class(button_class(self.bar.open)).on_press(Message::Toggle);
        let button = widget::mouse_area(button).on_scroll(Message::Scroll);
        let mut bar = widget::row::with_capacity(2).spacing(4).align_y(Alignment::Center);
        if let Some(pill) = self.privacy_pill(size) {
            bar = bar.push(pill);
        }
        bar = bar.push(button);
        widget::autosize::autosize(bar, widget::Id::new("cabin-status")).into()
    }
}

// ---- the clock

pub struct Clock {
    core: app::Core,
    bar: bus::ClockBar,
    now: DateTime<Local>,
}

impl cosmic::Application for Clock {
    type Message = Message;
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    const APP_ID: &'static str = crate::CLOCK_ID;

    fn init(core: app::Core, _flags: ()) -> (Self, app::Task<Message>) {
        (Self { core, bar: bus::ClockBar::default(), now: Local::now() }, text::load_font().map(|()| cosmic::action::app(Message::Done)))
    }

    fn core(&self) -> &app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut app::Core {
        &mut self.core
    }

    fn style(&self) -> Option<iced::theme::Style> {
        Some(cosmic::applet::style())
    }

    fn update(&mut self, message: Message) -> app::Task<Message> {
        match message {
            Message::Bar(json) => match serde_json::from_str(&json) {
                Ok(bar) => self.bar = bar,
                Err(err) => tracing::warn!("cabind sent a clock bar this can't read: {err}"),
            },
            Message::Tick => self.now = Local::now(),
            Message::Toggle => return ask("open the date menu", |d| async move { d.toggle("date").await }),
            // Notifications come to this button (the panel hands it the
            // daemon's socket); the daemon keeps and shows them.
            Message::Notification(notifications::Event::New(notification)) => {
                let Ok(json) = serde_json::to_string(&notification) else {
                    return Task::none();
                };
                return ask("pass on a notification", move |d| async move { d.notified(&json).await });
            }
            Message::Notification(notifications::Event::Closed(id)) => return ask("pass on a closed notification", move |d| async move { d.notification_closed(id).await }),
            Message::Notification(notifications::Event::Dismissed) => {}
            Message::Invoke(id, key) => {
                return Task::future(async move {
                    notifications::invoke(id, key).await;
                    cosmic::action::app(Message::Done)
                });
            }
            Message::Privacy | Message::Scroll(_) | Message::Surface(_) | Message::Settings(_) | Message::Done => {}
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            bus::bar("clock").map(Message::Bar),
            iced::time::every(std::time::Duration::from_secs(2)).map(|_| Message::Tick),
            notifications::subscription().map(Message::Notification),
            bus::invoke_requests().map(|(id, key)| Message::Invoke(id, key)),
        ])
    }

    /// The date and the time; a dot while there are notifications, the
    /// bell crossed out while they're held back.
    fn view(&self) -> Element<'_, Message> {
        let label = bar_text(self.now.format("%a %-d %b  %H:%M").to_string(), 12.0, true);
        // The mark has a slot of its own, there or not, so the button keeps
        // its width as notifications come and go.
        let mark: Element<'_, Message> = if self.bar.dnd {
            bar_icon("bell-off", 13)
        } else if self.bar.unread {
            widget::container(widget::space::horizontal())
                .width(Length::Fixed(6.0))
                .height(Length::Fixed(6.0))
                .class(theme::Container::custom(|_| widget::container::Style {
                    background: Some(Background::Color(palette::KNOB)),
                    border: iced::Border { radius: 3.0.into(), width: 1.5, color: Color { a: 0.35, ..Color::BLACK } },
                    ..Default::default()
                }))
                .into()
        } else {
            widget::space::horizontal().width(Length::Fixed(1.0)).into()
        };
        let mark = widget::container(mark).width(Length::Fixed(14.0)).align_x(Alignment::Center);
        let row = widget::row::with_capacity(2).spacing(6).align_y(Alignment::Center).push(label).push(mark);
        let button = widget::button::custom(row).padding(padding(&self.core)).class(button_class(self.bar.open)).on_press(Message::Toggle);
        widget::autosize::autosize(button, widget::Id::new("cabin-clock")).into()
    }
}

// ---- the System button

pub struct System {
    core: app::Core,
    bar: bus::SystemBar,
}

/// A small upright bar, filled from the bottom: white on a faint track.
fn mini_bar<'a>(fraction: f32) -> Element<'a, Message> {
    let filled = (fraction.clamp(0.0, 1.0) * 100.0).round() as u16;
    let mut parts: Vec<Element<'a, Message>> = Vec::new();
    if filled < 100 {
        parts.push(widget::space::vertical().height(Length::FillPortion(100 - filled)).into());
    }
    if filled > 0 {
        parts.push(
            widget::container(widget::space::horizontal())
                .width(Length::Fill)
                .height(Length::FillPortion(filled))
                .class(theme::Container::custom(|_| widget::container::Style { background: Some(Background::Color(Color::WHITE)), ..Default::default() }))
                .into(),
        );
    }
    widget::container(widget::column::with_children(parts).height(Length::Fill))
        .width(Length::Fixed(5.0))
        .height(Length::Fixed(14.0))
        .class(theme::Container::custom(|_| widget::container::Style {
            background: Some(Background::Color(palette::alpha(Color::WHITE, 0.25))),
            border: iced::Border { radius: 2.0.into(), ..Default::default() },
            shadow: iced::Shadow { color: palette::alpha(Color::BLACK, 0.4), offset: iced::Vector::new(0.0, 1.0), blur_radius: 2.0 },
            ..Default::default()
        }))
        .into()
}

impl cosmic::Application for System {
    type Message = Message;
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    const APP_ID: &'static str = crate::SYSTEM_ID;

    fn init(core: app::Core, _flags: ()) -> (Self, app::Task<Message>) {
        (Self { core, bar: bus::SystemBar::default() }, text::load_font().map(|()| cosmic::action::app(Message::Done)))
    }

    fn core(&self) -> &app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut app::Core {
        &mut self.core
    }

    fn style(&self) -> Option<iced::theme::Style> {
        Some(cosmic::applet::style())
    }

    fn update(&mut self, message: Message) -> app::Task<Message> {
        match message {
            Message::Bar(json) => match serde_json::from_str(&json) {
                Ok(bar) => self.bar = bar,
                Err(err) => tracing::warn!("cabind sent a System bar this can't read: {err}"),
            },
            Message::Toggle => return ask("open the System panel", |d| async move { d.toggle("system").await }),
            Message::Surface(action) => return cosmic::task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(action))),
            _ => {}
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        bus::bar("system").map(Message::Bar)
    }

    fn view(&self) -> Element<'_, Message> {
        // Wide enough for "100 %", so the bars don't shift as it changes.
        let label = widget::container(bar_text(self.bar.label.clone(), 12.0, false)).width(Length::Fixed(40.0)).align_x(Alignment::End);
        let row = widget::row::with_children(vec![mini_bar(self.bar.cpu), mini_bar(self.bar.memory), label.into()]).spacing(4).align_y(Alignment::Center);
        let button = widget::button::custom(row).padding(padding(&self.core)).class(button_class(self.bar.open)).on_press(Message::Toggle);
        let tip = if self.bar.tip.is_empty() { "System".to_owned() } else { self.bar.tip.clone() };
        let button = self.core.applet.applet_tooltip(button, tip, self.bar.open, Message::Surface, None);
        widget::autosize::autosize(button, widget::Id::new("cabin-system")).into()
    }
}
