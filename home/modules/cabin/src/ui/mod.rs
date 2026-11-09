// SPDX-License-Identifier: GPL-3.0-only

//! How things look, after the design: navy glass panels, periwinkle for
//! what's on, Cantarell, Tabler icons. Everything here is drawn the same
//! way everywhere.

pub mod audio_pane;
pub mod behind;
pub mod grain;
pub mod icons;
pub mod main_pane;
pub mod mark;
pub mod marquee;
pub mod notifications;
pub mod pair;
pub mod palette;
pub mod popup;
pub mod registry;
pub mod sparkline;
pub mod system_pane;
pub mod text;

use cosmic::{
    Element,
    iced::{self, Alignment, Background, Color, Length, border::Radius},
    theme,
    widget::{self, button},
};

use palette::{ACCENT, ACCENT_HOVER, CARD, CTX, CTX_BORDER, KNOB, MUTED, ON_ACCENT, ON_ACCENT_SUB, OVER, PANE, PANE_BORDER, RED, RED_HOVER, ROW_HOVER, ROW_SELECTED, SEP, SURFACE, SURFACE_HOVER, TEXT, TRACK, alpha};

/// A panel's width.
pub const PANE_WIDTH: f32 = 400.0;
/// Between panels.
pub const GAP: f32 = 10.0;
/// Between things in a panel.
pub const INNER_GAP: f32 = 8.0;
pub const PANE_RADIUS: f32 = 22.0;
pub const PANE_PADDING: f32 = 16.0;
/// A tile's height and corners.
pub const TILE: f32 = 52.0;
pub const TILE_RADIUS: f32 = 14.0;
/// A square button at the top (`.ib`), and a round one by a slider (`.ab`).
pub const SQUARE: f32 = 36.0;
pub const ROUND: f32 = 34.0;

fn custom_button(style: impl Fn(bool, &cosmic::Theme) -> button::Style + 'static + Clone) -> theme::Button {
    let s1 = style.clone();
    let s2 = style.clone();
    let s3 = style.clone();
    theme::Button::Custom {
        active: Box::new(move |_, theme| s1(false, theme)),
        disabled: Box::new(move |theme| {
            let mut s = style(false, theme);
            s.icon_color = s.icon_color.map(|c| alpha(c, 0.4));
            s.text_color = s.text_color.map(|c| alpha(c, 0.5));
            s
        }),
        hovered: Box::new(move |_, theme| s2(true, theme)),
        pressed: Box::new(move |_, theme| s3(true, theme)),
    }
}

/// A panel: navy glass with a faint edge, its content padded inside. One
/// taller than the room under the bar scrolls, with a hair of a scrollbar.
pub fn pane<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    let scrollbar = iced::widget::scrollable::Scrollbar::new().width(3).scroller_width(3).margin(0);
    let content = widget::scrollable(widget::container(content).width(Length::Fill))
        .direction(iced::widget::scrollable::Direction::Vertical(scrollbar))
        .width(Length::Fill)
        .height(Length::Shrink);
    // Under the grip, which leaves room for a slider's bubble too.
    widget::container(content).width(Length::Fixed(PANE_WIDTH)).padding([4.0, PANE_PADDING, PANE_PADDING, PANE_PADDING]).into()
}

/// How tall a panel's grip strip is: the panel's content starts under it.
pub const GRIP: f32 = 28.0;

/// The strip along a panel's top: a grip to drag it by (the whole strip
/// takes the press, a double click snaps it back) and a pin to keep it up.
/// Faint until the pointer is over the panel, it's pinned, or it's moving.
pub fn grip<'a, M: Clone + 'a>(pinned: bool, strong: bool, dragging: bool, on_grab: M, on_snap: M, on_pin: M) -> Element<'a, M> {
    let k = if strong || pinned || dragging { 1.0 } else { 0.35 };
    let bar = if dragging { TEXT } else { alpha(TEXT, 0.55 * k) };
    let pill = widget::container(widget::space::horizontal()).width(Length::Fixed(44.0)).height(Length::Fixed(5.0)).class(theme::Container::custom(move |_| widget::container::Style {
        background: Some(Background::Color(bar)),
        border: iced::Border { radius: 3.0.into(), ..Default::default() },
        ..Default::default()
    }));
    let glyph = alpha(TEXT, 0.8 * k);
    let pin = button::custom(widget::container(icon(if pinned { "pinned" } else { "pin" }, 12)).center(Length::Fill).class(inherit()))
        .width(Length::Fixed(20.0))
        .height(Length::Fixed(20.0))
        .padding(0)
        .class(custom_button(move |hovered, _| button::Style {
            background: Some(Background::Color(if pinned {
                ACCENT
            } else if hovered {
                SURFACE_HOVER
            } else {
                Color::TRANSPARENT
            })),
            text_color: Some(if pinned { ON_ACCENT } else { glyph }),
            icon_color: Some(if pinned { ON_ACCENT } else { glyph }),
            border_radius: 6.0.into(),
            ..button::Style::new()
        }))
        .on_press(on_pin);
    let row = widget::row::with_children(vec![widget::container(pill).padding([8, 6]).into(), pin.into()]).spacing(0).align_y(Alignment::Center);
    widget::mouse_area(widget::container(row).width(Length::Fill).height(Length::Fixed(GRIP)).align_x(Alignment::Center).padding([1, 0, 0, 0]).class(inherit()))
        .on_press(on_grab)
        .on_double_click(on_snap)
        .interaction(if dragging { iced::mouse::Interaction::Grabbing } else { iced::mouse::Interaction::Grab })
        .into()
}

