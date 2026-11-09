// SPDX-License-Identifier: GPL-3.0-only

//! A pinentry in the panel's style: gpg-agent talks Assuan to it over
//! stdin and stdout, and each question it asks is a card of glass over the
//! screen with the passphrase field, as the logout dialog is drawn.
//!
//! Installed as `pinentry`; gpg-agent is told `pinentry-program` by
//! home-manager (`services.gpg-agent.pinentry`).

use std::{
    io::{BufRead, Write as _},
    sync::{Mutex, OnceLock, mpsc},
};

use cosmic::{
    Element, app,
    iced::{
        self, Alignment, Background, Color, Length, Limits, Subscription, event,
        platform_specific::shell::commands::layer_surface::{Anchor, KeyboardInteractivity, destroy_layer_surface, get_layer_surface},
        runtime::platform_specific::wayland::layer_surface::SctkLayerSurfaceSettings,
        window,
    },
    theme, widget,
};
use futures::{SinkExt, StreamExt};
use cabin::ui::{glass, icon, palette, text};

const WIDTH: f32 = 520.0;

static INPUT: std::sync::LazyLock<widget::Id> = std::sync::LazyLock::new(|| widget::Id::new("pinentry-input"));

/// What gpg-agent asks: a passphrase, a yes or no, or just to be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Pin,
    Confirm,
    Message,
}

/// One question, with the words the agent set for it.
#[derive(Clone, Debug)]
struct Request {
    kind: Kind,
    title: String,
    description: String,
    prompt: String,
    error: String,
    ok: String,
    cancel: String,
    /// A third button that says no without cancelling, when the agent asks.
    not_ok: Option<String>,
    /// Shown twice to be typed twice (SETREPEAT).
    repeat: Option<String>,
    reply: mpsc::Sender<Reply>,
}

#[derive(Clone, Debug)]
enum Reply {
    Pin(String),
    Confirmed,
    NotConfirmed,
    Cancelled,
}

/// From the Assuan thread to the window.
#[derive(Clone, Debug)]
enum Event {
    Ask(Request),
    Quit,
}

static EVENTS: OnceLock<Mutex<Option<futures::channel::mpsc::UnboundedReceiver<Event>>>> = OnceLock::new();

// ---- the Assuan side

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'%' => out.push_str("%25"),
            b'\r' => out.push_str("%0D"),
            b'\n' => out.push_str("%0A"),
            _ => out.push(byte as char),
        }
    }
    out
}

/// A button's label as the agent gives it, without the mnemonic underscore.
fn label(s: &str) -> String {
    s.replace('_', "")
}

