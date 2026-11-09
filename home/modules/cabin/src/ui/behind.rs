// SPDX-License-Identifier: GPL-3.0-only

//! Something drawn behind content, at the content's size: a blurred cover
//! behind a player's card. The content decides how big the card is; the
//! back is stretched to that and never takes the pointer.

use cosmic::iced::{
    Event, Length, Rectangle, Size, Vector,
    advanced::{
        Clipboard, Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree},
    },
};

pub struct Behind<'a, Message> {
    back: cosmic::Element<'a, Message>,
    front: cosmic::Element<'a, Message>,
}

impl<'a, Message> Behind<'a, Message> {
    pub fn new(back: impl Into<cosmic::Element<'a, Message>>, front: impl Into<cosmic::Element<'a, Message>>) -> Self {
        Self { back: back.into(), front: front.into() }
    }
}

impl<Message> Widget<Message, cosmic::Theme, cosmic::Renderer> for Behind<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.back), Tree::new(&self.front)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut [&mut self.back, &mut self.front]);
    }

    fn size(&self) -> Size<Length> {
        self.front.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &cosmic::Renderer, limits: &layout::Limits) -> layout::Node {
        let front = self.front.as_widget_mut().layout(&mut tree.children[1], renderer, limits);
        let size = front.size();
        // The very same bounds as the front, so their rounded edges coincide.
        let back = self.back.as_widget_mut().layout(&mut tree.children[0], renderer, &layout::Limits::new(size, size));
        layout::Node::with_children(size, vec![back, front])
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
        let front = layout.children().nth(1).expect("the front");
        self.front.as_widget_mut().update(&mut tree.children[1], event, front, cursor, renderer, clipboard, shell, viewport);
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
        let back = children.next().expect("the back");
        let front = children.next().expect("the front");
        self.back.as_widget().draw(&tree.children[0], renderer, theme, style, back, cursor, viewport);
        // In a layer of its own: within one layer the renderer paints images
        // over quads whatever the order, and the back is an image.
        // The layer a little larger than the card, so the card's soft rounded
        // edge isn't cut hard by the layer's clip.
        let bounds = layout.bounds();
        let roomy = Rectangle { x: bounds.x - 4.0, y: bounds.y - 4.0, width: bounds.width + 8.0, height: bounds.height + 8.0 };
        renderer.with_layer(roomy, |renderer| {
            self.front.as_widget().draw(&tree.children[1], renderer, theme, style, front, cursor, viewport);
        });
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &cosmic::Renderer,
    ) -> mouse::Interaction {
        let front = layout.children().nth(1).expect("the front");
        self.front.as_widget().mouse_interaction(&tree.children[1], front, cursor, viewport, renderer)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &cosmic::Renderer, operation: &mut dyn Operation) {
        let front = layout.children().nth(1).expect("the front");
        self.front.as_widget_mut().operate(&mut tree.children[1], front, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &cosmic::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, cosmic::Theme, cosmic::Renderer>> {
        let front = layout.children().nth(1).expect("the front");
        self.front.as_widget_mut().overlay(&mut tree.children[1], front, renderer, viewport, translation)
    }
}

impl<'a, Message: 'a> From<Behind<'a, Message>> for cosmic::Element<'a, Message> {
    fn from(behind: Behind<'a, Message>) -> Self {
        cosmic::Element::new(behind)
    }
}