/// A panel as it floats: the glass, the grip strip, what's in it. A
/// pinned one has a ring in the accent.
pub fn panel<'a, M: 'a>(grip: impl Into<Element<'a, M>>, content: impl Into<Element<'a, M>>, width: f32, pinned: bool) -> Element<'a, M> {
    let edge = if pinned { (alpha(ACCENT, 0.6), 2.0) } else { (PANE_BORDER, 1.0) };
    glass_edged(widget::column::with_children(vec![grip.into(), content.into()]), 0.0_f32, Some(width), edge)
}

/// A menu inside a panel: a darker pool with room inside.
pub fn card<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    widget::container(content)
        .width(Length::Fill)
        .padding(12)
        .class(theme::Container::custom(|_| widget::container::Style {
            background: Some(Background::Color(CARD)),
            text_color: Some(TEXT),
            icon_color: Some(TEXT),
            border: iced::Border { radius: 18.0.into(), ..Default::default() },
            ..Default::default()
        }))
        .into()
}

/// A container that keeps the colors it's given.
pub fn inherit() -> theme::Container<'static> {
    theme::Container::custom(|_| widget::container::Style::default())
}

/// A menu's heading: the icon on a lit square, and the name.
pub fn card_header<'a, M: 'a>(icon_name: &str, title: &'a str) -> Element<'a, M> {
    card_header_with(icon_name, title, None)
}

/// A menu's heading with something big at its right end: a battery's charge.
pub fn card_header_with<'a, M: 'a>(icon_name: &str, title: &'a str, right: Option<String>) -> Element<'a, M> {
    let chip = widget::container(icon(icon_name, 19)).center(Length::Fixed(40.0)).class(theme::Container::custom(|_| widget::container::Style {
        background: Some(Background::Color(ACCENT)),
        icon_color: Some(ON_ACCENT),
        text_color: Some(ON_ACCENT),
        border: iced::Border { radius: 12.0.into(), ..Default::default() },
        ..Default::default()
    }));
    let mut row = widget::row::with_capacity(3).spacing(14).align_y(Alignment::Center).push(chip).push(text::heading(title));
    if let Some(right) = right {
        row = row.push(widget::space::horizontal()).push(widget::container(text::sized_bold(right, 20.0)).padding([0, 10, 0, 0]));
    }
    widget::container(row).width(Length::Fill).padding([4, 4, 10, 4]).into()
}

/// A thin line across a menu.
pub fn rule<'a, M: 'a>() -> Element<'a, M> {
    widget::container(widget::container(widget::space::horizontal()).width(Length::Fill).height(Length::Fixed(1.0)).class(theme::Container::custom(|_| {
        widget::container::Style { background: Some(Background::Color(SEP)), ..Default::default() }
    })))
    .padding([8, 14])
    .into()
}

pub fn icon(name: &str, size: u16) -> widget::icon::Icon {
    icons::icon(name, size)
}

pub fn chevron(open: bool) -> &'static str {
    if open { "chevron-down" } else { "chevron-right" }
}

fn surface_style(lit: bool, radius: Radius) -> theme::Button {
    custom_button(move |hovered, _| button::Style {
        background: Some(Background::Color(match (lit, hovered) {
            (true, true) => ACCENT_HOVER,
            (true, false) => ACCENT,
            (false, true) => SURFACE_HOVER,
            (false, false) => SURFACE,
        })),
        text_color: Some(if lit { ON_ACCENT } else { TEXT }),
        icon_color: Some(if lit { ON_ACCENT } else { TEXT }),
        border_radius: radius,
        ..button::Style::new()
    })
}

