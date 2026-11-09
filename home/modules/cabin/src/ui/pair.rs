// SPDX-License-Identifier: GPL-3.0-only

//! Two columns side by side where the right one sets the height and the
//! left one is made to match it: the date menu's notifications beside its
//! calendar. A plain row would let a filling left column take all the room
//! there is.

use cosmic::iced::{
    Event, Length, Point, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree},
    },
};

pub struct Pair<'a, Message> {
    children: Vec<cosmic::Element<'a, Message>>,
    /// The right column's width; the left takes the rest.
    right_width: f32,
    /// The height the left column is never shorter than.
    min_height: f32,
}

impl<'a, Message> Pair<'a, Message> {
    pub fn new(left: impl Into<cosmic::Element<'a, Message>>, right: impl Into<cosmic::Element<'a, Message>>, right_width: f32) -> Self {
        Self { children: vec![left.into(), right.into()], right_width, min_height: 0.0 }
    }

    pub fn min_height(mut self, height: f32) -> Self {
        self.min_height = height;
        self
    }
}

impl<Message> Widget<Message, cosmic::Theme, cosmic::Renderer> for Pair<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.children);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &cosmic::Renderer, limits: &layout::Limits) -> layout::Node {
        let width = limits.max().width;
        let left_width = (width - self.right_width).max(1.0);
        let (left, right) = self.children.split_at_mut(1);
        let (left_tree, right_tree) = tree.children.split_at_mut(1);
        // The right column as tall as it likes; the left exactly as tall.
        let right_node = right[0].as_widget_mut().layout(&mut right_tree[0], renderer, &layout::Limits::new(Size::ZERO, Size::new(self.right_width, limits.max().height)));
        let height = right_node.size().height.max(self.min_height).min(limits.max().height);
        let left_node = left[0].as_widget_mut().layout(&mut left_tree[0], renderer, &layout::Limits::new(Size::new(left_width, height), Size::new(left_width, height)));
        layout::Node::with_children(Size::new(width, height), vec![left_node, right_node.move_to(Point::new(left_width, 0.0))])
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
        for ((child, state), layout) in self.children.iter_mut().zip(&mut tree.children).zip(layout.children()) {
            child.as_widget_mut().update(state, event, layout, cursor, renderer, clipboard, shell, viewport);
        }
    }

    fn draw(&self, tree: &Tree, renderer: &mut cosmic::Renderer, theme: &cosmic::Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        for ((child, state), layout) in self.children.iter().zip(&tree.children).zip(layout.children()) {
            child.as_widget().draw(state, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &cosmic::Renderer) -> mouse::Interaction {
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, state), layout)| child.as_widget().mouse_interaction(state, layout, cursor, viewport, renderer))
            .max()
            .unwrap_or_default()
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &cosmic::Renderer, operation: &mut dyn Operation) {
        for ((child, state), layout) in self.children.iter_mut().zip(&mut tree.children).zip(layout.children()) {
            child.as_widget_mut().operate(state, layout, renderer, operation);
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
        overlay::from_children(&mut self.children, tree, layout, renderer, viewport, translation)
    }
}

impl<'a, Message: 'a> From<Pair<'a, Message>> for cosmic::Element<'a, Message> {
    fn from(pair: Pair<'a, Message>) -> Self {
        cosmic::Element::new(pair)
    }
}
