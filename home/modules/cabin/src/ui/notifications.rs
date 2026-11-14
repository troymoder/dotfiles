// SPDX-License-Identifier: GPL-3.0-only

//! The notifications as the design lists them: a card each, newest first,
//! with the app and the age above, what it says, its picture; under them
//! do not disturb and a Clear button. Drawn for whichever applet holds them.

use std::time::SystemTime;

use cosmic::{
    Element,
    iced::{Alignment, Background, Length},
    theme, widget,
};

use super::{icon, palette, text};
use crate::backend::{
    nixos::{Notice, NoticeAction},
    notifications::Notification,
};

/// What the list can ask of its owner.
pub struct Actions<M> {
    pub dismiss: fn(u32) -> M,
    pub clear: fn() -> M,
    pub action: fn(u32, String) -> M,
    pub toggle_dnd: fn(bool) -> M,
    pub notice: fn(NoticeAction) -> M,
}

/// A row of icons at the top (do not disturb, clear), then the system's
/// pinned notices, then the list, filling the height it's given. `leaving`
/// says how far each dismissed card is through going, by id.
pub fn view<'a, M: Clone + 'static>(
    notifications: &'a [Notification],
    notices: &'a [Notice],
    dnd: bool,
    actions: &Actions<M>,
    leaving: &[(u32, f32)],
) -> Element<'a, M> {
    let body: Element<'a, M> = if notifications.is_empty() && notices.is_empty() {
        widget::container(
            widget::column::with_children(vec![
                widget::container(icon("bell-off", 40))
                    .class(theme::Container::custom(|_| widget::container::Style {
                        icon_color: Some(palette::FAINT),
                        ..Default::default()
                    }))
                    .into(),
                text::body(if dnd {
                    "Do not disturb is on"
                } else {
                    "No notifications"
                })
                .class(theme::Text::Custom(|_| cosmic::iced::widget::text::Style {
                    color: Some(palette::FAINT),
                    ..Default::default()
                }))
                .into(),
            ])
            .spacing(10)
            .align_x(Alignment::Center),
        )
        .center(Length::Fill)
        .height(Length::Fill)
        .into()
    } else {
        let progress = |id: u32| {
            leaving
                .iter()
                .find(|(l, _)| *l == id)
                .map_or(0.0, |(_, t)| *t)
        };
        let cards: Vec<Element<'a, M>> = notices
            .iter()
            .map(|n| notice_card(n, actions))
            .chain(
                notifications
                    .iter()
                    .map(|n| card(n, actions, progress(n.id))),
            )
            .collect();
        widget::scrollable(
            widget::column::with_children(cards)
                .spacing(8)
                .width(Length::Fill),
        )
        .height(Length::Fill)
        .into()
    };
    // Flat icons: the bell crossed while held back, the bin to clear.
    let header = widget::row::with_children(vec![
        text::heading("Notifications").into(),
        widget::space::horizontal().into(),
        icon_button(
            if dnd { "bell-off" } else { "bell" },
            dnd,
            Some((actions.toggle_dnd)(!dnd)),
        ),
        // Nothing to clear (the pinned notices stay): it can't be pressed.
        icon_button(
            "trash",
            false,
            notifications
                .iter()
                .any(|n| !leaving.iter().any(|(id, _)| *id == n.id))
                .then(|| (actions.clear)()),
        ),
    ])
    .spacing(4)
    .align_y(Alignment::Center);
    widget::column::with_children(vec![
        widget::container(header).padding([0, 0, 10, 4]).into(),
        body,
    ])
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// A bare icon that does something: no plate, a shade under the pointer;
/// lit when on, faint when there's nothing for it to do.
fn icon_button<'a, M: Clone + 'a>(name: &str, on: bool, message: Option<M>) -> Element<'a, M> {
    let enabled = message.is_some();
    let color = if on {
        palette::ACCENT
    } else if enabled {
        palette::MUTED
    } else {
        palette::alpha(palette::TEXT, 0.28)
    };
    let style = move |hovered: bool| widget::button::Style {
        background: Some(Background::Color(if hovered && enabled {
            palette::SURFACE_HOVER
        } else {
            cosmic::iced::Color::TRANSPARENT
        })),
        icon_color: Some(color),
        text_color: Some(color),
        border_radius: 10.0.into(),
        ..widget::button::Style::new()
    };
    widget::button::custom(
        widget::container(icon(name, 17))
            .center(Length::Fill)
            .class(super::inherit()),
    )
    .width(Length::Fixed(34.0))
    .height(Length::Fixed(34.0))
    .padding(0)
    .class(theme::Button::Custom {
        active: Box::new(move |_, _| style(false)),
        disabled: Box::new(move |_| style(false)),
        hovered: Box::new(move |_, _| style(true)),
        pressed: Box::new(move |_, _| style(true)),
    })
    .on_press_maybe(message)
    .into()
}

