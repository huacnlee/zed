use epaint::{pos2, Shape};
use gpui::{
    canvas, div, point, prelude::*, px, size, App, AppContext, Bounds, MouseDownEvent, Path,
    PathVertex, Pixels, Point, Render, ViewContext, WindowContext, WindowOptions,
};
struct PaintingViewer {
    default_shapes: Vec<Shape>,
    lines: Vec<Vec<Point<Pixels>>>,
    start: Point<Pixels>,
    _painting: bool,
}

fn tessellate(
    shape: epaint::Shape,
    cx: &WindowContext,
) -> (Vec<PathVertex<Pixels>>, Bounds<Pixels>) {
    let options = epaint::TessellationOptions::default();
    let mut mesh = epaint::Mesh::default();
    let rect = shape.visual_bounding_rect();

    epaint::tessellator::Tessellator::new(cx.scale_factor(), options, [0, 0], vec![])
        .tessellate_shape(shape, &mut mesh);

    let bounds = Bounds {
        origin: point(px(rect.min.x), px(rect.min.y)),
        size: size(px(rect.width()), px(rect.height())),
    };

    let mut vertices = vec![];
    for vertex in mesh.vertices {
        vertices.push(PathVertex::new(
            point(px(vertex.pos.x), px(vertex.pos.y)),
            point(vertex.uv.x, 1.0 - vertex.uv.y),
        ));
    }

    (vertices, bounds)
}

impl PaintingViewer {
    fn new(_cx: &mut ViewContext<Self>) -> Self {
        let mut shapes = vec![];
        let stroke = epaint::Stroke::new(1., epaint::Color32::BLACK);

        // draw a line
        let shape = Shape::line(vec![pos2(50., 140.), pos2(100., 220.)], stroke);
        shapes.push(shape);

        // draw a circle
        let shape = Shape::circle_filled(pos2(300., 200.), 100., epaint::Color32::BLACK);
        shapes.push(shape);

        // // draw a lightening bolt ⚡
        // let shape = Shape::closed_line(
        //     vec![pos2(520., 230.), pos2(620., 100.), pos2(700., 230.)],
        //     stroke,
        // );
        // shapes.push(shape);

        // // draw a ⭐
        // let shape = Shape::closed_line(
        //     vec![
        //         pos2(350., 100.),
        //         pos2(370., 160.),
        //         pos2(430., 160.),
        //         pos2(380., 200.),
        //         pos2(400., 260.),
        //         pos2(350., 220.),
        //         pos2(300., 260.),
        //         pos2(320., 200.),
        //         pos2(270., 160.),
        //         pos2(330., 160.),
        //         pos2(350., 100.),
        //     ],
        //     stroke,
        // );
        // shapes.push(shape);

        Self {
            default_shapes: shapes,
            lines: vec![],
            start: point(px(0.), px(0.)),
            _painting: false,
        }
    }

    fn clear(&mut self, cx: &mut ViewContext<Self>) {
        self.lines.clear();
        cx.notify();
    }
}
impl Render for PaintingViewer {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let shapes = self.default_shapes.clone();
        let lines = self.lines.clone();
        div()
            .font_family(".SystemUIFont")
            .bg(gpui::white())
            .size_full()
            .p_4()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .justify_between()
                    .items_center()
                    .child("Mouse down any point and drag to draw lines (Hold on shift key to draw straight lines)")
                    .child(
                        div()
                            .id("clear")
                            .child("Clean up")
                            .bg(gpui::black())
                            .text_color(gpui::white())
                            .active(|this| this.opacity(0.8))
                            .flex()
                            .px_3()
                            .py_1()
                            .on_click(cx.listener(|this, _, cx| {
                                this.clear(cx);
                            })),
                    ),
            )
            .child(
                div()
                    .size_full()
                    .child(
                        canvas(
                            move |_, _| {},
                            move |_, _, cx| {
                                let stroke = epaint::Stroke::new(1., epaint::Color32::BLACK);
                                for shape in shapes {
                                    let mut path = Path::new(point(px(0.), px(0.)));
                                    let (vertices, bounds) = tessellate(shape, cx);
                                    path.set_vertices(vertices, bounds);
                                    cx.paint_path(path, gpui::black());
                                }

                                for points in lines {
                                    let mut path = Path::new(point(px(0.), px(0.)));
                                    let shape = epaint::Shape::line(
                                        points.iter().map(|p| pos2(p.x.0, p.y.0)).collect(),
                                        stroke,
                                    );
                                    let (vertices, bounds) = tessellate(shape, cx);
                                    if vertices.len() > 0 {
                                        path.set_vertices(vertices, bounds);
                                        cx.paint_path(path, gpui::black());
                                    }
                                }
                            },
                        )
                        .size_full(),
                    )
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, ev: &MouseDownEvent, _| {
                            this.start = ev.position;
                            let path = vec![];
                            this.lines.push(path);
                            this._painting = true;
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, cx| {
                        if !this._painting {
                            return;
                        }

                        let is_shifted = ev.modifiers.shift;
                        let mut pos = ev.position;
                        // When holding shift, draw a straight line
                        if is_shifted {
                            let dx = pos.x - this.start.x;
                            let dy = pos.y - this.start.y;
                            if dx.abs() > dy.abs() {
                                pos.y = this.start.y;
                            } else {
                                pos.x = this.start.x;
                            }
                        }

                        if let Some(path) = this.lines.last_mut() {
                            if let Some(last_pos) = path.last() {
                                if pos == *last_pos {
                                    return
                                }
                            }

                            path.push(pos);
                        }

                        cx.notify();
                    }))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _| {
                            this._painting = false;
                        }),
                    ),
            )
    }
}

fn main() {
    App::new().run(|cx: &mut AppContext| {
        cx.open_window(
            WindowOptions {
                focus: true,
                ..Default::default()
            },
            |cx| cx.new_view(PaintingViewer::new),
        )
        .unwrap();
        cx.activate(true);
    });
}
