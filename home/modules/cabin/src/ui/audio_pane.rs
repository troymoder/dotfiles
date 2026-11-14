// SPDX-License-Identifier: GPL-3.0-only

//! The audio pane: the output and the microphone, each a slider with its
//! devices behind the arrow; then a card for each player.

use cosmic::{
    Element,
    iced::{Alignment, Background, Color, Length, gradient},
    theme, widget,
};

use super::{
    INNER_GAP, SliderRow, Tone, card, card_header, check, icon, inherit, marquee::Marquee, note,
    round_button, row_item, rule,
};

/// A player's card's corners, the backdrop's too.
const CARD_RADIUS: f32 = 16.0;

/// Every quarter up to the boost, in the slider's terms: where a drag snaps.
fn volume_snaps() -> impl Iterator<Item = f32> {
    [0.25, 0.5, 0.75, 1.0, 1.25]
        .into_iter()
        .map(|l| l / audio::MAX)
}

/// A level as "75 %".
pub fn percent(level: f32) -> String {
    format!("{:.0} %", level * 100.0)
}

fn tone(volume: audio::Volume, level: f32) -> Tone {
    if volume.muted {
        Tone::Muted
    } else if level > 1.0 + 0.001 {
        Tone::Boost
    } else {
        Tone::Normal
    }
}
use crate::{
    app::{Adjusting, App, Card, Message},
    backend::{
        audio::{self, Device},
        media::{self, Control, Media},
    },
};

