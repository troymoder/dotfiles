// SPDX-License-Identifier: GPL-3.0-only

//! The dialog asking to log out to switch graphics mode: a scrim over the
//! screen and a panel-styled card with a countdown, as the design draws
//! it. The applet spawns it, since an applet can't map overlays of its own.
//! On yes it asks supergfxd for the mode and ends the session, within the
//! 30 s supergfxd waits for a logout.
//!
//! graphics-logout <mode> <mode name>

use std::time::Duration;

use cabin::ui::{glass, icon, palette, text};
use cosmic::{
    Element, app,
    iced::{
        self, Alignment, Background, Color, Length, Limits, Subscription, event,
        platform_specific::shell::commands::layer_surface::{
            Anchor, KeyboardInteractivity, destroy_layer_surface, get_layer_surface,
        },
        runtime::platform_specific::wayland::layer_surface::SctkLayerSurfaceSettings,
        time, window,
    },
    theme, widget,
};

const COUNTDOWN: u8 = 60;
const WIDTH: f32 = 560.0;

#[derive(Clone, Debug)]
enum Message {
    Tick,
    Confirm,
    Cancel,
    Done,
}

struct Dialog {
    core: app::Core,
    mode: u32,
    name: String,
    surface: window::Id,
    countdown: u8,
}

#[zbus::proxy(
    interface = "org.supergfxctl.Daemon",
    default_service = "org.supergfxctl.Daemon",
    default_path = "/org/supergfxctl/Gfx"
)]
trait SuperGfx {
    fn set_mode(&self, mode: u32) -> zbus::Result<u32>;
}

#[zbus::proxy(
    interface = "com.system76.CosmicSession",
    default_service = "com.system76.CosmicSession",
    default_path = "/com/system76/CosmicSession"
)]
trait CosmicSession {
    fn exit(&self) -> zbus::Result<()>;
}

async fn switch_and_log_out(mode: u32) -> zbus::Result<()> {
    let system = zbus::Connection::system().await?;
    SuperGfxProxy::new(&system).await?.set_mode(mode).await?;
    let session = zbus::Connection::session().await?;
    CosmicSessionProxy::new(&session).await?.exit().await
}

/// A dialog button: the pale chip, or the accent for the one that goes ahead.
fn dialog_button<'a>(label: &'a str, primary: bool, message: Message) -> Element<'a, Message> {
    let style = move |hovered: bool| widget::button::Style {
        background: Some(Background::Color(match (primary, hovered) {
            (true, true) => palette::ACCENT_HOVER,
            (true, false) => palette::ACCENT,
            (false, true) => palette::rgba(160, 190, 235, 0.2),
            (false, false) => palette::rgba(160, 190, 235, 0.12),
        })),
        text_color: Some(if primary {
            palette::ON_ACCENT
        } else {
            palette::TEXT
        }),
        border_radius: 12.0.into(),
        ..widget::button::Style::new()
    };
    widget::button::custom(text::sized_bold(label, 13.5))
        .padding([9, 18])
        .class(theme::Button::Custom {
            active: Box::new(move |_, _| style(false)),
            disabled: Box::new(move |_| style(false)),
            hovered: Box::new(move |_, _| style(true)),
            pressed: Box::new(move |_, _| style(true)),
        })
        .on_press(message)
        .into()
}

impl cosmic::Application for Dialog {
    type Executor = iced::executor::Default;
    type Flags = (u32, String);
    type Message = Message;
    const APP_ID: &'static str = "dev.troy.Cabin.GraphicsLogout";

    fn core(&self) -> &app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut app::Core {
        &mut self.core
    }

    fn init(mut core: app::Core, (mode, name): (u32, String)) -> (Self, app::Task<Message>) {
        core.set_app_type(cosmic::core::AppType::System);
        let surface = window::Id::unique();
        let dialog = Self {
            core,
            mode,
            name,
            surface,
            countdown: COUNTDOWN,
        };
        // The whole screen: the scrim, with the card near the top.
        let open = get_layer_surface(SctkLayerSurfaceSettings {
            id: surface,
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            namespace: "dialog".into(),
            size: None,
            size_limits: Limits::NONE.min_width(1.0).min_height(1.0),
            ..Default::default()
        });
        let font = match text::file().and_then(|path| std::fs::read(path).ok()) {
            Some(bytes) => iced::font::load(std::borrow::Cow::Owned(bytes))
                .map(|_| cosmic::action::app(Message::Done)),
            None => app::Task::none(),
        };
        (dialog, app::Task::batch([open, font]))
    }

