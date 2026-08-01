use gpui::{
    BenchAppContext, Context, Entity, IntoElement, PaintGroupId, Render, SharedString,
    StyleRefinement, Window, div, prelude::*, px, rgb,
};

#[gpui::bench]
fn rerender_empty_view(cx: &mut BenchAppContext) {
    let mut window = cx.add_empty_window();
    let view = window.update(|window, cx| window.replace_root(cx, |_window, _cx| EmptyBenchView));
    cx.bench_renderer(view, |_view, _window, cx| cx.notify());
}

#[gpui::bench]
fn present_unchanged_scene(cx: &mut BenchAppContext) {
    let mut window = cx.add_empty_window();
    let view = window.update(|window, cx| {
        window.replace_root(cx, |_window, _cx| StaticRowsBenchView {
            rows: bench_rows(1_000),
        })
    });
    cx.bench_presenter(view);
}

#[gpui::bench]
fn blink_cursor_in_paint_group(cx: &mut BenchAppContext) {
    let cursor_group = PaintGroupId::new();
    let mut window = cx.add_empty_window();
    let view = window.update(|window, cx| {
        window.replace_root(cx, |_window, _cx| PaintGroupCursorBenchView {
            rows: bench_rows(1_000),
            cursor_group,
        })
    });
    cx.bench_paint_group_visibility(view, cursor_group);
}

#[gpui::bench(
    inputs = row_counts(),
    group = "Blink cursor in one entity",
    input_name = "static rows",
    sample_size = 30
)]
fn blink_cursor_in_one_entity(row_count: &usize, cx: &mut BenchAppContext) {
    let mut window = cx.add_empty_window();
    let view = window.update(|window, cx| {
        window.replace_root(cx, |_window, _cx| BlinkCursorBenchView {
            rows: bench_rows(*row_count),
            cursor_visible: true,
        })
    });
    cx.bench_renderer(view, |view, _window, cx| {
        view.cursor_visible = !view.cursor_visible;
        cx.notify();
    });
}

#[gpui::bench(
    inputs = row_counts(),
    group = "Blink cursor with keyed rows",
    input_name = "static rows",
    sample_size = 30
)]
fn blink_cursor_with_keyed_rows(row_count: &usize, cx: &mut BenchAppContext) {
    let mut window = cx.add_empty_window();
    let view = window.update(|window, cx| {
        window.replace_root(cx, |_window, _cx| BlinkCursorWithKeyedRowsBenchView {
            rows: bench_rows(*row_count),
            cursor_visible: true,
        })
    });
    cx.bench_renderer(view, |view, _window, cx| {
        view.cursor_visible = !view.cursor_visible;
        cx.notify();
    });
}

#[gpui::bench(
    inputs = row_counts(),
    group = "Blink cursor in leaf entity",
    input_name = "static rows",
    sample_size = 30
)]
fn blink_cursor_in_leaf_entity(row_count: &usize, cx: &mut BenchAppContext) {
    let cursor = cx.new(|_| CursorBenchView {
        cursor_visible: true,
    });
    let mut window = cx.add_empty_window();
    window.update(|window, cx| {
        let cursor = cursor.clone();
        let row_count = *row_count;
        window.replace_root(cx, |_window, _cx| LargeViewWithCursorEntity {
            rows: bench_rows(row_count),
            render_text: true,
            cursor,
        });
    });
    cx.bench_renderer(cursor, |cursor, _window, cx| {
        cursor.cursor_visible = !cursor.cursor_visible;
        cx.notify();
    });
}

#[gpui::bench(
    inputs = row_counts(),
    group = "Blink cursor beside empty rows",
    input_name = "static rows",
    sample_size = 30
)]
fn blink_cursor_beside_empty_rows(row_count: &usize, cx: &mut BenchAppContext) {
    let cursor = cx.new(|_| CursorBenchView {
        cursor_visible: true,
    });
    let mut window = cx.add_empty_window();
    window.update(|window, cx| {
        let cursor = cursor.clone();
        let row_count = *row_count;
        window.replace_root(cx, |_window, _cx| LargeViewWithCursorEntity {
            rows: bench_rows(row_count),
            render_text: false,
            cursor,
        });
    });
    cx.bench_renderer(cursor, |cursor, _window, cx| {
        cursor.cursor_visible = !cursor.cursor_visible;
        cx.notify();
    });
}

#[gpui::bench(
    inputs = row_counts(),
    group = "Blink cursor beside cached rows",
    input_name = "static rows",
    sample_size = 30
)]
fn blink_cursor_beside_cached_rows(row_count: &usize, cx: &mut BenchAppContext) {
    let cursor = cx.new(|_| CursorBenchView {
        cursor_visible: true,
    });
    let rows = cx.new(|_| StaticRowsBenchView {
        rows: bench_rows(*row_count),
    });
    let mut window = cx.add_empty_window();
    window.update(|window, cx| {
        let cursor = cursor.clone();
        let rows_height = px(*row_count as f32 * 24.);
        window.replace_root(cx, |_window, _cx| LargeViewWithCachedRows {
            rows,
            rows_height,
            cursor,
        });
    });
    cx.bench_renderer(cursor, |cursor, _window, cx| {
        cursor.cursor_visible = !cursor.cursor_visible;
        cx.notify();
    });
}