impl App {
    pub(crate) fn audio_pane(&self) -> Element<'_, Message> {
        let mut rows: Vec<Element<'_, Message>> = Vec::new();
        match &self.audio {
            Some(audio) => {
                if let Some(output) = audio.output {
                    rows.push(self.output_row(output));
                    if self.card == Some(Card::Output) {
                        rows.push(card(self.devices_card(
                            &audio.sinks,
                            &audio.default_sink,
                            Message::SetSink,
                            "No outputs",
                            true,
                        )));
                    }
                }
                if let Some(input) = audio.input {
                    rows.push(self.input_row(input));
                    if self.card == Some(Card::Input) {
                        rows.push(card(self.devices_card(
                            &audio.sources,
                            &audio.default_source,
                            Message::SetSource,
                            "No microphones",
                            false,
                        )));
                    }
                }
            }
            None => rows.push(note("No sound")),
        }
        for media in self
            .media
            .iter()
            .filter(|m| !self.hidden_players.contains(&m.app))
        {
            // A right click on the card keeps the player out of sight.
            rows.push(
                widget::mouse_area(self.media_card(media))
                    .on_right_press(Message::OpenMenu(crate::app::MenuFor::Player(
                        media.app.clone(),
                    )))
                    .into(),
            );
        }
        for (stream, _) in self
            .streams
            .iter()
            .filter(|(s, _)| !self.hidden_apps.contains(&s.app))
        {
            rows.push(self.stream_row(stream));
            if self.stream_card.as_ref() == Some(&stream.app) {
                rows.push(card(self.stream_outputs(stream)));
            }
        }
        super::pane(widget::column::with_children(rows).spacing(INNER_GAP as u16))
    }

    fn output_row(&self, output: audio::Volume) -> Element<'_, Message> {
        let (level, dragging) = match &self.adjusting {
            Some(Adjusting::Output(level)) => (*level, true),
            _ => (output.level, false),
        };
        let headphones = self.audio.as_ref().is_some_and(|a| {
            let sink = a.default_sink.to_lowercase();
            let description = a
                .sinks
                .iter()
                .find(|s| s.name == a.default_sink)
                .map(|s| s.description.to_lowercase())
                .unwrap_or_default();
            sink.contains("bluez")
                || description.contains("headphone")
                || description.contains("headset")
        });
        let icon_name = if output.muted {
            "audio-volume-muted-symbolic"
        } else if headphones {
            "audio-headphones-symbolic"
        } else {
            output.output_icon()
        };
        SliderRow::new(icon_name, level / audio::MAX, |v| {
            Message::SetOutput(v * audio::MAX)
        })
        .snaps(volume_snaps())
        .mark(1.0 / audio::MAX)
        .tone(tone(output, level))
        .label(dragging.then(|| percent(level)))
        .on_release(Message::AdjustDone)
        .on_icon_press(Some(Message::ToggleOutputMute))
        // What's playing, inside the rail; nothing while muted.
        .level((!output.muted).then_some(self.output_level))
        .dimmed(output.muted)
        .details(
            Some(Message::ToggleCard(Card::Output)),
            self.card == Some(Card::Output),
        )
        .view()
    }

    fn input_row(&self, input: audio::Volume) -> Element<'_, Message> {
        let (level, dragging) = match &self.adjusting {
            Some(Adjusting::Input(level)) => (*level, true),
            _ => (input.level, false),
        };
        SliderRow::new(input.input_icon(), level / audio::MAX, |v| {
            Message::SetInput(v * audio::MAX)
        })
        .snaps(volume_snaps())
        .mark(1.0 / audio::MAX)
        .tone(tone(input, level))
        .label(dragging.then(|| percent(level)))
        .on_release(Message::AdjustDone)
        .on_icon_press(Some(Message::ToggleInputMute))
        // What the microphone hears, inside the rail; nothing while muted.
        .level((!input.muted).then_some(self.mic_level))
        .dimmed(input.muted)
        .details(
            Some(Message::ToggleCard(Card::Input)),
            self.card == Some(Card::Input),
        )
        .view()
    }

    fn devices_card<'a>(
        &'a self,
        devices: &'a [Device],
        default: &str,
        choose: fn(String) -> Message,
        empty: &'static str,
        with_hidden: bool,
    ) -> Element<'a, Message> {
        let mut rows: Vec<Element<'a, Message>> = vec![card_header(
            if with_hidden { "volume" } else { "microphone" },
            if with_hidden { "Output" } else { "Input" },
        )];
        rows.extend(devices.iter().map(|device| {
            let mut trailing: Vec<Element<'a, Message>> = Vec::new();
            if let Some(level) = self.sink_battery(&device.name) {
                trailing.push(super::text::sub(format!("{level} %")).into());
            }
            if device.name == default {
                trailing.push(check());
            }
            let trailing = (!trailing.is_empty()).then(|| {
                widget::row::with_children(trailing)
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .into()
            });
            super::menu_row(
                None,
                device.description.clone(),
                None,
                trailing,
                Some(choose(device.name.clone())),
                device.name == default,
            )
        }));
        if devices.is_empty() {
            rows.push(note(empty));
        }
        if with_hidden && !self.hidden_apps.is_empty() {
            rows.push(super::heading("Hidden apps"));
            for app in &self.hidden_apps {
                rows.push(row_item(
                    Some("eye"),
                    super::registry::name(app),
                    None,
                    Some(Message::UnhideApp(app.clone())),
                ));
            }
        }
        if !with_hidden && !self.privacy.microphone.is_empty() {
            rows.push(super::heading("In use by"));
            for app in &self.privacy.microphone {
                let info = super::registry::app_info(app);
                rows.push(super::Row::new(info.name).icon(info.icon).view());
            }
            rows.push(self.mute_microphone_row());
        }
        if with_hidden && !self.hidden_players.is_empty() {
            rows.push(super::heading("Hidden players"));
            for app in &self.hidden_players {
                rows.push(row_item(
                    Some("eye"),
                    super::registry::name(app),
                    None,
                    Some(Message::UnhidePlayer(app.clone())),
                ));
            }
        }
        rows.push(rule());
        rows.push(row_item(
            Some("settings"),
            "Sound settings".to_owned(),
            None,
            Some(Message::Settings(Some("sound"))),
        ));
        widget::column::with_children(rows).into()
    }

    /// An app's sound: its icon and name, and its own level under them.
    fn stream_row<'a>(&'a self, stream: &'a audio::Stream) -> Element<'a, Message> {
        let (level, dragging) = match &self.adjusting {
            Some(Adjusting::Stream(app, level)) if *app == stream.app => (*level, true),
            _ => (stream.level, false),
        };
        let info = super::registry::app_info(&stream.app);
        let app_icon: Element<'a, Message> = super::registry::glyph(&stream.app, 20);
        let indices = stream.indices.clone();
        let app = stream.app.clone();
        let idle = stream.corked || stream.indices.is_empty();
        // Muted: a small muted badge over the app's icon.
        let app_icon: Element<'a, Message> = if stream.muted {
            cosmic::iced::widget::stack([
                widget::container(app_icon)
                    .center(Length::Fill)
                    .class(inherit())
                    .into(),
                widget::container(
                    widget::container(icon("volume-off", 10))
                        .center(Length::Fixed(16.0))
                        .class(badge_class()),
                )
                .align_right(Length::Fill)
                .align_bottom(Length::Fill)
                .into(),
            ])
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            app_icon
        };
        let mute = widget::button::custom(
            widget::container(app_icon)
                .center(Length::Fill)
                .class(inherit()),
        )
        .width(Length::Fixed(super::ROUND))
        .height(Length::Fixed(super::ROUND))
        .padding(0)
        .class(theme::Button::Icon)
        .on_press_maybe(
            (!stream.indices.is_empty()).then_some(Message::StreamIcon(stream.app.clone())),
        );
        // A right click on the icon keeps the app out of the mixer.
        let mute = widget::mouse_area(mute).on_right_press(Message::OpenMenu(
            crate::app::MenuFor::App(stream.app.clone()),
        ));
        let slider = widget::slider(0.0..=1.0, (level / audio::MAX).clamp(0.0, 1.0), move |v| {
            Message::SetStream(app.clone(), indices.clone(), v * audio::MAX)
        })
        .step(0.01_f32)
        .on_release(Message::AdjustDone)
        .width(Length::Fill)
        .class(super::slider_class(if idle || stream.muted {
            super::Tone::Muted
        } else {
            super::Tone::Normal
        }));
        // The level in a bubble over the knob while it's dragged.
        let label: Option<Element<'a, Message>> =
            dragging.then(|| super::text::sized_bold(percent(level), 12.0).into());
        // What it's playing now, inside the rail, as the output's meter shows.
        let meter = (!idle && !stream.muted).then(|| {
            self.stream_levels
                .iter()
                .find(|(a, _)| *a == stream.app)
                .map_or(0.0, |(_, l)| *l)
        });
        let slider: Element<'a, Message> = super::mark::Marked::new(slider, level / audio::MAX)
            .label(label)
            .level(meter)
            .into();
        let name = super::text::small(info.name)
            .ellipsize(one_line())
            .class(theme::Text::Custom(|_| cosmic::iced::widget::text::Style {
                color: Some(super::palette::rgba(232, 236, 246, 0.75)),
                ..Default::default()
            }));
        let name = if idle {
            name.class(theme::Text::Custom(super::dimmed))
        } else {
            name
        };
        // Where it plays, behind a small arrow: another output can be picked.
        let open = self.stream_card.as_ref() == Some(&stream.app);
        let route: Element<'a, Message> = round_button(
            super::chevron(open),
            (!stream.indices.is_empty()).then_some(Message::ToggleStreamCard(stream.app.clone())),
            false,
        );
        widget::row::with_children(vec![
            mute.into(),
            widget::column::with_children(vec![name.into(), slider.into()])
                .spacing(2)
                .width(Length::Fill)
                .into(),
            route,
        ])
        .spacing(14)
        .align_y(Alignment::Center)
        .into()
    }

    /// The outputs an app's sound can go to, the one it's on marked.
    fn stream_outputs<'a>(&'a self, stream: &'a audio::Stream) -> Element<'a, Message> {
        let Some(audio) = &self.audio else {
            return note("No sound");
        };
        let mut rows: Vec<Element<'a, Message>> = vec![card_header("volume", "Play through")];
        for sink in &audio.sinks {
            let current = stream.sink.as_deref() == Some(sink.name.as_str());
            rows.push(super::menu_row(
                None,
                sink.description.clone(),
                None,
                current.then(check),
                (!current).then(|| Message::MoveStream(stream.indices.clone(), sink.name.clone())),
                current,
            ));
        }
        widget::column::with_children(rows).into()
    }

    /// A player: its cover blurred behind, the cover itself with the
    /// player's badge, what's playing, the controls, and the seek bar.
    fn media_card<'a>(&'a self, media: &'a Media) -> Element<'a, Message> {
        let spacing = theme::spacing();
        let tint = media.color;
        let on = on_color(tint);
        let flat = |name: &str, message: Option<Message>| -> Element<'a, Message> {
            widget::button::custom(
                widget::container(icon(name, 16))
                    .center(Length::Fill)
                    .class(inherit()),
            )
            .width(Length::Fixed(30.0))
            .height(Length::Fixed(30.0))
            .padding(0)
            .class(flat_control_class(tint))
            .on_press_maybe(message)
            .into()
        };
        let cover: Element<'a, Message> = match &media.art {
            Some(path) => widget::image(widget::image::Handle::from_path(path))
                .width(Length::Fixed(58.0))
                .height(Length::Fixed(58.0))
                .border_radius([10.0; 4])
                .into(),
            None => widget::container(icon("music", 24))
                .center(Length::Fixed(58.0))
                .class(art_class())
                .into(),
        };
        // The player's own icon, small over the cover's corner.
        let cover: Element<'a, Message> = match Some(&media.app) {
            Some(name) => cosmic::iced::widget::stack([
                cover,
                widget::container(
                    widget::container(player_badge(name))
                        .center(Length::Fixed(20.0))
                        .class(badge_plain_class()),
                )
                .align_right(Length::Fill)
                .align_bottom(Length::Fill)
                .into(),
            ])
            .width(Length::Fixed(63.0))
            .height(Length::Fixed(63.0))
            .into(),
            None => cover,
        };
        let mut byline = Vec::new();
        if !media.artist.is_empty() {
            byline.push(media.artist.clone());
        }
        if !media.album.is_empty() && media.album != media.title {
            byline.push(media.album.clone());
        }
        byline.push(super::registry::name(&media.app));
        let byline = byline.join(" · ");
        let title = media.title.clone();
        let text = widget::column::with_children(vec![
            Marquee::new(move |short| {
                let text = super::text::big(title.clone());
                if short {
                    text.ellipsize(one_line()).into()
                } else {
                    text.into()
                }
            })
            .into(),
            Marquee::new(move |short| {
                let text = super::text::sub(byline.clone());
                if short {
                    text.ellipsize(one_line()).into()
                } else {
                    text.into()
                }
            })
            .into(),
        ])
        .width(Length::Fill);
        let service = media.service.clone();
        let raise = media
            .can_raise
            .then(|| Message::RaisePlayer(service.clone()));
        let (cover, text): (Element<'a, Message>, Element<'a, Message>) = match raise {
            Some(raise) => (
                widget::mouse_area(cover)
                    .on_press(raise.clone())
                    .interaction(cosmic::iced::mouse::Interaction::Pointer)
                    .into(),
                widget::mouse_area(text).on_press(raise).into(),
            ),
            None => (cover, text.into()),
        };
        let play = widget::button::custom(
            widget::container(icon(
                if media.playing {
                    "media-playback-pause-symbolic"
                } else {
                    "media-playback-start-symbolic"
                },
                18,
            ))
            .center(Length::Fill)
            .class(inherit()),
        )
        .width(Length::Fixed(46.0))
        .height(Length::Fixed(46.0))
        .padding(0)
        .class(play_class(tint))
        .on_press(Message::Media(service.clone(), Control::PlayPause));
        let row = widget::row::with_children(vec![
            cover,
            text,
            flat(
                "media-skip-backward-symbolic",
                media
                    .can_previous
                    .then(|| Message::Media(service.clone(), Control::Previous)),
            ),
            play.into(),
            flat(
                "media-skip-forward-symbolic",
                media
                    .can_next
                    .then(|| Message::Media(service.clone(), Control::Next)),
            ),
        ])
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center);
        let mut column = widget::column::with_capacity(3)
            .spacing(spacing.space_xxs)
            .push(row);
        if media.seekable() {
            let position = match &self.seeking {
                Some((s, position)) if *s == media.service => *position,
                _ => media.position_now() as f32,
            };
            let length = media.length.max(1) as f32;
            let service = media.service.clone();
            let seeking = self
                .seeking
                .as_ref()
                .is_some_and(|(s, _)| *s == media.service);
            let slider = widget::slider(0.0..=length, position.min(length), move |p| {
                Message::Seek(service.clone(), p)
            })
            .on_release(Message::SeekDone(media.service.clone()))
            .class(super::slider_class(super::Tone::Color(on)))
            .height(20.0)
            .width(Length::Fill);
            // The time in a bubble over the knob while it's dragged.
            let label: Option<Element<'a, Message>> =
                seeking.then(|| super::text::sized_bold(media::time(position as i64), 12.0).into());
            let slider: Element<'a, Message> = super::mark::Marked::new(slider, position / length)
                .label(label)
                .into();
            let mut seek_row = widget::row::with_capacity(5)
                .spacing(spacing.space_xs)
                .align_y(Alignment::Center);
            let browser = matches!(
                super::registry::app_info(&media.app).icon,
                "brand-chrome" | "brand-firefox"
            );
            if let (Some(shuffle), false) = (media.shuffle, browser) {
                seek_row = seek_row.push(small_toggle(
                    "media-playlist-shuffle-symbolic",
                    shuffle,
                    Message::SetShuffle(media.service.clone(), !shuffle),
                    tint,
                ));
            }
            seek_row = seek_row
                .push(super::text::sub(media::time(position as i64)))
                .push(slider)
                .push(super::text::sub(media::time(media.length)));
            if let (Some(status), false) = (&media.loop_status, browser) {
                let (icon_name, on, next) = match status.as_str() {
                    "Track" => ("media-playlist-repeat-song-symbolic", true, "None"),
                    "Playlist" => ("media-playlist-repeat-symbolic", true, "Track"),
                    _ => ("media-playlist-repeat-symbolic", false, "Playlist"),
                };
                seek_row = seek_row.push(small_toggle(
                    icon_name,
                    on,
                    Message::SetLoop(media.service.clone(), next.to_owned()),
                    tint,
                ));
            }
            column = column.push(seek_row);
        }
        // With a cover behind, the content itself is see-through: the wash
        // paints the fade. Without one, it paints its own color.
        let content = widget::container(column)
            .width(Length::Fill)
            .padding([spacing.space_xs, spacing.space_s])
            .class(if media.backdrop.is_some() {
                text_only_class(tint)
            } else {
                media_class(tint)
            });
        // The cover, blurred, behind it all, under a dark wash so the text reads.
        match &media.backdrop {
            Some(path) => super::behind::Behind::new(
                widget::image(widget::image::Handle::from_path(path))
                    .content_fit(cosmic::iced::ContentFit::Fill)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .border_radius([CARD_RADIUS; 4]),
                widget::container(content).width(Length::Fill),
            )
            .into(),
            None => content.into(),
        }
    }
}