/// How long ago: "now", "5 min ago", "2 h ago", "3 d ago".
fn ago(time: SystemTime) -> String {
    let seconds = time.elapsed().unwrap_or_default().as_secs();
    match seconds {
        0..60 => "now".to_owned(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86400 => format!("{} h ago", seconds / 3600),
        _ => format!("{} d ago", seconds / 86400),
    }
}

/// Pale text at this strength, faded by `a` as the card leaves.
fn faint(strength: f32, a: f32) -> theme::Text {
    theme::Text::Color(palette::rgba(232, 236, 246, strength * a))
}

/// How long a dismissed card takes to go: fading, sliding right.
pub const LEAVE_MS: u64 = 200;
/// How far it slides as it goes.
const LEAVE_SLIDE: f32 = 24.0;

/// A notification's card. `leaving` is how far through going it is, 0 to
/// 1: it fades and slides right, and takes no presses meanwhile.
fn card<'a, M: Clone + 'static>(
    notification: &'a Notification,
    actions: &Actions<M>,
    leaving: f32,
) -> Element<'a, M> {
    let a = 1.0 - leaving.clamp(0.0, 1.0);
    let live = leaving <= 0.0;
    // An app's own icon is a picture that can't fade: gone halfway.
    let app_icon: Element<'a, M> = if a < 0.5 {
        widget::space::horizontal()
            .width(Length::Fixed(14.0))
            .into()
    } else if notification.icon.starts_with('/') {
        widget::icon::from_path(notification.icon.clone().into())
            .icon()
            .size(14)
            .into()
    } else if super::icons::known(&notification.icon) {
        widget::container(icon(&notification.icon, 14))
            .class(theme::Container::custom(move |_| {
                widget::container::Style {
                    icon_color: Some(palette::alpha(palette::TEXT, a)),
                    ..Default::default()
                }
            }))
            .into()
    } else {
        widget::icon::from_name(notification.icon.as_str())
            .size(64)
            .icon()
            .size(14)
            .into()
    };
    let header = widget::row::with_children(vec![
        app_icon,
        text::small(notification.app.clone())
            .class(faint(0.7, a))
            .into(),
        widget::space::horizontal().into(),
        text::small(ago(notification.time))
            .class(faint(0.7, a))
            .into(),
        widget::button::custom(
            widget::container(icon("x", 14))
                .center(Length::Fill)
                .class(super::inherit()),
        )
        .width(Length::Fixed(24.0))
        .height(Length::Fixed(24.0))
        .padding(0)
        .class(theme::Button::Custom {
            active: Box::new(move |_, _| x_style(false, a)),
            disabled: Box::new(move |_| x_style(false, a)),
            hovered: Box::new(move |_, _| x_style(true, a)),
            pressed: Box::new(move |_, _| x_style(true, a)),
        })
        .on_press_maybe(live.then(|| (actions.dismiss)(notification.id)))
        .into(),
    ])
    .spacing(8)
    .align_y(Alignment::Center);
    let mut column = widget::column::with_capacity(5).spacing(2).push(header);
    if !notification.summary.is_empty() {
        column = column.push(
            text::title(notification.summary.clone())
                .class(theme::Text::Color(palette::alpha(palette::TEXT, a))),
        );
    }
    if !notification.body.is_empty() {
        column = column.push(text::sized(notification.body.clone(), 12.5).class(faint(0.78, a)));
    }
    let buttons: Vec<Element<'a, M>> = notification
        .actions
        .iter()
        .filter(|(key, _)| key != "default")
        .map(|(key, label)| {
            widget::button::custom(text::small(label.clone()))
                .padding([4, 12])
                .class(theme::Button::Custom {
                    active: Box::new(move |_, _| pill_style(false, a)),
                    disabled: Box::new(move |_| pill_style(false, a)),
                    hovered: Box::new(move |_, _| pill_style(true, a)),
                    pressed: Box::new(move |_, _| pill_style(true, a)),
                })
                .on_press_maybe(live.then(|| (actions.action)(notification.id, key.clone())))
                .into()
        })
        .collect();
    if !buttons.is_empty() {
        column = column.push(
            widget::container(widget::row::with_children(buttons).spacing(6)).padding([6, 0, 0, 0]),
        );
    }
    if let Some(image) = &notification.image {
        column = column.push(
            widget::container(
                widget::image(widget::image::Handle::from_path(image))
                    .content_fit(cosmic::iced::ContentFit::Cover)
                    .width(Length::Fill)
                    .height(Length::Fixed(64.0))
                    .border_radius([10.0; 4])
                    .opacity(a),
            )
            .padding([8, 0, 0, 0]),
        );
    }
    let opens =
        notification.actions.iter().any(|(key, _)| key == "default") && !notification.restored;
    let body = widget::container(column)
        .width(Length::Fill)
        .padding([12, 14]);
    let card: Element<'a, M> = match opens {
        true => widget::button::custom(body)
            .width(Length::Fill)
            .padding(0)
            .class(theme::Button::Custom {
                active: Box::new(move |_, _| card_style(false, a)),
                disabled: Box::new(move |_| card_style(false, a)),
                hovered: Box::new(move |_, _| card_style(true, a)),
                pressed: Box::new(move |_, _| card_style(true, a)),
            })
            .on_press_maybe(live.then(|| (actions.action)(notification.id, "default".to_owned())))
            .into(),
        false => body
            .class(theme::Container::custom(move |_| {
                widget::container::Style {
                    background: Some(Background::Color(palette::alpha(
                        palette::CARD,
                        palette::CARD.a * a,
                    ))),
                    text_color: Some(palette::alpha(palette::TEXT, a)),
                    icon_color: Some(palette::alpha(palette::TEXT, a)),
                    border: cosmic::iced::Border {
                        radius: 16.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            }))
            .into(),
    };
    if live {
        card
    } else {
        Element::new(Shifted {
            child: card,
            by: LEAVE_SLIDE * leaving,
        })
    }
}

