// SPDX-License-Identifier: GPL-3.0-only

//! A line of text that's cut short with an ellipsis when it's too long, and
//! rolls by like a ticker, round and round, while the pointer is over it.

use std::time::{Duration, Instant};

use cosmic::iced::{
    Event, Length, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Renderer as _, Shell, Widget, layout, mouse, renderer,
        widget::{Tree, tree},
    },
    window,
};

/// Pixels a second.
const SPEED: f32 = 40.0;
/// Between the end of the text and its start again.
const SPACE: f32 = 48.0;
/// Hovered this long before it starts to roll.
const DELAY: Duration = Duration::from_millis(500);

pub struct Marquee<'a, Message> {
    /// Cut short, for when it's still.
    short: cosmic::Element<'a, Message>,
    /// Whole, for the ticker.
    full: cosmic::Element<'a, Message>,
}

#[derive(Default)]
struct State {
    hovered_since: Option<Instant>,
}

impl<'a, Message> Marquee<'a, Message> {
    /// Two versions of the same text: `make(true)` cut short to one line,
    /// `make(false)` whole.
    pub fn new(make: impl Fn(bool) -> cosmic::Element<'a, Message>) -> Self {
        Self {
            short: make(true),
            full: make(false),
        }
    }
}

impl<Message> Widget<Message, cosmic::Theme, cosmic::Renderer> for Marquee<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.short), Tree::new(&self.full)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut [&mut self.short, &mut self.full]);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &cosmic::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let width = limits.max().width;
        let bounded = layout::Limits::new(Size::ZERO, Size::new(width, limits.max().height));
        let short = self
            .short
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &bounded);
        let unbounded =
            layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, limits.max().height));
        let full = self
            .full
            .as_widget_mut()
            .layout(&mut tree.children[1], renderer, &unbounded);
        let size = Size::new(width, short.size().height.max(full.size().height));
        layout::Node::with_children(size, vec![short, full])
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &cosmic::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let full = layout.children().nth(1).expect("the whole text");
        let overflow = full.bounds().width > bounds.width + 1.0;
        let hovered = cursor.is_over(bounds);
        match (hovered, state.hovered_since) {
            (true, None) => state.hovered_since = Some(Instant::now()),
            (false, Some(_)) => state.hovered_since = None,
            _ => {}
        }
        if overflow && hovered {
            if let Event::Window(window::Event::RedrawRequested(_)) = event {
                shell.request_redraw();
            }
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let mut children = layout.children();
        let short = children.next().expect("the short text");
        let full = children.next().expect("the whole text");
        let width = full.bounds().width;
        let state = tree.state.downcast_ref::<State>();
        let rolling = state
            .hovered_since
            .is_some_and(|since| since.elapsed() > DELAY)
            && width > bounds.width + 1.0;
        if !rolling {
            self.short.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                short,
                cursor,
                viewport,
            );
            return;
        }
        let since = state.hovered_since.expect("hovered");
        let period = width + SPACE;
        let shift = ((since.elapsed() - DELAY).as_secs_f32() * SPEED) % period;
        renderer.with_layer(bounds, |renderer| {
            for copy in [0.0, period] {
                let x = copy - shift;
                if x < bounds.width && x + width > 0.0 {
                    renderer.with_translation(Vector::new(x, 0.0), |renderer| {
                        self.full.as_widget().draw(
                            &tree.children[1],
                            renderer,
                            theme,
                            style,
                            full,
                            cursor,
                            viewport,
                        );
                    });
                }
            }
        });
    }
}

impl<'a, Message: 'a> From<Marquee<'a, Message>> for cosmic::Element<'a, Message> {
    fn from(marquee: Marquee<'a, Message>) -> Self {
        cosmic::Element::new(marquee)
    }
}