/// A square button with an icon, at the top of the panel.
pub fn square_button<'a, M: Clone + 'a>(icon_name: &str, on_press: Option<M>, lit: bool) -> Element<'a, M> {
    button::custom(widget::container(icon(icon_name, 17)).center(Length::Fill).class(inherit()))
        .width(Length::Fixed(SQUARE))
        .height(Length::Fixed(SQUARE))
        .padding(0)
        .class(surface_style(lit, 12.0.into()))
        .on_press_maybe(on_press)
        .into()
}

/// A round button with an icon: the arrow by a slider or a mixer row.
pub fn round_button<'a, M: Clone + 'a>(icon_name: &str, on_press: Option<M>, lit: bool) -> Element<'a, M> {
    button::custom(widget::container(icon(icon_name, 16)).center(Length::Fill).class(inherit()))
        .width(Length::Fixed(ROUND))
        .height(Length::Fixed(ROUND))
        .padding(0)
        .class(surface_style(lit, (ROUND / 2.0).into()))
        .on_press_maybe(on_press)
        .into()
}

/// A pill with an icon and a word: the battery with its charge.
pub fn round_label<'a, M: Clone + 'a>(icon_name: &str, label: String, on_press: Option<M>, lit: bool) -> Element<'a, M> {
    let content = widget::row::with_children(vec![icon(icon_name, 15).into(), text::body(label).into()]).spacing(6).align_y(Alignment::Center);
    button::custom(widget::container(content).center_y(Length::Fill).class(inherit()))
        .height(Length::Fixed(SQUARE))
        .padding([0, 12])
        .class(surface_style(lit, 12.0.into()))
        .on_press_maybe(on_press)
        .into()
}

/// The red square button: for shutting down.
pub fn danger_button<'a, M: Clone + 'a>(icon_name: &str, on_press: Option<M>) -> Element<'a, M> {
    button::custom(widget::container(icon(icon_name, 17)).center(Length::Fill).class(inherit()))
        .width(Length::Fixed(SQUARE))
        .height(Length::Fixed(SQUARE))
        .padding(0)
        .class(custom_button(|hovered, _| button::Style {
            background: Some(Background::Color(if hovered { RED_HOVER } else { RED })),
            text_color: Some(palette::ON_RED),
            icon_color: Some(palette::ON_RED),
            border_radius: 12.0.into(),
            ..button::Style::new()
        }))
        .on_press_maybe(on_press)
        .into()
}

fn one_line() -> iced::core::text::Ellipsize {
    iced::core::text::Ellipsize::End(iced::core::text::EllipsizeHeightLimit::Lines(1))
}

/// How a tile is lit: off, on in the accent, or in a warning color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Lit {
    #[default]
    Off,
    On,
    /// A camera or microphone in use.
    Amber,
    /// The screen being shared.
    Coral,
}

impl Lit {
    pub fn when(on: bool) -> Self {
        if on { Lit::On } else { Lit::Off }
    }

    /// The outline in the tone's own color: the warning tones are glass
    /// with an edge, not a fill.
    fn edge(self) -> Option<Color> {
        match self {
            Lit::Amber => Some(alpha(palette::AMBER, 0.6)),
            Lit::Coral => Some(alpha(OVER, 0.65)),
            _ => None,
        }
    }

    /// The icon in the tone's color, where it isn't the text's.
    fn glyph(self) -> Option<Color> {
        match self {
            Lit::Amber => Some(palette::AMBER),
            Lit::Coral => Some(palette::rgb_(0xec, 0x8d, 0x9c)),
            _ => None,
        }
    }

    /// Background, background under the pointer, text, second line.
    fn colors(self) -> (Color, Color, Color, Color) {
        match self {
            Lit::Off => (Color::TRANSPARENT, SURFACE_HOVER, TEXT, MUTED),
            Lit::On => (ACCENT, ACCENT_HOVER, ON_ACCENT, ON_ACCENT_SUB),
            // The warning tones: glass with a faint wash of the color, pale text.
            Lit::Amber => (alpha(palette::AMBER, 0.16), alpha(palette::AMBER, 0.24), TEXT, palette::rgb_(0xf6, 0xcf, 0x94)),
            Lit::Coral => (alpha(OVER, 0.18), alpha(OVER, 0.26), TEXT, palette::rgb_(0xf2, 0xb3, 0xbe)),
        }
    }
}

/// The look of a tile's parts: lit, else see-through, a shade under the
/// pointer.
fn tile_style(lit: Lit, radius: Radius) -> theme::Button {
    custom_button(move |hovered, _| {
        let (resting, hover, on, _) = lit.colors();
        let edge = lit.edge();
        button::Style {
            background: Some(Background::Color(if hovered { hover } else { resting })),
            text_color: Some(on),
            icon_color: Some(on),
            border_radius: radius,
            border_width: if edge.is_some() { 1.0 } else { 0.0 },
            border_color: edge.unwrap_or(Color::TRANSPARENT),
            ..button::Style::new()
        }
    })
}

