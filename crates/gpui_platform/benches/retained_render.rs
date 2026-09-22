use gpui::{
    BenchAppContext, Context, InputEvent, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement as _, Render, RenderImage, Styled as _, Window,
    div, img, point, px, rgb, svg,
};
use std::{cell::Cell, fmt, rc::Rc, sync::Arc, time::Duration};

#[derive(Clone, Copy, Debug)]
enum Change {
    SingleColor,
    SingleSize,
    AllColors,
    SingleText,
    TextContainerSize,
    SingleImage,
    SingleSvg,
    Handlers,
    CanvasText,
    TextDecoration,
}

#[derive(Clone, Copy)]
struct Workload {
    nodes: usize,
    change: Change,
}

impl fmt::Display for Workload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}-{}", self.change, self.nodes)
    }
}

fn workloads() -> Vec<Workload> {
    [100, 1_000, 10_000]
        .into_iter()
        .flat_map(|nodes| {
            [
                Change::SingleColor,
                Change::SingleSize,
                Change::AllColors,
                Change::SingleText,
                Change::TextContainerSize,
                Change::SingleImage,
                Change::SingleSvg,
                Change::Handlers,
                Change::CanvasText,
                Change::TextDecoration,
            ]
            .into_iter()
            .map(move |change| Workload { nodes, change })
        })
        .collect()
}

struct Grid {
    workload: Workload,
    revision: usize,
    rendered_revision: Rc<Cell<usize>>,
    images: [Arc<RenderImage>; 2],
    invoked_revision: Rc<Cell<usize>>,
    painted_revision: Rc<Cell<usize>>,
}

impl Render for Grid {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.rendered_revision.set(self.revision);
        let alternate = !self.revision.is_multiple_of(2);
        let has_text = matches!(
            self.workload.change,
            Change::SingleText
                | Change::TextContainerSize
                | Change::CanvasText
                | Change::TextDecoration
        );
        div()
            .flex()
            .flex_wrap()
            .w(px(800.))
            .children((0..self.workload.nodes).map(|index| {
                let changed = index == 0;
                let color_changed = match self.workload.change {
                    Change::SingleColor => changed,
                    Change::AllColors => true,
                    Change::SingleSize | Change::SingleText | Change::TextContainerSize
                    | Change::SingleImage | Change::SingleSvg | Change::Handlers | Change::CanvasText | Change::TextDecoration => false,
                };
                let base_width = if has_text { 80. } else { 8. };
                let width = if matches!(
                    self.workload.change,
                    Change::SingleSize | Change::TextContainerSize
                ) && changed
                    && alternate
                {
                    base_width + 1.
                } else {
                    base_width
                };
                let element = div()
                    .id(index)
                    .w(px(width))
                    .h(px(if has_text { 20. } else { 8. }))
                    .flex_shrink_0()
                    .bg(rgb(if color_changed && alternate {
                        0x6688aa
                    } else {
                        0x224466
                    }));
                if matches!(self.workload.change, Change::CanvasText) {
                    let text = if changed && alternate { "edit" } else { "row" };
                    let revision = if changed { self.revision } else { 0 };
                    let painted_revision = self.painted_revision.clone();
                    element.child(gpui::canvas(|_, _, _| (), move |bounds, (), window, cx| {
                        let style = window.text_style();
                        let line = window.text_system().shape_line(text.into(), px(14.), &[style.to_run(text.len())], None);
                        line.paint(bounds.origin, px(20.), gpui::TextAlign::Left, None, window, cx).expect("paint benchmark text");
                        if changed { painted_revision.set(revision); }
                    }).retained(index, revision as u64).size_full())
                } else if matches!(self.workload.change, Change::TextDecoration) {
                    element.child(gpui::StyledText::new("row").with_highlights([(0..3, gpui::HighlightStyle {
                        background_color: Some(rgb(if changed && alternate { 0x446688 } else { 0x224466 }).into()),
                        underline: Some(gpui::UnderlineStyle { thickness: px(1.), color: Some(rgb(0xffffff).into()), wavy: changed && alternate }),
                        ..Default::default()
                    })]))
                } else if has_text {
                    element.child(
                        if matches!(self.workload.change, Change::SingleText)
                            && changed
                            && alternate
                        {
                            "edit"
                        } else {
                            "row"
                        },
                    )
                } else if matches!(self.workload.change, Change::SingleImage) {
                    let image = if changed && alternate { &self.images[1] } else { &self.images[0] };
                    element.child(img(image.clone()).size_full())
                } else if matches!(self.workload.change, Change::SingleSvg) {
                    let data = if changed && alternate {
                        br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><circle cx="4" cy="4" r="4"/></svg>"#.as_slice()
                    } else {
                        br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8"/></svg>"#.as_slice()
                    };
                    element.child(svg().data(data).size_full())
                } else if matches!(self.workload.change, Change::Handlers) {
                    let revision = self.revision;
                    let invoked = self.invoked_revision.clone();
                    element.on_mouse_down(MouseButton::Left, move |_, _, _| invoked.set(revision))
                } else {
                    element
                }
            }))
    }
}