#[gpui::bench(
    inputs = row_counts(),
    group = "Blink cursor beside rows entity",
    input_name = "static rows",
    sample_size = 30
)]
fn blink_cursor_beside_rows_entity(row_count: &usize, cx: &mut BenchAppContext) {
    let cursor = cx.new(|_| CursorBenchView {
        cursor_visible: true,
    });
    let rows = cx.new(|_| StaticRowsBenchView {
        rows: bench_rows(*row_count),
    });
    let mut window = cx.add_empty_window();
    window.update(|window, cx| {
        let cursor = cursor.clone();
        window.replace_root(cx, |_window, _cx| LargeViewWithRowsEntity { rows, cursor });
    });
    cx.bench_renderer(cursor, |cursor, _window, cx| {
        cursor.cursor_visible = !cursor.cursor_visible;
        cx.notify();
    });
}

fn row_counts() -> [usize; 4] {
    [0, 10, 100, 1_000]
}

fn bench_rows(row_count: usize) -> Vec<SharedString> {
    (0..row_count)
        .map(|index| SharedString::from(format!("Static row {index}: The quick brown fox")))
        .collect()
}

struct EmptyBenchView;

impl Render for EmptyBenchView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

struct CursorBenchView {
    cursor_visible: bool,
}

impl Render for CursorBenchView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(2.))
            .h(px(20.))
            .when(self.cursor_visible, |cursor| cursor.bg(rgb(0x000000)))
    }
}

struct BlinkCursorBenchView {
    rows: Vec<SharedString>,
    cursor_visible: bool,
}

struct PaintGroupCursorBenchView {
    rows: Vec<SharedString>,
    cursor_group: PaintGroupId,
}

impl Render for PaintGroupCursorBenchView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let cursor_group = self.cursor_group;
        div()
            .size_full()
            .flex_col()
            .children(
                self.rows
                    .iter()
                    .map(|row| div().h(px(24.)).child(row.clone())),
            )
            .child(
                gpui::canvas(
                    |bounds, _, _| bounds,
                    move |bounds, _, window, _| {
                        window.paint_group(cursor_group, true, |window| {
                            window.paint_quad(gpui::fill(bounds, rgb(0x000000)));
                        });
                    },
                )
                .w(px(2.))
                .h(px(20.)),
            )
    }
}

struct BlinkCursorWithKeyedRowsBenchView {
    rows: Vec<SharedString>,
    cursor_visible: bool,
}

impl Render for BlinkCursorWithKeyedRowsBenchView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex_col()
            .children(
                self.rows
                    .iter()
                    .enumerate()
                    .map(|(index, row)| div().id(index).h(px(24.)).child(row.clone())),
            )
            .child(
                div()
                    .w(px(2.))
                    .h(px(20.))
                    .when(self.cursor_visible, |cursor| cursor.bg(rgb(0x000000))),
            )
    }
}

impl Render for BlinkCursorBenchView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex_col()
            .children(
                self.rows
                    .iter()
                    .map(|row| div().h(px(24.)).child(row.clone())),
            )
            .child(
                div()
                    .w(px(2.))
                    .h(px(20.))
                    .when(self.cursor_visible, |cursor| cursor.bg(rgb(0x000000))),
            )
    }
}

struct LargeViewWithCursorEntity {
    rows: Vec<SharedString>,
    render_text: bool,
    cursor: Entity<CursorBenchView>,
}

impl Render for LargeViewWithCursorEntity {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex_col()
            .children(self.rows.iter().map(|row| {
                div().h(px(24.)).when(self.render_text, |row_element| {
                    row_element.child(row.clone())
                })
            }))
            .child(self.cursor.clone())
    }
}

struct StaticRowsBenchView {
    rows: Vec<SharedString>,
}

impl Render for StaticRowsBenchView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().flex_col().children(
            self.rows
                .iter()
                .map(|row| div().h(px(24.)).child(row.clone())),
        )
    }
}

struct LargeViewWithCachedRows {
    rows: Entity<StaticRowsBenchView>,
    rows_height: gpui::Pixels,
    cursor: Entity<CursorBenchView>,
}

struct LargeViewWithRowsEntity {
    rows: Entity<StaticRowsBenchView>,
    cursor: Entity<CursorBenchView>,
}

impl Render for LargeViewWithRowsEntity {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex_col()
            .child(self.rows.clone())
            .child(self.cursor.clone())
    }
}

impl Render for LargeViewWithCachedRows {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex_col()
            .child(
                self.rows
                    .clone()
                    .cached(StyleRefinement::default().w_full().h(self.rows_height)),
            )
            .child(self.cursor.clone())
    }
}

gpui::bench_group!(
    benches,
    rerender_empty_view,
    present_unchanged_scene,
    blink_cursor_in_paint_group,
    blink_cursor_in_one_entity,
    blink_cursor_with_keyed_rows,
    blink_cursor_in_leaf_entity,
    blink_cursor_beside_empty_rows,
    blink_cursor_beside_cached_rows,
    blink_cursor_beside_rows_entity,
);
gpui::bench_main!(benches);
