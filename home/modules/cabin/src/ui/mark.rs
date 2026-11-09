// SPDX-License-Identifier: GPL-3.0-only

//! A slider with things drawn over it, in the same pass so nothing clips
//! them: a mark at a point of the rail (100 %), the rail from there to the
//! handle in red when the level is past it, and while dragging a bubble over
//! the handle with the level in it. The bubble rises above the slider's own
//! room, which a stacked layer would have cut off.

use cosmic::iced::{
    Background, Border, Color, Event, Length, Point, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree},
    },
};

/// The slider's handle, which its rail makes room for at both ends.
/// The knob as drawn (its radius is 9), and the room the slider leaves for
/// it at each end of the rail: iced lays the rail out for a 26 px handle
/// whatever the knob drawn, so the rail runs from 13 px in to 13 px short.
const HANDLE: f32 = 18.0;
const RAIL_INSET: f32 = 13.0;
const RAIL: f32 = 6.0;

pub struct Marked<'a, Message> {
    slider: cosmic::Element<'a, Message>,
    /// Where the mark is, 0 to 1, if any.
    at: Option<f32>,
    /// The level, 0 to 1.
    value: f32,
    /// What to say over the handle, while dragging.
    label: Option<cosmic::Element<'a, Message>>,
    /// Past the mark: the bubble goes red.
    red: bool,
    /// A live level, 0 to 1, shown inside the rail: what the microphone
    /// hears, what the speakers play.
    level: Option<f32>,
}

impl<'a, Message> Marked<'a, Message> {
    pub fn new(slider: impl Into<cosmic::Element<'a, Message>>, value: f32) -> Self {
        Self { slider: slider.into(), at: None, value: value.clamp(0.0, 1.0), label: None, red: false, level: None }
    }

    pub fn level(mut self, level: Option<f32>) -> Self {
        self.level = level.map(|l| l.clamp(0.0, 1.0));
        self
    }

    pub fn mark(mut self, at: Option<f32>) -> Self {
        self.at = at;
        self
    }

    pub fn label(mut self, label: Option<cosmic::Element<'a, Message>>) -> Self {
        self.label = label;
        self
    }

    pub fn red(mut self, red: bool) -> Self {
        self.red = red;
        self
    }
}

impl<Message> Widget<Message, cosmic::Theme, cosmic::Renderer> for Marked<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        let mut children = vec![Tree::new(&self.slider)];
        children.extend(self.label.iter().map(Tree::new));
        children
    }

    fn diff(&mut self, tree: &mut Tree) {
        let mut children: Vec<&mut cosmic::Element<'_, Message>> = vec![&mut self.slider];
        children.extend(self.label.iter_mut());
        tree.diff_children(&mut children);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &cosmic::Renderer, limits: &layout::Limits) -> layout::Node {
        let slider = self.slider.as_widget_mut().layout(&mut tree.children[0], renderer, limits);
        let size = slider.size();
        layout::Node::with_children(size, vec![slider])
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &cosmic::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Some(slider) = layout.children().next() {
            self.slider.as_widget_mut().update(&mut tree.children[0], event, slider, cursor, renderer, clipboard, shell, viewport);
        }
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &cosmic::Renderer) -> mouse::Interaction {
        layout.children().next().map_or(mouse::Interaction::None, |slider| self.slider.as_widget().mouse_interaction(&tree.children[0], slider, cursor, viewport, renderer))
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &cosmic::Renderer, operation: &mut dyn Operation) {
        if let Some(slider) = layout.children().next() {
            self.slider.as_widget_mut().operate(&mut tree.children[0], slider, renderer, operation);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &cosmic::Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, cosmic::Theme, cosmic::Renderer>> {
        // The bubble is an overlay: drawn over everything, clipped by nothing.
        let label = self.label.as_mut()?;
        let slider = layout.children().next()?.bounds();
        let (_, labels) = tree.children.split_at_mut(1);
        let bubble = Bubble { label, tree: labels.first_mut()?, slider: slider + translation, value: self.value, red: self.red };
        Some(overlay::Element::new(Box::new(bubble)))
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
        let mut children = layout.children();
        let Some(slider) = children.next() else {
            return;
        };
        self.slider.as_widget().draw(&tree.children[0], renderer, theme, style, slider, cursor, viewport);
        let bounds = slider.bounds();
        let rail_x = bounds.x + RAIL_INSET;
        let rail_width = (bounds.width - 2.0 * RAIL_INSET).max(1.0);
        let x = |v: f32| rail_x + rail_width * v.clamp(0.0, 1.0);
        let rail_y = bounds.y + bounds.height / 2.0;
        // The live level: a bar inside the rail, from within its rounded end,
        // around the knob rather than over it. Dark over the filled part of
        // the rail, the accent over the empty part, so it reads on both.
        if let Some(level) = self.level.filter(|l| *l > 0.005) {
            let start = rail_x;
            let end = x(level).max(start);
            let knob = x(self.value);
            let (gap_from, gap_to) = (knob - HANDLE / 2.0 - 1.5, knob + HANDLE / 2.0 + 1.5);
            let pieces = [
                (start, end.min(gap_from), Color { a: 0.6, ..super::palette::ON_ACCENT }),
                (start.max(gap_to), end, super::palette::ACCENT),
            ];
            for (from, to, color) in pieces {
                if to - from < 0.5 {
                    continue;
                }
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle { x: from, y: rail_y - 1.5, width: to - from, height: 3.0 },
                        border: Border { radius: 1.5.into(), ..Default::default() },
                        ..Default::default()
                    },
                    Background::Color(color),
                );
            }
        }
        if let Some(at) = self.at {
            // Past the mark, up to the handle: red.
            if self.value > at + 0.001 {
                let from = x(at);
                let to = (x(self.value) - HANDLE / 2.0 + 2.0).max(from);
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle { x: from, y: rail_y - RAIL / 2.0, width: to - from, height: RAIL },
                        border: Border { radius: 2.0.into(), ..Default::default() },
                        ..Default::default()
                    },
                    Background::Color(super::palette::OVER),
                );
            }
            let tick: Color = super::palette::alpha(super::palette::TEXT, 0.6);
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle { x: x(at) - 1.0, y: rail_y - 8.0, width: 2.0, height: 16.0 },
                    border: Border { radius: 1.0.into(), ..Default::default() },
                    ..Default::default()
                },
                Background::Color(tick),
            );
        }
    }
}