    fn style(&self) -> Option<iced::theme::Style> {
        Some(iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: palette::TEXT,
            icon_color: palette::TEXT,
        })
    }

    fn update(&mut self, message: Message) -> app::Task<Message> {
        match message {
            Message::Done => app::Task::none(),
            Message::Tick => {
                self.countdown = self.countdown.saturating_sub(1);
                if self.countdown == 0 {
                    return self.update(Message::Confirm);
                }
                app::Task::none()
            }
            Message::Cancel => std::process::exit(0),
            Message::Confirm => {
                let mode = self.mode;
                app::Task::batch([
                    destroy_layer_surface(self.surface),
                    app::Task::future(async move {
                        if let Err(err) = switch_and_log_out(mode).await {
                            eprintln!("graphics-logout: {err}");
                        }
                        std::process::exit(0)
                    }),
                ])
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        widget::text("").into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        if id != self.surface {
            return widget::text("").into();
        }
        let chip = widget::container(icon("logout", 26))
            .center(Length::Fixed(52.0))
            .class(theme::Container::custom(|_| widget::container::Style {
                background: Some(Background::Color(palette::ACCENT)),
                icon_color: Some(palette::ON_ACCENT),
                border: iced::Border {
                    radius: 14.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }));
        let body = format!(
            "Switching graphics to {} needs a new session. Logging out in {} s.",
            self.name, self.countdown
        );
        let words = widget::column::with_children(vec![
            text::sized_bold("Log out?", 20.0).into(),
            text::sized(body, 13.5)
                .class(theme::Text::Color(palette::rgba(232, 236, 246, 0.78)))
                .into(),
        ])
        .spacing(6);
        let top = widget::row::with_children(vec![chip.into(), words.into()])
            .spacing(18)
            .align_y(Alignment::Start);
        let buttons = widget::row::with_children(vec![
            widget::space::horizontal().into(),
            dialog_button("Cancel", false, Message::Cancel),
            dialog_button("Log out", true, Message::Confirm),
        ])
        .spacing(10);
        let card = glass(
            widget::column::with_children(vec![top.into(), buttons.into()]).spacing(22),
            [26.0, 26.0, 20.0, 26.0],
            Some(WIDTH),
        );
        // The scrim: a press on it is a Cancel; the card swallows its own.
        widget::mouse_area(
            widget::container(cosmic::iced::widget::stack([
                widget::mouse_area(
                    widget::container(widget::space::horizontal())
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .class(theme::Container::custom(|_| widget::container::Style {
                            background: Some(Background::Color(palette::rgba(5, 8, 18, 0.45))),
                            ..Default::default()
                        })),
                )
                .on_press(Message::Cancel)
                .into(),
                widget::container(widget::mouse_area(card).on_press(Message::Done))
                    .width(Length::Fill)
                    .align_x(Alignment::Center)
                    .padding([240, 0, 0, 0])
                    .into(),
            ]))
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            time::every(Duration::from_secs(1)).map(|_| Message::Tick),
            event::listen_with(|event, _, _| match event {
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                }) => Some(Message::Cancel),
                _ => None,
            }),
        ])
    }
}

fn main() -> iced::Result {
    cabin::hand_over_to_dev();
    cabin::prefer_integrated_gpu();
    let mut args = std::env::args().skip(1);
    let mode: u32 = args.next().and_then(|m| m.parse().ok()).unwrap_or(0);
    let name = args.next().unwrap_or_else(|| "another mode".to_owned());
    cosmic::app::run::<Dialog>(
        app::Settings::default()
            .no_main_window(true)
            .exit_on_close(false),
        (mode, name),
    )
}