#[gpui::bench(inputs = workloads(), input_name = "workload", group = "retained_tree", fps = 120)]
fn frames(workload: &Workload, cx: &mut BenchAppContext) {
    assert!(!cfg!(debug_assertions), "use --profile release-fast");
    let rendered_revision = Rc::new(Cell::new(0));
    let invoked_revision = Rc::new(Cell::new(0));
    let painted_revision = Rc::new(Cell::new(0));
    let mut window = cx.add_empty_window();
    let view = window.update(|window, cx| {
        assert!(
            !cx.text_system().all_font_names().is_empty(),
            "benchmark requires a native font backend"
        );
        window.replace_root(cx, |_, _| Grid {
            workload: *workload,
            revision: 0,
            rendered_revision: rendered_revision.clone(),
            invoked_revision: invoked_revision.clone(),
            painted_revision: painted_revision.clone(),
            images: [0x22, 0xaa].map(|red| {
                Arc::new(RenderImage::new(vec![image::Frame::new(
                    image::ImageBuffer::from_pixel(8, 8, image::Rgba([red, 0x44, 0x66, 0xff])),
                )]))
            }),
        })
    });
    cx.run_until_idle();
    let frames_before = window.update(|window, _| window.frame_duration_snapshot());
    let mut updates = 0;
    cx.bench_renderer(view, |view, _, cx| {
        view.revision += 1;
        updates += 1;
        cx.notify();
    });
    let frames_after = window.update(|window, _| window.frame_duration_snapshot());
    let retained = window.update(|window, _| window.retained_frame_snapshot());
    assert!(updates > 0);
    assert_eq!(rendered_revision.get(), updates);
    if matches!(workload.change, Change::CanvasText) {
        assert_eq!(painted_revision.get(), updates);
    }
    assert_eq!(
        frames_after.draw_duration_histogram.len() - frames_before.draw_duration_histogram.len(),
        updates as u64
    );
    let retained_enabled = std::env::var("GPUI_RETAINED_TREE").is_ok_and(|value| value == "1");
    assert_eq!(retained.enabled, retained_enabled);
    if retained_enabled {
        assert_eq!(
            retained.property_snapshots + 1,
            retained.nodes_total,
            "all benchmark nodes except the conservative root ViewElement must retain properties"
        );
        assert!(retained.property_bytes > 0);
    } else {
        assert_eq!(retained.property_snapshots, 0);
        assert_eq!(retained.property_bytes, 0);
    }
    if matches!(workload.change, Change::Handlers) {
        window.update(|window, cx| {
            let position = point(px(1.), px(1.));
            window.dispatch_event(
                MouseMoveEvent {
                    position,
                    modifiers: Default::default(),
                    pressed_button: None,
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                MouseDownEvent {
                    position,
                    button: MouseButton::Left,
                    modifiers: Default::default(),
                    click_count: 1,
                    first_mouse: false,
                }
                .to_platform_input(),
                cx,
            );
        });
        assert_eq!(invoked_revision.get(), updates);
    }
}

gpui::bench_group! {
    name = benches;
    config = criterion::Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .without_plots();
    targets = frames
}
gpui::bench_main!(benches);