/// The conversation with gpg-agent, line by line, until BYE. Questions go
/// to the window and the answers come back before the reply is written.
fn serve(mut to_window: futures::channel::mpsc::UnboundedSender<Event>) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut say = |line: &str| {
        let _ = writeln!(stdout, "{line}");
        let _ = stdout.flush();
    };
    say("OK Pleased to meet you");
    let (mut title, mut description, mut prompt, mut error, mut ok, mut cancel) = (String::new(), String::new(), String::new(), String::new(), String::new(), String::new());
    let (mut not_ok, mut repeat): (Option<String>, Option<String>) = (None, None);
    let mut ask = |kind: Kind, title: &str, description: &str, prompt: &str, error: &str, ok: &str, cancel: &str, not_ok: &Option<String>, repeat: &Option<String>| -> Option<Reply> {
        let (reply_tx, reply_rx) = mpsc::channel();
        let request = Request {
            kind,
            title: if title.is_empty() { "Passphrase".to_owned() } else { title.to_owned() },
            description: description.to_owned(),
            prompt: if prompt.is_empty() { "Passphrase".to_owned() } else { prompt.to_owned() },
            error: error.to_owned(),
            ok: if ok.is_empty() { "OK".to_owned() } else { label(ok) },
            cancel: if cancel.is_empty() { "Cancel".to_owned() } else { label(cancel) },
            not_ok: not_ok.as_deref().map(label),
            repeat: repeat.clone(),
            reply: reply_tx,
        };
        futures::executor::block_on(to_window.send(Event::Ask(request))).ok()?;
        reply_rx.recv().ok()
    };
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim_end();
        let (command, argument) = match line.split_once(' ') {
            Some((c, a)) => (c.to_uppercase(), a.trim()),
            None => (line.to_uppercase(), ""),
        };
        let argument = percent_decode(argument);
        match command.as_str() {
            "" | "NOP" | "OPTION" | "SETTIMEOUT" | "SETQUALITYBAR" | "SETQUALITYBAR_TT" | "SETGENPIN" | "SETGENPIN_TT" | "SETKEYINFO" | "CLEARPASSPHRASE" => say("OK"),
            "RESET" => {
                title.clear();
                description.clear();
                prompt.clear();
                error.clear();
                ok.clear();
                cancel.clear();
                not_ok = None;
                repeat = None;
                say("OK");
            }
            "SETDESC" => {
                description = argument;
                say("OK");
            }
            "SETPROMPT" => {
                prompt = argument;
                say("OK");
            }
            "SETTITLE" => {
                title = argument;
                say("OK");
            }
            "SETERROR" => {
                error = argument;
                say("OK");
            }
            "SETOK" => {
                ok = argument;
                say("OK");
            }
            "SETCANCEL" => {
                cancel = argument;
                say("OK");
            }
            "SETNOTOK" => {
                not_ok = Some(argument);
                say("OK");
            }
            "SETREPEAT" => {
                repeat = Some(if argument.is_empty() { "Repeat".to_owned() } else { argument });
                say("OK");
            }
            "SETREPEATERROR" => say("OK"),
            "GETINFO" => {
                match argument.as_str() {
                    "pid" => say(&format!("D {}", std::process::id())),
                    "version" => say("D 1.3.1"),
                    "flavor" => say("D cosmic"),
                    "ttyinfo" => say("D - - -"),
                    _ => {}
                }
                say("OK");
            }
            "GETPIN" => {
                match ask(Kind::Pin, &title, &description, &prompt, &error, &ok, &cancel, &not_ok, &repeat) {
                    Some(Reply::Pin(pin)) => {
                        if repeat.is_some() {
                            say("S PIN_REPEATED");
                        }
                        say(&format!("D {}", percent_encode(&pin)));
                        say("OK");
                    }
                    _ => say("ERR 83886179 Operation cancelled <Pinentry>"),
                }
                error.clear();
                repeat = None;
            }
            "CONFIRM" => match ask(Kind::Confirm, &title, &description, &prompt, &error, &ok, &cancel, &not_ok, &None) {
                Some(Reply::Confirmed) => say("OK"),
                Some(Reply::NotConfirmed) => say("ERR 83886194 Not confirmed <Pinentry>"),
                _ => say("ERR 83886179 Operation cancelled <Pinentry>"),
            },
            "MESSAGE" => {
                let _ = ask(Kind::Message, &title, &description, &prompt, &error, &ok, &cancel, &None, &None);
                say("OK");
            }
            "HELP" => say("OK"),
            "BYE" => {
                say("OK closing connection");
                break;
            }
            _ => say("ERR 536871187 Unknown IPC command <Pinentry>"),
        }
    }
    let _ = futures::executor::block_on(to_window.send(Event::Quit));
}

// ---- the window

#[derive(Clone, Debug)]
enum Message {
    Event(Event),
    Typed(String),
    TypedAgain(String),
    ToggleShown,
    Submit,
    NotOk,
    Cancel,
    Done,
}

struct Pinentry {
    core: app::Core,
    surface: Option<window::Id>,
    request: Option<Request>,
    pin: String,
    again: String,
    shown: bool,
    /// Our own word of warning: the two passphrases differ.
    mismatch: bool,
}

impl Pinentry {
    fn answer(&mut self, reply: Reply) -> app::Task<Message> {
        if let Some(request) = self.request.take() {
            let _ = request.reply.send(reply);
        }
        self.pin.clear();
        self.again.clear();
        self.mismatch = false;
        match self.surface.take() {
            Some(id) => destroy_layer_surface(id),
            None => app::Task::none(),
        }
    }
}