/// A tile: icon, name, status; lit when on. Pressing it toggles; the arrow
/// at its end, past a thin line, opens its menu and points down while
/// that's open.
pub fn tile<'a, M: Clone + 'a>(
    icon_name: &str,
    title: &'a str,
    subtitle: Option<String>,
    lit: Lit,
    toggle: Option<M>,
    details: Option<M>,
    open: bool,
) -> Element<'a, M> {
    tile_with_dot(icon_name, title, subtitle, lit, toggle, details, open, false)
}

/// A tile, with a small pink dot on its icon's corner when something there
/// wants attention (a reboot to finish a switch).
#[allow(clippy::too_many_arguments)]
pub fn tile_with_dot<'a, M: Clone + 'a>(
    icon_name: &str,
    title: &'a str,
    subtitle: Option<String>,
    lit: Lit,
    toggle: Option<M>,
    details: Option<M>,
    open: bool,
    dot: bool,
) -> Element<'a, M> {
    let (_, _, _, sub_color) = lit.colors();
    let mut lines = widget::column::with_capacity(2).push(text::title(title).ellipsize(one_line()));
    if let Some(subtitle) = subtitle {
        let sub = text::sub(subtitle).ellipsize(one_line());
        lines = lines.push(sub.class(theme::Text::Color(sub_color)));
    }
    let glyph: Element<'a, M> = if dot {
        let mark = widget::container(widget::space::horizontal()).width(Length::Fixed(7.0)).height(Length::Fixed(7.0)).class(theme::Container::custom(|_| widget::container::Style {
            background: Some(Background::Color(palette::EVENT)),
            border: iced::Border { radius: 3.5.into(), ..Default::default() },
            ..Default::default()
        }));
        iced::widget::stack([
            widget::container(icon(icon_name, 17)).center(Length::Fixed(21.0)).class(inherit()).into(),
            widget::container(mark).align_right(Length::Fixed(21.0)).align_top(Length::Fixed(21.0)).into(),
        ])
        .into()
    } else {
        icon(icon_name, 17).into()
    };
    let glyph: Element<'a, M> = match lit.glyph() {
        Some(color) => widget::container(glyph).class(theme::Container::custom(move |_| widget::container::Style { icon_color: Some(color), ..Default::default() })).into(),
        None => glyph,
    };
    let content = widget::row::with_capacity(2).push(glyph).push(lines.width(Length::Fill)).spacing(10).align_y(Alignment::Center);
    let has_arrow = details.is_some();
    let main_radius: Radius = if has_arrow { [TILE_RADIUS, 0.0, 0.0, TILE_RADIUS].into() } else { TILE_RADIUS.into() };
    let main = button::custom(widget::container(content).center_y(Length::Fill).class(inherit()))
        .width(Length::Fill)
        .height(Length::Fixed(TILE))
        .padding([0, 12])
        .class(tile_style(lit, main_radius))
        .on_press_maybe(toggle);
    let Some(details) = details else {
        return main.into();
    };
    let line = widget::container(widget::space::vertical())
        .width(Length::Fixed(1.0))
        .height(Length::Fixed(TILE))
        .class(theme::Container::custom(move |_| widget::container::Style {
            background: Some(Background::Color(match lit {
                Lit::On => palette::rgba(20, 29, 56, 0.35),
                Lit::Amber => alpha(palette::AMBER, 0.35),
                Lit::Coral => alpha(OVER, 0.35),
                Lit::Off => palette::rgba(160, 190, 235, 0.14),
            })),
            ..Default::default()
        }));
    let arrow = button::custom(widget::container(icon(chevron(open), 16)).center(Length::Fill).class(inherit()))
        .width(Length::Fixed(36.0))
        .height(Length::Fixed(TILE))
        .padding(0)
        .class(tile_style(lit, [0.0, TILE_RADIUS, TILE_RADIUS, 0.0].into()))
        .on_press(details);
    widget::row::with_children(vec![main.into(), line.into(), arrow.into()]).align_y(Alignment::Center).into()
}

/// A menu row, built up: an icon, a name, maybe a second line and more
/// under it (a meter, a graph), something at the end; a shade under the
/// pointer, more when it's the one picked; dim when it's just a word.
pub struct Row<'a, M> {
    icon: Option<String>,
    title: String,
    subtitle: Option<String>,
    extra: Option<Element<'a, M>>,
    trailing: Option<Element<'a, M>>,
    on_press: Option<M>,
    selected: bool,
    dim: bool,
    /// Pushed in under the icons of the rows around it.
    indent: bool,
    /// Something to see to: the icon amber, the name pale amber.
    warn: bool,
}

