// SPDX-License-Identifier: GPL-3.0-only

//! The overlay is the whole work area under the panel, see-through, with
//! the panels drawn in it where they are. This lays children at fixed
//! points, later ones over earlier ones, tells when the pointer presses on
//! one (to bring it forward) or on none of them.

use cosmic::iced::{
    Event, Length, Point, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree},
    },
};

pub struct Placed<'a, Message> {
    children: Vec<cosmic::Element<'a, Message>>,
    /// Each child's top left corner and width; its height is its own.
    places: Vec<(Point, f32)>,
    /// Sent when the left button goes down on this child, before the child
    /// sees the press.
    pressing: Vec<Option<Message>>,
    /// Each child's place in the stack, higher over lower. Kept apart from
    /// the children's order, which must not change when one comes to the
    /// front: their state (a button half-pressed) goes by position.
    z: Vec<u32>,
    size: Size,
    on_press_outside: Option<Message>,
    /// Where the children ended up last time, and whom to tell when that
    /// changes: the compositor frosts just those rectangles.
    report: Option<(Vec<Rectangle>, Box<dyn Fn(Vec<Rectangle>) -> Message + 'a>)>,
}

impl<'a, Message> Placed<'a, Message> {
    pub fn new(size: Size) -> Self {
        Self {
            children: Vec::new(),
            places: Vec::new(),
            pressing: Vec::new(),
            z: Vec::new(),
            size,
            on_press_outside: None,
            report: None,
        }
    }

    /// A child over all the others.
    pub fn push(
        mut self,
        child: impl Into<cosmic::Element<'a, Message>>,
        at: Point,
        width: f32,
    ) -> Self {
        self.children.push(child.into());
        self.places.push((at, width));
        self.pressing.push(None);
        self.z.push(u32::MAX);
        self
    }

    /// A child at this height in the stack, and what to send when it's pressed.
    pub fn push_stacked(
        mut self,
        child: impl Into<cosmic::Element<'a, Message>>,
        at: Point,
        width: f32,
        z: u32,
        message: Message,
    ) -> Self {
        self.children.push(child.into());
        self.places.push((at, width));
        self.pressing.push(Some(message));
        self.z.push(z);
        self
    }

    /// The children's indices, bottom of the stack first.
    fn stacked(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.children.len()).collect();
        order.sort_by_key(|i| self.z[*i]);
        order
    }

    /// Sent when the left button goes down on none of the children.
    pub fn on_press_outside(mut self, message: Message) -> Self {
        self.on_press_outside = Some(message);
        self
    }

    /// Sent with the children's rectangles whenever they differ from `known`.
    pub fn on_bounds(
        mut self,
        known: Vec<Rectangle>,
        message: impl Fn(Vec<Rectangle>) -> Message + 'a,
    ) -> Self {
        self.report = Some((known, Box::new(message)));
        self
    }
}