/// What reads on a player's card: white, the backdrop being darkened.
fn on_color(_tint: Option<[u8; 3]>) -> Color {
    Color::WHITE
}

/// No background, just the colors text and icons take on the card.
fn text_only_class(tint: Option<[u8; 3]>) -> theme::Container<'static> {
    theme::Container::custom(move |theme| {
        let (_, on) = media_colors(tint, theme);
        widget::container::Style {
            text_color: Some(on),
            icon_color: Some(on),
            ..Default::default()
        }
    })
}

/// The play button: solid in what reads on the card, the icon in the card's color.
fn play_class(tint: Option<[u8; 3]>) -> theme::Button {
    let style = move |hovered: bool, _theme: &cosmic::Theme| {
        let _ = tint;
        let background = if hovered {
            super::palette::TEXT
        } else {
            super::palette::KNOB
        };
        widget::button::Style {
            background: Some(Background::Color(background)),
            text_color: Some(super::palette::ON_ACCENT),
            icon_color: Some(super::palette::ON_ACCENT),
            border_radius: 23.0.into(),
            ..widget::button::Style::new()
        }
    };
    theme::Button::Custom {
        active: Box::new(move |_, theme| style(false, theme)),
        disabled: Box::new(move |theme| style(false, theme)),
        hovered: Box::new(move |_, theme| style(true, theme)),
        pressed: Box::new(move |_, theme| style(true, theme)),
    }
}