impl<'a, M: Clone + 'a> Row<'a, M> {
    pub fn new(title: impl Into<String>) -> Self {
        Self { icon: None, title: title.into(), subtitle: None, extra: None, trailing: None, on_press: None, selected: false, dim: false, indent: false, warn: false }
    }

    pub fn warn(mut self, warn: bool) -> Self {
        self.warn = warn;
        self
    }

    pub fn icon(mut self, name: &str) -> Self {
        self.icon = Some(name.to_owned());
        self
    }

    pub fn icon_maybe(mut self, name: Option<&str>) -> Self {
        self.icon = name.map(str::to_owned);
        self
    }

    pub fn sub(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn sub_maybe(mut self, subtitle: Option<String>) -> Self {
        self.subtitle = subtitle;
        self
    }

    pub fn extra(mut self, extra: impl Into<Element<'a, M>>) -> Self {
        self.extra = Some(extra.into());
        self
    }

    pub fn trailing(mut self, trailing: impl Into<Element<'a, M>>) -> Self {
        self.trailing = Some(trailing.into());
        self
    }

    pub fn trailing_maybe(mut self, trailing: Option<Element<'a, M>>) -> Self {
        self.trailing = trailing;
        self
    }

    pub fn on_press(mut self, message: M) -> Self {
        self.on_press = Some(message);
        self
    }

    pub fn on_press_maybe(mut self, message: Option<M>) -> Self {
        self.on_press = message;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn dim(mut self, dim: bool) -> Self {
        self.dim = dim;
        self
    }

    pub fn indent(mut self) -> Self {
        self.indent = true;
        self
    }

    pub fn view(self) -> Element<'a, M> {
        let dim = self.dim;
        let color = if dim { palette::FAINT } else { TEXT };
        let warn = self.warn;
        let top = self.extra.is_some();
        let mut row = widget::row::with_capacity(3).spacing(12).align_y(if top { Alignment::Start } else { Alignment::Center });
        if let Some(name) = &self.icon {
            let glyph = icon(name, 16);
            let glyph: Element<'a, M> = if warn {
                widget::container(glyph).class(theme::Container::custom(|_| widget::container::Style { icon_color: Some(palette::AMBER), ..Default::default() })).into()
            } else {
                glyph.into()
            };
            row = row.push(if top { widget::container(glyph).padding([2, 0, 0, 0]).class(inherit()).into() } else { glyph });
        }
        let title_color = if warn { palette::rgb_(0xf6, 0xcf, 0x94) } else { color };
        let mut lines = widget::column::with_capacity(3).push(text::row(self.title).ellipsize(one_line()).class(theme::Text::Color(title_color)));
        if let Some(subtitle) = self.subtitle {
            lines = lines.push(text::row_sub(subtitle).ellipsize(one_line()).class(theme::Text::Color(if dim { palette::rgba(232, 236, 246, 0.4) } else { palette::rgba(232, 236, 246, 0.55) })));
        }
        if let Some(extra) = self.extra {
            lines = lines.push(extra);
        }
        row = row.push(lines.width(Length::Fill));
        if let Some(trailing) = self.trailing {
            row = row.push(trailing);
        }
        let interactive = self.on_press.is_some() && !dim;
        let selected = self.selected;
        let left = if self.indent { 40 } else if self.icon.is_some() { 14 } else { 18 };
        let (vertical, radius) = if self.indent { (6, 10.0) } else { (10, 12.0) };
        button::custom(row)
            .width(Length::Fill)
            .padding([vertical, 14, vertical, left])
            .class(custom_button(move |hovered, _| button::Style {
                background: Some(Background::Color(if selected { ROW_SELECTED } else if hovered && interactive { ROW_HOVER } else { Color::TRANSPARENT })),
                text_color: Some(color),
                icon_color: Some(color),
                border_radius: radius.into(),
                ..button::Style::new()
            }))
            .on_press_maybe(if dim { None } else { self.on_press })
            // A row that just tells something keeps its full color.
            .force_enabled(!dim)
            .into()
    }
}

/// A menu row: an icon, a name and maybe a second line, something at the
/// end; a shade under the pointer, more when it's the one picked.
pub fn menu_row<'a, M: Clone + 'a>(
    icon_name: Option<&str>,
    title: String,
    subtitle: Option<String>,
    trailing: Option<Element<'a, M>>,
    on_press: Option<M>,
    selected: bool,
) -> Element<'a, M> {
    Row::new(title).icon_maybe(icon_name).sub_maybe(subtitle).trailing_maybe(trailing).on_press_maybe(on_press).selected(selected).view()
}