impl<Message: Clone> Widget<Message, cosmic::Theme, cosmic::Renderer> for Placed<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.children);
    }

    fn size(&self) -> Size<Length> {
        Size::new(
            Length::Fixed(self.size.width),
            Length::Fixed(self.size.height),
        )
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &cosmic::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        let nodes = self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(&self.places)
            .map(|((child, state), (at, width))| {
                let room = (self.size.height - at.y).max(1.0);
                let limits = layout::Limits::new(Size::ZERO, Size::new(*width, room));
                child
                    .as_widget_mut()
                    .layout(state, renderer, &limits)
                    .move_to(*at)
            })
            .collect();
        layout::Node::with_children(self.size, nodes)
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
        // The pointer reaches only the topmost child under it: a menu over a
        // pane takes the press, the slider beneath it doesn't. Everything
        // else (keys, time) goes to all.
        let pointer = matches!(event, Event::Mouse(_) | Event::Touch(_));
        let covered = cursor.position().and_then(|at| {
            let bounds: Vec<Rectangle> = layout.children().map(|child| child.bounds()).collect();
            self.stacked()
                .into_iter()
                .filter(|i| bounds[*i].contains(at))
                .next_back()
        });
        // Reported once a frame while the app knows something else: a report
        // the runtime drops (it does, while a popup is being built) is tried
        // again next frame, and none go out once the app has caught up.
        if let (
            Some((known, report)),
            Event::Window(cosmic::iced::window::Event::RedrawRequested(_)),
        ) = (&self.report, event)
        {
            let now: Vec<Rectangle> = layout.children().map(|child| child.bounds()).collect();
            let same = now.len() == known.len()
                && now.iter().zip(known.iter()).all(|(a, b)| {
                    (a.x - b.x).abs() < 0.5
                        && (a.y - b.y).abs() < 0.5
                        && (a.width - b.width).abs() < 0.5
                        && (a.height - b.height).abs() < 0.5
                });
            if !same {
                shell.publish(report(now));
            }
        }
        if let (Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)), Some(top)) =
            (event, covered)
        {
            if let Some(Some(message)) = self.pressing.get(top) {
                shell.publish(message.clone());
            }
        }
        for (i, ((child, state), layout)) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
            .enumerate()
        {
            if pointer
                && covered.is_some_and(|top| top != i)
                && !matches!(event, Event::Mouse(mouse::Event::CursorMoved { .. }))
            {
                continue;
            }
            child.as_widget_mut().update(
                state, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }
        if let (Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)), Some(message)) =
            (event, &self.on_press_outside)
        {
            let bounds = layout.bounds();
            if let Some(position) = cursor.position_in(bounds) {
                let position = position + Vector::new(bounds.x, bounds.y);
                if !layout
                    .children()
                    .any(|child| child.bounds().contains(position))
                {
                    shell.publish(message.clone());
                }
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
        // Each in a layer of its own, so a higher child (a menu) lies over a
        // lower one whatever they draw: within a layer the renderer paints
        // images over quads, and the panes hold images.
        let layouts: Vec<Layout<'_>> = layout.children().collect();
        for i in self.stacked() {
            let (child, state, layout) = (&self.children[i], &tree.children[i], layouts[i]);
            let bounds = layout.bounds();
            let roomy = Rectangle {
                x: bounds.x - 16.0,
                y: bounds.y - 16.0,
                width: bounds.width + 32.0,
                height: bounds.height + 32.0,
            };
            renderer.with_layer(roomy, |renderer| {
                child
                    .as_widget()
                    .draw(state, renderer, theme, style, layout, cursor, viewport);
            });
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &cosmic::Renderer,
    ) -> mouse::Interaction {
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, state), layout)| {
                child
                    .as_widget()
                    .mouse_interaction(state, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        operation: &mut dyn Operation,
    ) {
        for ((child, state), layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child
                .as_widget_mut()
                .operate(state, layout, renderer, operation);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &cosmic::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, cosmic::Theme, cosmic::Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message: Clone + 'a> From<Placed<'a, Message>> for cosmic::Element<'a, Message> {
    fn from(placed: Placed<'a, Message>) -> Self {
        cosmic::Element::new(placed)
    }
}

/// Rectangles with rounded corners, as a Wayland region can hold them: the
/// middle in one piece, each corner row as a strip one pixel high, inset to
/// follow the curve. The compositor's frost then stops at the panes'
/// rounded edges instead of filling their corners.
pub fn rounded_region(rects: &[Rectangle], radius: f32) -> Vec<Rectangle> {
    let mut region = Vec::new();
    for rect in rects {
        let r = radius
            .min(rect.width / 2.0)
            .min(rect.height / 2.0)
            .floor()
            .max(0.0);
        if r < 1.0 {
            region.push(*rect);
            continue;
        }
        region.push(Rectangle {
            x: rect.x,
            y: rect.y + r,
            width: rect.width,
            height: rect.height - 2.0 * r,
        });
        for row in 0..r as usize {
            // How far the curve is from the side, at the middle of this row.
            let from_centre = r - row as f32 - 0.5;
            let inset = (r - (r * r - from_centre * from_centre).max(0.0).sqrt()).round();
            let width = rect.width - 2.0 * inset;
            region.push(Rectangle {
                x: rect.x + inset,
                y: rect.y + row as f32,
                width,
                height: 1.0,
            });
            region.push(Rectangle {
                x: rect.x + inset,
                y: rect.y + rect.height - 1.0 - row as f32,
                width,
                height: 1.0,
            });
        }
    }
    region
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_follow_the_curve() {
        let region = rounded_region(
            &[Rectangle {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 60.0,
            }],
            10.0,
        );
        // The middle, and two strips per corner row.
        assert_eq!(region.len(), 1 + 2 * 10);
        // The top row is inset most, the row by the middle hardly at all.
        assert!(region[1].x >= 5.0);
        assert!(region[region.len() - 2].x <= 1.0);
        assert!(region.iter().all(|r| r.x >= 0.0 && r.x + r.width <= 100.0));
    }
}
