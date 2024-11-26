use gpui::{
    canvas, div, point, prelude::*, px, size, App, AppContext, Bounds, Hsla, MouseDownEvent, Path,
    Pixels, Point, Render, ViewContext, WindowOptions,
};
struct PaintingViewer {
    default_lines: Vec<(Path<Pixels>, Hsla)>,
    lines: Vec<Vec<Point<Pixels>>>,
    start: Point<Pixels>,
    _painting: bool,
}

impl PaintingViewer {
    fn new() -> Self {
        let mut lines = vec![];

        // draw a line
        let mut path = Path::new(point(px(20.), px(100.)));
        path.line_to(point(px(50.), px(160.)));
        path.line_to(point(px(80.), px(100.)));
        path.line_to(point(px(80.5), px(100.5)));
        path.line_to(point(px(51.), px(160.)));
        path.line_to(point(px(21.), px(100.)));
        lines.push((path, gpui::black()));

        // draw a triangle
        let mut path = Path::new(point(px(25.), px(0.)));
        path.line_to(point(px(50.), px(50.)));
        path.line_to(point(px(0.), px(50.)));
        path.translate(point(px(100.), px(100.)));
        lines.push((path, gpui::red()));

        // draw a lightening bolt ⚡
        let mut path = Path::new(point(px(-50.), px(50.)));
        path.line_to(point(px(0.), px(-25.)));
        path.line_to(point(px(0.), px(0.)));
        path.move_to(point(px(0.), px(0.)));
        path.line_to(point(px(50.), px(-50.)));
        path.line_to(point(px(0.), px(25.)));
        path.line_to(point(px(0.), px(5.)));
        path.translate(point(px(220.), px(150.)));
        lines.push((path, gpui::blue()));

        // draw a ⭐
        let mut path = Path::new(point(px(76.8), px(116.864)));
        path.line_to(point(px(31.6608), px(142.1312)));
        path.line_to(point(px(41.7408), px(91.392)));
        path.line_to(point(px(3.7568), px(56.2688)));
        path.line_to(point(px(55.1296), px(50.176)));
        path.line_to(point(px(76.8), px(3.2)));
        path.line_to(point(px(98.4704), px(50.176)));
        path.line_to(point(px(149.8432), px(56.2688)));
        path.line_to(point(px(111.8592), px(91.392)));
        path.line_to(point(px(121.9392), px(142.1312)));
        path.translate(point(px(270.), px(80.)));
        lines.push((path, gpui::yellow()));

        // draw double square
        // https://yqnn.github.io/svg-path-editor/#P=M_2_1_L_2_3_L_4_3_L_4_4_L_6_4_L_6_2_L_4_2_L_4_1_L_2_1
        let mut path = Path::new(point(px(0.), px(50.)));
        path.line_to(point(px(0.), px(150.)));
        path.line_to(point(px(100.), px(150.)));
        path.line_to(point(px(100.), px(200.)));
        path.line_to(point(px(200.), px(200.)));
        path.line_to(point(px(200.), px(100.)));
        path.line_to(point(px(100.), px(100.)));
        path.line_to(point(px(100.), px(50.)));
        path.line_to(point(px(0.), px(50.)));
        path.translate(point(px(20.), px(200.)));
        lines.push((path, gpui::black()));

        // draw a square with rounded corners
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
        lines.push((path, gpui::green()));

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
                                const STROKE_WIDTH: Pixels = px(1.0);
                                for (path, color) in default_lines {
                                    cx.paint_path(path, color);
                                }
                                for points in lines {
                                    let mut path = Path::new(points[0]);
                                    for p in points.iter().skip(1) {
                                        path.line_to(*p);
                                    }

                                    let mut last = points.last().unwrap();
                                    for p in points.iter().rev() {
                                        let dx = p.x - last.x;
                                        let dy = p.y - last.y;
                                        let distance = (dx * dx + dy * dy).0.sqrt();
                                        let offset_x = (STROKE_WIDTH  * dy / distance).clamp(px(0.0), STROKE_WIDTH);
                                        let offset_y = (STROKE_WIDTH  * dx / distance).clamp(px(0.0), STROKE_WIDTH);
                                        path.line_to(point(p.x + offset_x, p.y - offset_y));
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