/// A thin bar under a row's text: how full something is, 0 to 1.
pub fn meter<'a, M: 'a>(fraction: f32) -> Element<'a, M> {
    let fraction = fraction.clamp(0.0, 1.0);
    let fill = widget::container(widget::space::horizontal())
        .width(Length::FillPortion((fraction * 1000.0).round().max(1.0) as u16))
        .height(Length::Fixed(4.0))
        .class(theme::Container::custom(|_| widget::container::Style {
            background: Some(Background::Color(ACCENT)),
            border: iced::Border { radius: 2.0.into(), ..Default::default() },
            ..Default::default()
        }));
    let rest = widget::container(widget::space::horizontal()).width(Length::FillPortion(((1.0 - fraction) * 1000.0).round().max(1.0) as u16)).height(Length::Fixed(4.0));
    // The track is the bar's own 4 px; the 6 px above it is just room.
    let track = widget::container(widget::row::with_children(vec![fill.into(), rest.into()]).width(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fixed(4.0))
        .class(theme::Container::custom(|_| widget::container::Style {
            background: Some(Background::Color(TRACK)),
            border: iced::Border { radius: 2.0.into(), ..Default::default() },
            ..Default::default()
        }));
    widget::container(track).width(Length::Fill).padding([6, 0, 0, 0]).class(inherit()).into()
}

/// A small coral button at the end of a row: "Stop".
pub fn action_button<'a, M: Clone + 'a>(icon_name: &str, label: &'a str, on_press: M) -> Element<'a, M> {
    button::custom(widget::row::with_children(vec![icon(icon_name, 13).into(), text::sized_bold(label, 12.5).into()]).spacing(6).align_y(Alignment::Center))
        .padding([6, 12])
        .class(custom_button(|hovered, _| button::Style {
            background: Some(Background::Color(if hovered { palette::OVER_HOVER } else { OVER })),
            text_color: Some(palette::ON_OVER),
            icon_color: Some(palette::ON_OVER),
            border_radius: 10.0.into(),
            ..button::Style::new()
        }))
        .on_press(on_press)
        .into()
}

/// Something rich under a row's text, like a graph, set off from it.
pub fn under<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    widget::container(content).width(Length::Fill).padding([6, 0, 0, 0]).class(inherit()).into()
}

/// A plain menu row: icon, name, something at the end.
pub fn row_item<'a, M: Clone + 'a>(icon_name: Option<&str>, title: String, trailing: Option<Element<'a, M>>, on_press: Option<M>) -> Element<'a, M> {
    menu_row(icon_name, title, None, trailing, on_press, false)
}

/// A check at the end of a row: the one in use.
pub fn check<'a, M: 'a>() -> Element<'a, M> {
    widget::container(icon("check", 15))
        .class(theme::Container::custom(|_| widget::container::Style { icon_color: Some(KNOB), ..Default::default() }))
        .into()
}

/// A faint icon at the end of a row: a lock on a secured network.
pub fn faint_icon<'a, M: 'a>(name: &str) -> Element<'a, M> {
    widget::container(icon(name, 15))
        .class(theme::Container::custom(|_| widget::container::Style { icon_color: Some(palette::rgba(232, 236, 246, 0.8)), ..Default::default() }))
        .into()
}

/// A line of lesser text in a menu: a section's label, or a word on its own.
pub fn note<'a, M: 'a>(content: impl Into<String>) -> Element<'a, M> {
    widget::container(text::body(content.into()).class(theme::Text::Custom(|_| iced::widget::text::Style {
        color: Some(palette::rgba(232, 236, 246, 0.6)),
        ..Default::default()
    })))
    .padding([8, 14, 4, 14])
    .into()
}

/// A section's label in a menu.
pub fn heading<'a, M: 'a>(content: &'a str) -> Element<'a, M> {
    note(content)
}

/// A pill in a menu: "7 more".
pub fn pill<'a, M: Clone + 'a>(label: String, on_press: M) -> Element<'a, M> {
    widget::container(
        button::custom(text::sized(label, 12.5))
            .padding([5, 14])
            .class(custom_button(|hovered, _| button::Style {
                background: Some(Background::Color(palette::rgba(160, 190, 235, if hovered { 0.2 } else { 0.12 }))),
                text_color: Some(palette::rgba(232, 236, 246, 0.8)),
                border_radius: 99.0.into(),
                ..button::Style::new()
            }))
            .on_press(on_press),
    )
    .center_x(Length::Fill)
    .padding([4, 0, 2, 0])
    .into()
}

