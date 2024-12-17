use epaint::pos2;
use gpui::{
    canvas, div, point, prelude::*, px, size, App, AppContext, Bounds, MouseDownEvent, Path,
    Pixels, Point, Render, ViewContext, WindowOptions,
};
use usvg::tiny_skia_path::PathVerb;
struct PaintingViewer {
    default_lines: Vec<Path<Pixels>>,
    lines: Vec<Vec<Point<Pixels>>>,
    start: Point<Pixels>,
    _painting: bool,
}

impl PaintingViewer {
    fn new() -> Self {
        let mut lines = vec![];

        // draw a line
        let mut path = Path::new(point(px(50.), px(180.)));
        path.line_to(point(px(100.), px(120.)));
        // go back to close the path
        path.line_to(point(px(100.), px(121.)));
        path.line_to(point(px(50.), px(181.)));
        lines.push(path);

        // draw a lightening bolt ⚡
        let mut path = Path::new(point(px(150.), px(200.)));
        path.line_to(point(px(200.), px(125.)));
        path.line_to(point(px(200.), px(175.)));
        path.line_to(point(px(250.), px(100.)));
        lines.push(path);

        // draw a ⭐
        let mut path = Path::new(point(px(350.), px(100.)));
        path.line_to(point(px(370.), px(160.)));
        path.line_to(point(px(430.), px(160.)));
        path.line_to(point(px(380.), px(200.)));
        path.line_to(point(px(400.), px(260.)));
        path.line_to(point(px(350.), px(220.)));
        path.line_to(point(px(300.), px(260.)));
        path.line_to(point(px(320.), px(200.)));
        path.line_to(point(px(270.), px(160.)));
        path.line_to(point(px(330.), px(160.)));
        path.line_to(point(px(350.), px(100.)));
        // lines.push(path);

        let square_bounds = Bounds {
            origin: point(px(450.), px(100.)),
            size: size(px(200.), px(80.)),
        };
        let height = square_bounds.size.height;
        let horizontal_offset = height;
        let vertical_offset = px(30.);
        let mut path = Path::new(square_bounds.lower_left());
        path.curve_to(
            square_bounds.origin + point(horizontal_offset, vertical_offset),
            square_bounds.origin + point(px(0.0), vertical_offset),
        );
        path.line_to(square_bounds.upper_right() + point(-horizontal_offset, vertical_offset));
        path.curve_to(
            square_bounds.lower_right(),
            square_bounds.upper_right() + point(px(0.0), vertical_offset),
        );
        path.line_to(square_bounds.lower_left());
        lines.push(path);
        let stroke = epaint::Stroke::new(0., epaint::Color32::BLACK);

        let points = [
            pos2(350., 100.),
            pos2(370., 160.),
            pos2(430., 160.),
            pos2(380., 200.),
            pos2(400., 260.),
            pos2(350., 220.),
            pos2(300., 260.),
            pos2(320., 200.),
            pos2(270., 160.),
            pos2(330., 160.),
        ];

        // let path_shape = epaint::Shape::convex_polygon(points, epaint::Color32::BLUE, stroke);
        // let mut path = Path::new(point(px(0.), px(0.)));
        // for v in tessellate_path(&path_shape) {
        //     path.push_vertice(point(px(v.pos.x), px(v.pos.y)), point(v.uv.x, 1. - v.uv.y));
        // }
        // lines.push(path);

        let mut builder = lyon::path::Path::builder().with_svg();
        lyon_extra::rust_logo::build_logo_path(&mut builder);
        builder.close();
        let path = builder.build();
        let buf = lyon_tessellate_path(&path);
        let mut path = Path::new(point(px(0.), px(0.)));
        for v in buf.vertices {
            path.push_vertex(v.pos, v.st);
        }
        lines.push(path);

        Self {
            default_lines: lines.clone(),
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

#[derive(Copy, Clone, Debug)]
struct MyVertex {
    pos: Point<Pixels>,
    st: Point<f32>,
}

fn lyon_tessellate_path(
    path: &lyon::path::Path,
) -> lyon::tessellation::VertexBuffers<MyVertex, u16> {
    use lyon::*;
    use lyon_tessellation::*;

    // Will contain the result of the tessellation.
    let mut geometry: VertexBuffers<MyVertex, u16> = VertexBuffers::new();
    let mut tessellator = StrokeTessellator::new();

    // Compute the tessellation.
    tessellator
        .tessellate_path(
            path,
            &StrokeOptions::default(),
            &mut BuffersBuilder::new(&mut geometry, |vertex: StrokeVertex| MyVertex {
                pos: point(px(vertex.position().x), px(vertex.position().y)),
                st: point(0., 1.),
            }),
        )
        .unwrap();

    geometry
}

fn tessellate_path(shape: &epaint::Shape) -> Vec<epaint::Vertex> {
    let atlas = epaint::TextureAtlas::new([4096, 256]);
    let font_tex_size = atlas.size();
    let prepared_discs = atlas.prepared_discs();
    let mut tessellator = epaint::Tessellator::new(
        1.0,
        epaint::TessellationOptions::default(),
        font_tex_size,
        prepared_discs,
    );
    let mut mesh = epaint::Mesh::default();
    tessellator.tessellate_shape(shape.clone(), &mut mesh);

    mesh.vertices
}

impl Render for PaintingViewer {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let default_lines = self.default_lines.clone();
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
                                for path in default_lines {
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
