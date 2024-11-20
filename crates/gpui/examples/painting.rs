use epaint::{pos2, Shape};
use gpui::{
    canvas, div, point, prelude::*, px, size, App, AppContext, Bounds, MouseDownEvent, Path,
    PathVertex, Pixels, Point, Render, ViewContext, WindowOptions,
};
struct PaintingViewer {
    default_shapes: Vec<Shape>,
    lines: Vec<Vec<Point<Pixels>>>,
    start: Point<Pixels>,
    _painting: bool,
}

fn tessellate(shape: epaint::Shape) -> (Vec<PathVertex<Pixels>>, Bounds<Pixels>) {
    let options = epaint::TessellationOptions::default();
    let mut mesh = epaint::Mesh::default();
    let rect = shape.visual_bounding_rect();

    epaint::tessellator::Tessellator::new(1., options, [12, 12], vec![])
        .tessellate_shape(shape, &mut mesh);

    let bounds = Bounds {
        origin: point(px(rect.min.x), px(rect.min.y)),
        size: size(px(rect.width()), px(rect.height())),
    };

    let mut vertices = vec![];
    for vertex in mesh.vertices {
        dbg!(vertex);
        vertices.push(PathVertex::new(
            point(px(vertex.pos.x), px(vertex.pos.y)),
            point(vertex.uv.x, vertex.uv.y),
        ));
    }

    (vertices, bounds)
}

impl PaintingViewer {
    fn new() -> Self {
        let mut shapes = vec![];
        let stroke = epaint::Stroke::new(1., epaint::Color32::BLACK);

        // draw a line
        let shape = Shape::line(
            vec![pos2(50., 180.), pos2(100., 100.), pos2(150., 50.)],
            stroke,
        );
        shapes.push(shape);

        // // draw a lightening bolt ⚡
        // let shape = Shape::line(
        //     vec![
        //         pos2(150., 200.),
        //         pos2(200., 125.),
        //         pos2(200., 175.),
        //         pos2(250., 100.),
        //     ],
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
                                const STROKE_WIDTH: Pixels = px(2.0);
                                for shape in shapes {
                                    let mut path = Path::new(point(px(0.), px(0.)));
                                    let (vertices, bounds) = tessellate(shape);
                                    path.set_vertices(vertices, bounds);

                                    cx.paint_path(path, gpui::black());
                                }
                                for points in lines {
                                    let mut path = Path::new(points[0]);
                                    for p in points.iter().skip(1) {
                                        path.line_to(*p);
                                    }

                                    let mut last = points.last().unwrap();
                                    for p in points.iter().rev() {
                                        let mut offset_x = px(0.);
                                        if last.x == p.x {
                                            offset_x = STROKE_WIDTH;
                                        }
                                        path.line_to(point(p.x + offset_x, p.y  + STROKE_WIDTH));
                                        last = p;
                                    }

                                    cx.paint_path(path, gpui::black());
                                }
                            },
                        )
                        .size_full(),
                    )
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, ev: &MouseDownEvent, _| {
                            this._painting = true;
                            this.start = ev.position;
                            let path = vec![ev.position];
                            this.lines.push(path);
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
            |cx| cx.new_view(|_| PaintingViewer::new()),
        )
        .unwrap();
        cx.activate(true);
    });
}