/// A player's own mark on its cover's corner: its brand, in its color.
fn player_badge<'a>(app: &str) -> Element<'a, Message> {
    super::registry::glyph(app, 13)
}

/// Previous and next: bare icons, a shade under the pointer.
fn flat_control_class(tint: Option<[u8; 3]>) -> theme::Button {
    let style = move |hovered: bool, theme: &cosmic::Theme| {
        let (_, on) = media_colors(tint, theme);
        let mut shade = on;
        shade.a = if hovered { 0.18 } else { 0.0 };
        widget::button::Style {
            background: Some(Background::Color(shade)),
            text_color: Some(on),
            icon_color: Some(on),
            border_radius: 16.0.into(),
            ..widget::button::Style::new()
        }
    };
    theme::Button::Custom {
        active: Box::new(move |_, theme| style(false, theme)),
        disabled: Box::new(move |theme| {
            let mut s = style(false, theme);
            s.icon_color = s.icon_color.map(|mut c| {
                c.a = 0.35;
                c
            });
            s
        }),
        hovered: Box::new(move |_, theme| style(true, theme)),
        pressed: Box::new(move |_, theme| style(true, theme)),
    }
}

/// A small toggle on a player's card: shuffle, repeat; lit when on.
fn small_toggle<'a>(
    icon_name: &str,
    on: bool,
    message: Message,
    tint: Option<[u8; 3]>,
) -> Element<'a, Message> {
    let style = move |hovered: bool, theme: &cosmic::Theme| {
        let (color, text) = media_colors(tint, theme);
        let mut background = text;
        background.a = if on {
            0.9
        } else if hovered {
            0.18
        } else {
            0.0
        };
        let mut dim = text;
        dim.a = 0.6;
        widget::button::Style {
            background: Some(Background::Color(background)),
            icon_color: Some(if on { color } else { dim }),
            text_color: Some(if on { color } else { dim }),
            border_radius: 8.0.into(),
            ..widget::button::Style::new()
        }
    };
    widget::button::custom(
        widget::container(icon(icon_name, 12))
            .center(Length::Fill)
            .class(inherit()),
    )
    .width(Length::Fixed(26.0))
    .height(Length::Fixed(26.0))
    .padding(0)
    .class(theme::Button::Custom {
        active: Box::new(move |_, theme| style(false, theme)),
        disabled: Box::new(move |theme| style(false, theme)),
        hovered: Box::new(move |_, theme| style(true, theme)),
        pressed: Box::new(move |_, theme| style(true, theme)),
    })
    .on_press(message)
    .into()
}

