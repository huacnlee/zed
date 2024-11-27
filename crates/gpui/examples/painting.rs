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

fn build_line_path(points: Vec<Point<Pixels>>, width: f32) -> Path<Pixels> {
    let mut path = Path::new(point(points[0].x, points[0].y));
    let half_width = width / 2.0;
    let angle_threshold: f32 = 15.;
    // 4~6 for performance, 8~12 for medium, 16~24 for high quality
    const SEGMENT: usize = 0;
    let angle_threshold_cos = angle_threshold.to_radians().cos();

    for i in 0..points.len() - 1 {
        let p0 = points[i];
        let p1 = points[i + 1];

        // Calculate direction vector and normal
        let dx = p1.x - p0.x;
        let dy = p1.y - p0.y;
        let length = (dx * dx + dy * dy).0.sqrt();
        let dir = [dx / length, dy / length];
        let normal = [-dir[1] * half_width, dir[0] * half_width];

        // Current segment boundary vertices
        let left0 = [p0.x - normal[0], p0.y - normal[1]];
        let right0 = [p0.x + normal[0], p0.y + normal[1]];
        let left1 = [p1.x - normal[0], p1.y - normal[1]];
        let right1 = [p1.x + normal[0], p1.y + normal[1]];

        // Add main triangles of the current segment
        path.move_to(point(left0[0], left0[1]));
        path.line_to(point(right0[0], right0[1]));
        path.line_to(point(left1[0], left1[1]));

        path.move_to(point(right0[0], right0[1]));
        path.line_to(point(right1[0], right1[1]));
        path.line_to(point(left1[0], left1[1]));

        // Corner handling
        if i < points.len() - 2 {
            let p2 = points[i + 2];

            // Previous and next direction vectors
            let next_length = ((p2.x - p1.x).0.powi(2) + (p2.y - p1.y).0.powi(2)).sqrt();
            let prev_dir = [dir[0], dir[1]];
            let next_dir = [(p2.x - p1.x) / next_length, (p2.y - p1.y) / next_length];

            // Calculate angle
            let cos_angle = prev_dir[0] * next_dir[0] + prev_dir[1] * next_dir[1];

            if cos_angle.0 < -0.99 {
                // 180 degree turn: fill intersection area
                path.line_to(point(p1.x - normal[0], p1.y - normal[1]));
                path.line_to(point(p1.x + normal[0], p1.y + normal[1]));
                continue;
            } else if cos_angle.0 > angle_threshold_cos {
                // Sharp angle: fill intersection area, generate polygon cover
                let mut intersection_points = vec![
                    [p1.x + normal[0], p1.y + normal[1]],
                    [p1.x - normal[0], p1.y - normal[1]],
                ];
                let step = (1.0 - cos_angle.0) * (std::f32::consts::PI / 2.0) / SEGMENT as f32;
                for j in 0..=SEGMENT {
                    let theta = j as f32 * step;
                    let rotated = [
                        prev_dir[0] * theta.cos() - prev_dir[1] * theta.sin(),
                        prev_dir[0] * theta.sin() + prev_dir[1] * theta.cos(),
                    ];
                    let rounded_vertex = [
                        p1.x + rotated[0] * half_width,
                        p1.y + rotated[1] * half_width,
                    ];
                    intersection_points.push(rounded_vertex);
                }
                for k in 1..intersection_points.len() - 1 {
                    path.move_to(point(intersection_points[0][0], intersection_points[0][1]));
                    path.line_to(point(intersection_points[k][0], intersection_points[k][1]));
                    path.line_to(point(
                        intersection_points[k + 1][0],
                        intersection_points[k + 1][1],
                    ));
                }
            } else {
                // Regular corner handling
                let step = (std::f32::consts::PI - cos_angle.0.acos()) / SEGMENT as f32;
                for j in 0..=SEGMENT {
                    let theta = j as f32 * step;
                    let rotated = [
                        prev_dir[0] * theta.cos() - prev_dir[1] * theta.sin(),
                        prev_dir[0] * theta.sin() + prev_dir[1] * theta.cos(),
                    ];
                    let rounded_vertex = [
                        p1.x + rotated[0] * half_width,
                        p1.y + rotated[1] * half_width,
                    ];
                    path.line_to(point(rounded_vertex[0], rounded_vertex[1]));
                }
            }
        }
    }

    path
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
                                for (path, color) in default_lines {
                                    cx.paint_path(path, color);
                                }
                                for points in lines {
                                    let path = build_line_path(points, 1.5);
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