/// A switch at the end of a row.
pub fn switch<'a, M: Clone + 'static>(on: bool, toggle: impl Fn(bool) -> M + 'a) -> Element<'a, M> {
    widget::toggler(on).on_toggle(toggle).size(24).into()
}

/// Dimmed text.
pub fn dimmed(_theme: &cosmic::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style { color: Some(MUTED), ..Default::default() }
}

/// A right-click menu: a small card at the pointer with what can be done,
/// gone once the pointer leaves it.
pub fn menu<'a>(what: &crate::app::MenuFor) -> Element<'a, crate::app::Message> {
    use crate::app::{MenuFor, Message};
    let rows: Vec<Element<'a, Message>> = match what {
        MenuFor::App(app) => {
            let name = registry::name(app);
            vec![
                row_item(Some("volume"), format!("Solo {name}"), None, Some(Message::SoloStream(app.clone()))),
                row_item(Some("eye-off"), format!("Hide {name}"), None, Some(Message::HideApp(app.clone()))),
            ]
        }
        MenuFor::Player(app) => vec![row_item(Some("eye-off"), format!("Hide {}'s player", registry::name(app)), None, Some(Message::HidePlayer(app.clone())))],
    };
    widget::mouse_area(
        widget::container(widget::column::with_children(rows)).padding(6).class(theme::Container::custom(|_| widget::container::Style {
            background: Some(Background::Color(CTX)),
            text_color: Some(TEXT),
            icon_color: Some(TEXT),
            border: iced::Border { radius: 16.0.into(), width: 1.0, color: CTX_BORDER },
            shadow: iced::Shadow { color: Color { a: 0.45, ..Color::BLACK }, offset: iced::Vector::new(0.0, 10.0), blur_radius: 28.0 },
            ..Default::default()
        })),
    )
    .on_exit(Message::CloseMenu)
    .into()
}

/// How a slider is colored: in the accent; gray when muted; red past 100 %.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Tone {
    #[default]
    Normal,
    Muted,
    Boost,
    /// A color of its own: a player's.
    Color(Color),
}

/// A slider with its icon before it: pressing the icon does something
/// else (mutes, say), and the arrow after it opens a menu.
pub struct SliderRow<'a, M> {
    pub icon: &'a str,
    /// 0 to 1.
    pub value: f32,
    /// Points it snaps to when dragged near, 0 to 1; not drawn.
    pub snaps: Vec<f32>,
    pub tone: Tone,
    /// Shown over the knob while it's dragged: "75 %".
    pub label: Option<String>,
    /// A mark on the rail, 0 to 1 (the 100 % of a boosted volume); the
    /// rail past it is red.
    pub mark: Option<f32>,
    pub on_change: Box<dyn Fn(f32) -> M + 'a>,
    pub on_release: Option<M>,
    pub on_icon_press: Option<M>,
    pub details: Option<M>,
    pub open: bool,
    pub dimmed: bool,
    /// A live level shown inside the rail, 0 to 1.
    pub level: Option<f32>,
}

impl<'a, M: Clone + 'a> SliderRow<'a, M> {
    pub fn new(icon: &'a str, value: f32, on_change: impl Fn(f32) -> M + 'a) -> Self {
        Self {
            icon,
            value: value.clamp(0.0, 1.0),
            snaps: Vec::new(),
            tone: Tone::Normal,
            label: None,
            mark: None,
            on_change: Box::new(on_change),
            on_release: None,
            on_icon_press: None,
            details: None,
            open: false,
            dimmed: false,
            level: None,
        }
    }

    pub fn snaps(mut self, snaps: impl IntoIterator<Item = f32>) -> Self {
        self.snaps = snaps.into_iter().collect();
        self
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub fn label(mut self, label: Option<String>) -> Self {
        self.label = label;
        self
    }

    pub fn mark(mut self, at: f32) -> Self {
        self.mark = Some(at);
        self
    }

    pub fn on_release(mut self, message: M) -> Self {
        self.on_release = Some(message);
        self
    }

    pub fn on_icon_press(mut self, message: Option<M>) -> Self {
        self.on_icon_press = message;
        self
    }

    pub fn details(mut self, message: Option<M>, open: bool) -> Self {
        self.details = message;
        self.open = open;
        self
    }

    pub fn level(mut self, level: Option<f32>) -> Self {
        self.level = level;
        self
    }

    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }

    pub fn view(self) -> Element<'a, M> {
        let icon_button: Element<'a, M> = button::custom(widget::container(icon(self.icon, 17)).center(Length::Fill).class(inherit()))
            .width(Length::Fixed(ROUND))
            .height(Length::Fixed(ROUND))
            .padding(0)
            .class(flat_button(self.dimmed))
            .on_press_maybe(self.on_icon_press)
            .into();
        let on_change = self.on_change;
        let snaps = self.snaps;
        let snapped = move |v: f32| snaps.iter().copied().find(|s| (s - v).abs() < 0.02).unwrap_or(v);
        let mut slider = widget::slider(0.0..=1.0, self.value, move |v| on_change(snapped(v)))
            .step(0.01_f32)
            .height(26.0)
            .width(Length::Fill)
            .class(slider_class(self.tone));
        if let Some(message) = self.on_release {
            slider = slider.on_release(message);
        }
        // What's drawn over the slider takes no room: the mark, the red
        // past it, and the level in a bubble while dragging.
        let slider: Element<'a, M> = if self.mark.is_some() || self.label.is_some() || self.level.is_some() {
            let over = self.mark.is_some_and(|at| self.value > at + 0.001);
            let label: Option<Element<'a, M>> = self.label.map(|t| text::sized_bold(t, 12.0).into());
            mark::Marked::new(slider, self.value).mark(self.mark).label(label).red(over).level(self.level).into()
        } else {
            slider.into()
        };
        let mut row = widget::row::with_capacity(3).spacing(14).align_y(Alignment::Center).push(icon_button).push(slider);
        if self.details.is_some() {
            row = row.push(round_button(chevron(self.open), self.details, self.open));
        }
        row.into()
    }
}