impl<'a, Message: 'a> From<Marked<'a, Message>> for cosmic::Element<'a, Message> {
    fn from(marked: Marked<'a, Message>) -> Self {
        cosmic::Element::new(marked)
    }
}

/// The level in a bubble over the handle, as an overlay of the popup.
struct Bubble<'a, 'b, Message> {
    label: &'b mut cosmic::Element<'a, Message>,
    tree: &'b mut Tree,
    /// The slider's place in the popup.
    slider: Rectangle,
    value: f32,
    red: bool,
}

const PAD: f32 = 9.0;

impl<Message> overlay::Overlay<Message, cosmic::Theme, cosmic::Renderer> for Bubble<'_, '_, Message> {
    fn layout(&mut self, renderer: &cosmic::Renderer, _bounds: Size) -> layout::Node {
        let node = self.label.as_widget_mut().layout(self.tree, renderer, &layout::Limits::new(Size::ZERO, Size::new(200.0, 40.0)));
        let text = node.size();
        // Over the handle, a little above the rail; kept within the slider's width.
        let rail_x = self.slider.x + RAIL_INSET;
        let rail_width = (self.slider.width - 2.0 * RAIL_INSET).max(1.0);
        let x = (rail_x + rail_width * self.value - text.width / 2.0).clamp(self.slider.x, (self.slider.x + self.slider.width - text.width).max(self.slider.x));
        let y = self.slider.y + self.slider.height / 2.0 - HANDLE / 2.0 - text.height - 14.0;
        let bubble = Size::new(text.width + 2.0 * PAD, text.height + 8.0);
        layout::Node::with_children(bubble, vec![node.move_to(Point::new(PAD, 4.0))]).move_to(Point::new(x - PAD, y - 4.0))
    }

    fn draw(&self, renderer: &mut cosmic::Renderer, theme: &cosmic::Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor) {
        let bubble = layout.bounds();
        let (fill, on) = if self.red { (super::palette::OVER, super::palette::ON_OVER) } else { (super::palette::KNOB, super::palette::ON_ACCENT) };
        renderer.fill_quad(
            renderer::Quad {
                bounds: bubble,
                border: Border { radius: 8.0.into(), ..Default::default() },
                shadow: cosmic::iced::Shadow { color: Color { a: 0.35, ..Color::BLACK }, offset: cosmic::iced::Vector::new(0.0, 2.0), blur_radius: 8.0 },
                ..Default::default()
            },
            Background::Color(fill),
        );
        if let Some(node) = layout.children().next() {
            let text_style = renderer::Style { text_color: on, icon_color: on, ..*style };
            self.label.as_widget().draw(self.tree, renderer, theme, &text_style, node, cursor, &bubble);
        }
    }
}