impl cosmic::Application for Pinentry {
    type Executor = iced::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "dev.troy.Cabin.Pinentry";

    fn core(&self) -> &app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut app::Core {
        &mut self.core
    }

    fn init(mut core: app::Core, _: ()) -> (Self, app::Task<Message>) {
        core.set_app_type(cosmic::core::AppType::System);
        let font = match text::file().and_then(|path| std::fs::read(path).ok()) {
            Some(bytes) => iced::font::load(std::borrow::Cow::Owned(bytes)).map(|_| cosmic::action::app(Message::Done)),
            None => app::Task::none(),
        };
        (Self { core, surface: None, request: None, pin: String::new(), again: String::new(), shown: false, mismatch: false }, font)
    }

    fn style(&self) -> Option<iced::theme::Style> {
        Some(iced::theme::Style { background_color: Color::TRANSPARENT, text_color: palette::TEXT, icon_color: palette::TEXT })
    }

    fn update(&mut self, message: Message) -> app::Task<Message> {
        match message {
            Message::Done => app::Task::none(),
            Message::Event(Event::Quit) => std::process::exit(0),
            Message::Event(Event::Ask(request)) => {
                self.request = Some(request);
                let id = window::Id::unique();
                self.surface = Some(id);
                let open = get_layer_surface(SctkLayerSurfaceSettings {
                    id,
                    keyboard_interactivity: KeyboardInteractivity::Exclusive,
                    anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                    namespace: "pinentry".into(),
                    size: None,
                    size_limits: Limits::NONE.min_width(1.0).min_height(1.0),
                    ..Default::default()
                });
                app::Task::batch([open, widget::text_input::focus(INPUT.clone())])
            }
            Message::Typed(pin) => {
                self.pin = pin;
                self.mismatch = false;
                app::Task::none()
            }
            Message::TypedAgain(again) => {
                self.again = again;
                self.mismatch = false;
                app::Task::none()
            }
            Message::ToggleShown => {
                self.shown = !self.shown;
                app::Task::none()
            }
            Message::Submit => match self.request.as_ref().map(|r| r.kind) {
                Some(Kind::Pin) => {
                    if self.request.as_ref().is_some_and(|r| r.repeat.is_some()) && self.again != self.pin {
                        self.mismatch = true;
                        return app::Task::none();
                    }
                    let pin = std::mem::take(&mut self.pin);
                    self.answer(Reply::Pin(pin))
                }
                Some(Kind::Confirm) | Some(Kind::Message) => self.answer(Reply::Confirmed),
                None => app::Task::none(),
            },
            Message::NotOk => self.answer(Reply::NotConfirmed),
            Message::Cancel => self.answer(Reply::Cancelled),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        widget::text("").into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        let (Some(request), true) = (&self.request, self.surface == Some(id)) else {
            return widget::text("").into();
        };
        let chip = widget::container(icon(if request.kind == Kind::Pin { "key" } else { "info-circle" }, 26)).center(Length::Fixed(52.0)).class(theme::Container::custom(|_| widget::container::Style {
            background: Some(Background::Color(palette::ACCENT)),
            icon_color: Some(palette::ON_ACCENT),
            border: iced::Border { radius: 14.0.into(), ..Default::default() },
            ..Default::default()
        }));
        let mut words = widget::column::with_capacity(4).spacing(6).push(text::sized_bold(request.title.clone(), 20.0));
        if !request.description.is_empty() {
            words = words.push(text::sized(request.description.clone(), 13.5).class(theme::Text::Color(palette::rgba(232, 236, 246, 0.78))));
        }
        let warning = if self.mismatch {
            Some("The two passphrases differ.".to_owned())
        } else if request.error.is_empty() {
            None
        } else {
            Some(request.error.clone())
        };
        if let Some(warning) = warning {
            words = words.push(text::sized(warning, 13.0).class(theme::Text::Color(palette::OVER)));
        }
        let mut body = widget::column::with_capacity(3).spacing(14).push(widget::row::with_children(vec![chip.into(), words.width(Length::Fill).into()]).spacing(18).align_y(Alignment::Start));
        if request.kind == Kind::Pin {
            let field = widget::secure_input(request.prompt.clone(), self.pin.clone(), Some(Message::ToggleShown), !self.shown)
                .id(INPUT.clone())
                .on_input(Message::Typed)
                .on_submit(|_| Message::Submit);
            body = body.push(widget::container(field).padding([0, 0, 0, 70]));
            if let Some(repeat) = &request.repeat {
                let again = widget::secure_input(repeat.clone(), self.again.clone(), None::<Message>, !self.shown).on_input(Message::TypedAgain).on_submit(|_| Message::Submit);
                body = body.push(widget::container(again).padding([0, 0, 0, 70]));
            }
        }
        let mut buttons = widget::row::with_capacity(4).spacing(10).push(widget::space::horizontal());
        if request.kind != Kind::Message {
            buttons = buttons.push(dialog_button(request.cancel.clone(), false, Message::Cancel));
        }
        if let (Kind::Confirm, Some(not_ok)) = (request.kind, &request.not_ok) {
            buttons = buttons.push(dialog_button(not_ok.clone(), false, Message::NotOk));
        }
        buttons = buttons.push(dialog_button(request.ok.clone(), true, Message::Submit));
        body = body.push(widget::container(buttons).padding([8, 0, 0, 0]));
        let card = glass(body, [26.0, 26.0, 20.0, 26.0], Some(WIDTH));
        cosmic::iced::widget::stack([
            widget::mouse_area(widget::container(widget::space::horizontal()).width(Length::Fill).height(Length::Fill).class(theme::Container::custom(|_| widget::container::Style {
                background: Some(Background::Color(palette::rgba(5, 8, 18, 0.45))),
                ..Default::default()
            })))
            .on_press(Message::Cancel)
            .into(),
            widget::container(widget::mouse_area(card).on_press(Message::Done)).width(Length::Fill).align_x(Alignment::Center).padding([240, 0, 0, 0]).into(),
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            Subscription::run(events).map(Message::Event),
            event::listen_with(|event, _, _| match event {
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape), .. }) => Some(Message::Cancel),
                _ => None,
            }),
        ])
    }
}