/// A slider's look by tone: a thin rail, a pale round knob.
pub fn slider_class(tone: Tone) -> theme::style::iced::Slider {
    use cosmic::iced::widget::slider;
    let style = move |_theme: &cosmic::Theme, hovered: bool| {
        let (active, knob): (Color, Color) = match tone {
            Tone::Normal => (ACCENT, KNOB),
            Tone::Muted => (palette::rgba(160, 190, 235, 0.35), palette::rgba(241, 226, 236, 0.6)),
            Tone::Boost => (ACCENT, palette::rgb_(0xf6, 0xd3, 0xda)),
            Tone::Color(color) => (color, KNOB),
        };
        slider::Style {
            rail: slider::Rail {
                backgrounds: (Background::Color(active), Background::Color(TRACK)),
                border: iced::Border { radius: 3.0.into(), ..Default::default() },
                width: 6.0,
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: 9.0 },
                border_color: if matches!(tone, Tone::Boost) { OVER } else { alpha(Color::BLACK, if hovered { 0.25 } else { 0.15 }) },
                border_width: if matches!(tone, Tone::Boost) { 2.0 } else { 1.0 },
                background: Background::Color(knob),
            },
            breakpoint: slider::Breakpoint { color: TEXT },
        }
    };
    theme::style::iced::Slider::Custom {
        active: std::rc::Rc::new(move |theme| style(theme, false)),
        hovered: std::rc::Rc::new(move |theme| style(theme, true)),
        dragging: std::rc::Rc::new(move |theme| style(theme, true)),
    }
}

/// A see-through button on the panel: the icon before a slider.
fn flat_button(dimmed: bool) -> theme::Button {
    custom_button(move |hovered, _| {
        let on = if dimmed { alpha(TEXT, 0.5) } else { TEXT };
        button::Style {
            background: Some(Background::Color(if hovered { SURFACE_HOVER } else { Color::TRANSPARENT })),
            text_color: Some(on),
            icon_color: Some(on),
            border_radius: 10.0.into(),
            ..button::Style::new()
        }
    })
}

/// A sheet of glass: the navy tint, grain over it, a hair of light along the
/// top edge and a faint border, the content padded inside. More room at the
/// top than the design: a slider's bubble rises there.
pub fn glass<'a, M: 'a>(content: impl Into<Element<'a, M>>, padding: impl Into<iced::Padding>, width: Option<f32>) -> Element<'a, M> {
    glass_edged(content, padding, width, (PANE_BORDER, 1.0))
}

/// The glass with its edge's color and width given.
pub fn glass_edged<'a, M: 'a>(content: impl Into<Element<'a, M>>, padding: impl Into<iced::Padding>, width: Option<f32>, (edge, edge_width): (Color, f32)) -> Element<'a, M> {
    let highlight = widget::container(widget::space::horizontal())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .class(theme::Container::custom(|_| widget::container::Style { background: Some(Background::Color(palette::PANE_HIGHLIGHT)), ..Default::default() }));
    let inner = widget::column::with_children(vec![
        widget::container(highlight).padding([0, PANE_RADIUS as u16]).width(Length::Fill).into(),
        widget::container(content).width(Length::Fill).padding(padding).into(),
    ]);
    let sheet = behind::Behind::new(grain::view(PANE_RADIUS), inner);
    let mut pane = widget::container(sheet).class(theme::Container::custom(move |_| widget::container::Style {
        background: Some(Background::Color(PANE)),
        text_color: Some(TEXT),
        icon_color: Some(TEXT),
        border: iced::Border { radius: PANE_RADIUS.into(), width: edge_width, color: edge },
        ..Default::default()
    }));
    if let Some(width) = width {
        pane = pane.width(Length::Fixed(width));
    }
    pane.into()
}