/// Something drawn moved right by so many pixels, its room unchanged: a
/// card sliding out.
struct Shifted<'a, M> {
    child: Element<'a, M>,
    by: f32,
}

impl<M> cosmic::iced::advanced::Widget<M, cosmic::Theme, cosmic::Renderer> for Shifted<'_, M> {
    fn children(&self) -> Vec<cosmic::iced::advanced::widget::Tree> {
        vec![cosmic::iced::advanced::widget::Tree::new(&self.child)]
    }

    fn diff(&mut self, tree: &mut cosmic::iced::advanced::widget::Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.child));
    }

    fn size(&self) -> cosmic::iced::Size<Length> {
        self.child.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut cosmic::iced::advanced::widget::Tree,
        renderer: &cosmic::Renderer,
        limits: &cosmic::iced::advanced::layout::Limits,
    ) -> cosmic::iced::advanced::layout::Node {
        let child = self
            .child
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        cosmic::iced::advanced::layout::Node::with_children(child.size(), vec![child])
    }

    fn draw(
        &self,
        tree: &cosmic::iced::advanced::widget::Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &cosmic::iced::advanced::renderer::Style,
        layout: cosmic::iced::advanced::Layout<'_>,
        cursor: cosmic::iced::mouse::Cursor,
        viewport: &cosmic::iced::Rectangle,
    ) {
        use cosmic::iced::advanced::Renderer as _;
        let Some(child) = layout.children().next() else {
            return;
        };
        renderer.with_translation(cosmic::iced::Vector::new(self.by, 0.0), |renderer| {
            self.child.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                child,
                cursor,
                viewport,
            );
        });
    }
}

