// SPDX-License-Identifier: GPL-3.0-only

//! A small graph of the last while: the area under the line filled faintly,
//! the line itself drawn thin. Two lines share one scale when there are two.

use cosmic::{
    Element,
    iced::{Color, Length, Point, Rectangle, Size, mouse, widget::canvas},
};

use super::palette::{ACCENT, EVENT, alpha};

pub struct Sparkline {
    /// Each series, oldest first; all drawn on the same scale.
    series: Vec<(Vec<f32>, Color)>,
    /// The top of the scale, at least.
    floor: f32,
    height: f32,
    /// The scale is `floor` exactly, not the values' reach.
    fixed: bool,
}

impl Sparkline {
    pub fn new(values: &[f32], floor: f32) -> Self {
        Self { series: vec![(values.to_vec(), ACCENT)], floor, height: 34.0, fixed: false }
    }

    /// One series on a fixed scale: a battery's percent, 0 to 100.
    pub fn fixed(values: &[f32], max: f32) -> Self {
        Self { series: vec![(values.to_vec(), ACCENT)], floor: max, height: 34.0, fixed: true }
    }

    /// Two series: down in the accent, up in pink.
    pub fn pair(down: &[f32], up: &[f32]) -> Self {
        Self { series: vec![(down.to_vec(), ACCENT), (up.to_vec(), EVENT)], floor: 1.0, height: 34.0, fixed: false }
    }

    fn top(&self) -> f32 {
        if self.fixed {
            return self.floor.max(1.0);
        }
        let most = self.series.iter().flat_map(|(v, _)| v.iter().copied()).fold(0.0_f32, f32::max);
        let scale = if self.series.len() > 1 { 1.1 } else { 1.15 };
        (most * scale).max(self.floor)
    }

    pub fn view<'a, M: 'a>(self) -> Element<'a, M> {
        let height = self.height;
        canvas::Canvas::new(self).width(Length::Fill).height(Length::Fixed(height)).into()
    }
}

impl<M> canvas::Program<M, cosmic::Theme, cosmic::Renderer> for Sparkline {
    type State = ();

    fn draw(&self, _state: &(), renderer: &cosmic::Renderer, _theme: &cosmic::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry<cosmic::Renderer>> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let Size { width, height } = bounds.size();
        let top = self.top();
        for (values, color) in &self.series {
            if values.len() < 2 {
                continue;
            }
            let step = width / (values.len() - 1) as f32;
            let point = |i: usize, v: f32| Point::new(i as f32 * step, height - (v / top).clamp(0.0, 1.0) * height);
            let line = canvas::Path::new(|path| {
                for (i, v) in values.iter().enumerate() {
                    let p = point(i, *v);
                    if i == 0 { path.move_to(p) } else { path.line_to(p) }
                }
            });
            let area = canvas::Path::new(|path| {
                path.move_to(Point::new(0.0, height));
                for (i, v) in values.iter().enumerate() {
                    path.line_to(point(i, *v));
                }
                path.line_to(Point::new(width, height));
                path.close();
            });
            frame.fill(&area, alpha(*color, if self.series.len() > 1 { 0.12 } else { 0.18 }));
            frame.stroke(&line, canvas::Stroke::default().with_color(*color).with_width(1.5));
        }
        vec![frame.into_geometry()]
    }
}