/// The questions, as the Assuan thread hands them over.
fn events() -> impl futures::Stream<Item = Event> + Send {
    let receiver = EVENTS.get().and_then(|slot| slot.lock().ok()?.take());
    futures::stream::iter(receiver).flatten()
}

/// A dialog button: the pale chip, or the accent for the one that goes ahead.
fn dialog_button<'a>(label: String, primary: bool, message: Message) -> Element<'a, Message> {
    let style = move |hovered: bool| widget::button::Style {
        background: Some(Background::Color(match (primary, hovered) {
            (true, true) => palette::ACCENT_HOVER,
            (true, false) => palette::ACCENT,
            (false, true) => palette::rgba(200, 220, 255, 0.2),
            (false, false) => palette::rgba(200, 220, 255, 0.12),
        })),
        text_color: Some(if primary { palette::ON_ACCENT } else { palette::TEXT }),
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

fn main() -> iced::Result {
    cabin::prefer_integrated_gpu();
    let (to_window, from_assuan) = futures::channel::mpsc::unbounded();
    let _ = EVENTS.set(Mutex::new(Some(from_assuan)));
    std::thread::Builder::new().name("assuan".into()).spawn(move || serve(to_window)).expect("a thread");
    cosmic::app::run::<Pinentry>(app::Settings::default().no_main_window(true).exit_on_close(false), ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_codes() {
        assert_eq!(percent_decode("Please%0Aenter%25"), "Please\nenter%");
        assert_eq!(percent_encode("a%b\nc"), "a%25b%0Ac");
        assert_eq!(label("_Cancel"), "Cancel");
    }
}