/// The small round plate for a player's icon over its cover.
fn badge_plain_class() -> theme::Container<'static> {
    theme::Container::custom(|_| widget::container::Style {
        background: Some(Background::Color(super::palette::BADGE_PLATE)),
        border: cosmic::iced::Border {
            radius: 10.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
}

fn one_line() -> cosmic::iced::core::text::Ellipsize {
    cosmic::iced::core::text::Ellipsize::End(cosmic::iced::core::text::EllipsizeHeightLimit::Lines(
        1,
    ))
}

/// The cover's color as a card's: its hue, made vivid, and deep enough to
/// read on.
fn tinted(tint: [u8; 3], dark: bool) -> Color {
    let [r, g, b] = tint.map(|c| f32::from(c) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    // Stretch the chroma: a muted cover still gives a colored card.
    let spread = (max - min).max(0.05);
    let stretch = (0.45 / spread).clamp(1.0, 4.0);
    let mid = (max + min) / 2.0;
    let vivid = |c: f32| (mid + (c - mid) * stretch).clamp(0.0, 1.0);
    let (r, g, b) = (vivid(r), vivid(g), vivid(b));
    if dark {
        // Bring the brightest channel to a deep but clear level.
        let top = r.max(g).max(b).max(0.01);
        let scale = 0.5 / top;
        Color::from_rgb(r * scale, g * scale, b * scale)
    } else {
        Color::from_rgb(0.5 + r * 0.5, 0.5 + g * 0.5, 0.5 + b * 0.5)
    }
}

/// The colors on a player's card: its background, and what reads on it.
fn media_colors(tint: Option<[u8; 3]>, _theme: &cosmic::Theme) -> (Color, Color) {
    let background = tint.map_or(super::palette::MEDIA, |tint| tinted(tint, true));
    (background, super::palette::on(background))
}

/// The little muted sign over a muted app's icon: red, round.
fn badge_class() -> theme::Container<'static> {
    theme::Container::custom(|_| widget::container::Style {
        background: Some(Background::Color(super::palette::MUTE_BADGE)),
        icon_color: Some(Color::WHITE),
        text_color: Some(Color::WHITE),
        border: cosmic::iced::Border {
            radius: 8.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
}

/// A player's card: a gradient from its cover's color, fading toward the
/// pane; plain when there's no cover.
fn media_class(tint: Option<[u8; 3]>) -> theme::Container<'static> {
    theme::Container::custom(move |theme| {
        let (color, on) = media_colors(tint, theme);
        let background = match tint {
            Some(_) => {
                let base: Color = super::palette::MEDIA;
                let far = Color::from_rgb(
                    color.r * 0.45 + base.r * 0.55,
                    color.g * 0.45 + base.g * 0.55,
                    color.b * 0.45 + base.b * 0.55,
                );
                Background::Gradient(
                    gradient::Linear::new(cosmic::iced::Radians(1.2))
                        .add_stop(0.0, color)
                        .add_stop(1.0, far)
                        .into(),
                )
            }
            None => Background::Color(color),
        };
        widget::container::Style {
            background: Some(background),
            text_color: Some(on),
            icon_color: Some(on),
            border: cosmic::iced::Border {
                radius: 16.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
}

/// Where a cover would be when there's none: a darker square.
fn art_class() -> theme::Container<'static> {
    theme::Container::custom(|_| widget::container::Style {
        background: Some(Background::Color(super::palette::SURFACE)),
        border: cosmic::iced::Border {
            radius: 8.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
}