/// A notice the system pins: amber-edged, a pin where the close button
/// would be, its button if it has one.
fn notice_card<'a, M: Clone + 'static>(notice: &'a Notice, actions: &Actions<M>) -> Element<'a, M> {
    let amber = |_: &cosmic::Theme| widget::container::Style {
        icon_color: Some(palette::AMBER),
        ..Default::default()
    };
    let header = widget::row::with_children(vec![
        widget::container(icon(notice.icon, 14))
            .class(theme::Container::custom(amber))
            .into(),
        text::small("NixOS").class(faint(0.7, 1.0)).into(),
        widget::space::horizontal().into(),
        widget::container(icon("pinned", 13))
            .class(theme::Container::custom(|_| widget::container::Style {
                icon_color: Some(palette::MUTED),
                ..Default::default()
            }))
            .padding([0, 5])
            .into(),
    ])
    .spacing(8)
    .align_y(Alignment::Center);
    let mut column = widget::column::with_capacity(4)
        .spacing(2)
        .push(header)
        .push(
            text::title(notice.title.clone())
                .class(theme::Text::Color(palette::rgb_(0xf6, 0xcf, 0x94))),
        );
    column = column.push(text::sized(notice.body.clone(), 12.5).class(faint(0.78, 1.0)));
    if let Some((label, action)) = notice.action {
        let button = widget::button::custom(text::small(label))
            .padding([4, 12])
            .class(theme::Button::Custom {
                active: Box::new(|_, _| pill_style(false, 1.0)),
                disabled: Box::new(|_| pill_style(false, 1.0)),
                hovered: Box::new(|_, _| pill_style(true, 1.0)),
                pressed: Box::new(|_, _| pill_style(true, 1.0)),
            })
            .on_press((actions.notice)(action));
        column = column.push(widget::container(button).padding([6, 0, 0, 0]));
    }
    widget::container(column)
        .width(Length::Fill)
        .padding([12, 14])
        .class(theme::Container::custom(|_| widget::container::Style {
            background: Some(Background::Color(palette::CARD)),
            text_color: Some(palette::TEXT),
            icon_color: Some(palette::TEXT),
            border: cosmic::iced::Border {
                radius: 16.0.into(),
                width: 1.0,
                color: palette::alpha(palette::AMBER, 0.45),
            },
            ..Default::default()
        }))
        .into()
}

fn card_style(hovered: bool, a: f32) -> widget::button::Style {
    let background = if hovered {
        palette::rgba(8, 12, 26, 0.5)
    } else {
        palette::CARD
    };
    widget::button::Style {
        background: Some(Background::Color(palette::alpha(
            background,
            background.a * a,
        ))),
        text_color: Some(palette::alpha(palette::TEXT, a)),
        icon_color: Some(palette::alpha(palette::TEXT, a)),
        border_radius: 16.0.into(),
        ..widget::button::Style::new()
    }
}

fn x_style(hovered: bool, a: f32) -> widget::button::Style {
    widget::button::Style {
        background: Some(Background::Color(palette::rgba(
            160,
            190,
            235,
            if hovered { 0.24 } else { 0.12 } * a,
        ))),
        text_color: Some(palette::alpha(palette::TEXT, a)),
        icon_color: Some(palette::alpha(palette::TEXT, a)),
        border_radius: 12.0.into(),
        ..widget::button::Style::new()
    }
}

/// A pill button: a notification's action.
fn pill_style(hovered: bool, a: f32) -> widget::button::Style {
    widget::button::Style {
        background: Some(Background::Color(palette::rgba(
            160,
            190,
            235,
            if hovered { 0.18 } else { 0.10 } * a,
        ))),
        text_color: Some(palette::alpha(palette::TEXT, a)),
        border_radius: 12.0.into(),
        ..widget::button::Style::new()
    }
}
