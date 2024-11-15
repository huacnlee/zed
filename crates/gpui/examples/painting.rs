use gpui::{
    canvas, div, point, prelude::*, px, size, App, AppContext, Bounds, MouseDownEvent, Path,
    Pixels, Point, Render, ViewContext, WindowOptions,
};
struct PaintingViewer {
    default_lines: Vec<Path<Pixels>>,
    lines: Vec<Vec<Point<Pixels>>>,
    start: Point<Pixels>,
    _painting: bool,
}

impl PaintingViewer {
    fn new() -> Self {
        let mut lines = vec![];
        let points = [
            point(px(20.), px(110.)),
            point(px(50.), px(20.)),
            point(px(60.), px(50.)),
            point(px(70.), px(30.)),
            point(px(80.), px(88.)),
            point(px(90.), px(40.)),
            point(px(90.), px(40.)),
            point(px(110.), px(34.)),
            point(px(140.), px(90.)),
            point(px(170.), px(50.)),
            point(px(200.), px(100.)),
            point(px(230.), px(150.)),
            point(px(250.), px(70.)),
            point(px(262.), px(25.)),
            point(px(280.), px(22.)),
            point(px(290.), px(25.)),
            point(px(300.), px(50.)),
        ];
        let offset = point(px(30.), px(160.));
        let mut path = Path::new(points[0] + offset);
        for p in points.iter().skip(1) {
            path.line_to(*p + offset);
        }
        path.stroke(px(1.));
        lines.push(path);

        // draw a lightening bolt ⚡
        let points = [
            point(px(150.), px(200.)),
            point(px(200.), px(125.)),
            point(px(200.), px(175.)),
            point(px(250.), px(100.)),
        ];
        let offset = point(px(200.), px(80.));
        let mut path = Path::new(points[0] + offset);
        for p in points.iter().skip(1) {
            path.line_to(*p + offset);
        }
        lines.push(path);

        // draw a ⭐
        let points = [
            point(px(350.), px(100.)),
            point(px(370.), px(160.)),
            point(px(430.), px(160.)),
            point(px(380.), px(200.)),
            point(px(400.), px(260.)),
            point(px(350.), px(220.)),
            point(px(300.), px(260.)),
            point(px(320.), px(200.)),
            point(px(270.), px(160.)),
            point(px(330.), px(160.)),
            point(px(350.), px(100.)),
        ];
        let offset = point(px(200.), px(80.));
        let mut path = Path::new(points[0] + offset);
        for p in points.iter().skip(1) {
            path.line_to(*p + offset);
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
                                for path in default_lines {
                                    cx.paint_path(path, gpui::black());
                                }

                                for points in lines {
                                    let mut path = Path::new(points[0]);
                                    for p in points.iter().skip(1) {
                                        path.line_to(*p);
                                    }
                                    path.stroke(px(1.));

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
