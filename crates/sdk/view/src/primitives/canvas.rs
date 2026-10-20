use crate::{Element, IntoElement, Lowering, wire};
use gpui::{Bounds, Hsla, Pixels, Point, StyleRefinement, Styled};

/// Geometry the host paints, in the canvas's own coordinates. gpui's
/// `canvas` paints in closures, which cannot cross to the host; a view's
/// is drawn by the calls below, in order.
pub struct Canvas {
    commands: Vec<wire::CanvasCommand>,
    style: Box<StyleRefinement>,
}

pub fn canvas() -> Canvas {
    Canvas {
        commands: Vec::new(),
        style: Box::default(),
    }
}

impl Canvas {
    /// `bounds`, filled with `color`.
    pub fn rect(self, bounds: Bounds<Pixels>, color: impl Into<Hsla>) -> Self {
        let shape = wire::CanvasShape::Rectangle {
            position: [f32::from(bounds.origin.x), f32::from(bounds.origin.y)],
            size: [f32::from(bounds.size.width), f32::from(bounds.size.height)],
            radius: [0.; 4],
        };
        self.fill(shape, color.into())
    }

    /// A disc at `center`, filled with `color`.
    pub fn circle(self, center: Point<Pixels>, radius: Pixels, color: impl Into<Hsla>) -> Self {
        let shape = wire::CanvasShape::Circle {
            center: [f32::from(center.x), f32::from(center.y)],
            radius: f32::from(radius),
        };
        self.fill(shape, color.into())
    }

    /// A line from `from` to `to`, `width` wide.
    pub fn line(
        mut self,
        from: Point<Pixels>,
        to: Point<Pixels>,
        width: Pixels,
        color: impl Into<Hsla>,
    ) -> Self {
        self.commands.push(wire::CanvasCommand::Draw {
            shape: wire::CanvasShape::Line {
                from: [f32::from(from.x), f32::from(from.y)],
                to: [f32::from(to.x), f32::from(to.y)],
            },
            fill: None,
            even_odd: false,
            stroke: Some(wire::CanvasStroke {
                color: color.into(),
                width: f32::from(width),
                cap: wire::CanvasLineCap::Butt,
                join: wire::CanvasLineJoin::Miter,
                dash: Vec::new(),
                dash_offset: 0,
            }),
        });
        self
    }

    fn fill(mut self, shape: wire::CanvasShape, color: Hsla) -> Self {
        self.commands.push(wire::CanvasCommand::Draw {
            shape,
            fill: Some(color),
            even_odd: false,
            stroke: None,
        });
        self
    }
}

impl Styled for Canvas {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Element for Canvas {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let this = *self;
        wire::Node::Canvas {
            style: lowering.style(&this.style),
            commands: this.commands,
        }
    }
}

impl IntoElement for Canvas {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl gpui::prelude::FluentBuilder for Canvas {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::App;
    use gpui::{point, px, size};

    #[test]
    fn a_canvas_crosses_its_calls_in_order() {
        let red = gpui::red();
        let drawn = canvas()
            .rect(
                Bounds::new(point(px(1.), px(2.)), size(px(3.), px(4.))),
                red,
            )
            .circle(point(px(5.), px(6.)), px(7.), red)
            .line(point(px(0.), px(0.)), point(px(8.), px(9.)), px(1.), red);
        let mut app = App::for_driver();
        let mut window = app.window();
        let wire::Node::Canvas { commands, .. } = Lowering::new(&mut window, &mut app).lower(drawn)
        else {
            panic!("a canvas")
        };
        let shapes: Vec<_> = commands
            .iter()
            .map(|command| match command {
                wire::CanvasCommand::Draw { shape, .. } => shape.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            shapes,
            [
                wire::CanvasShape::Rectangle {
                    position: [1., 2.],
                    size: [3., 4.],
                    radius: [0.; 4],
                },
                wire::CanvasShape::Circle {
                    center: [5., 6.],
                    radius: 7.,
                },
                wire::CanvasShape::Line {
                    from: [0., 0.],
                    to: [8., 9.],
                },
            ]
        );
    }
}
