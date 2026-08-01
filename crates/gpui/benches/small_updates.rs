use gpui::{
    BenchAppContext, Context, Entity, IntoElement, Render, SharedString, StyleRefinement, Window,
    div, prelude::*, px, rgb,
};

#[gpui::bench(inputs = row_counts(), group = "Blink cursor in root", input_name = "rows")]
fn blink_cursor_in_root(row_count: &usize, cx: &mut BenchAppContext) {
    let mut window = cx.add_empty_window();
    let view = window.update(|window, cx| {
        window.replace_root(cx, |_window, _cx| RootView {
            rows: bench_rows(*row_count),
            cursor_visible: true,
        })
    });
    cx.bench_renderer(view, |view, _window, cx| {
        view.cursor_visible = !view.cursor_visible;
        cx.notify();
    });
}

#[gpui::bench(inputs = row_counts(), group = "Blink cursor in leaf", input_name = "rows")]
fn blink_cursor_in_leaf(row_count: &usize, cx: &mut BenchAppContext) {
    let cursor = cx.new(|_| CursorView {
        cursor_visible: true,
    });
    let mut window = cx.add_empty_window();
    window.update(|window, cx| {
        let cursor = cursor.clone();
        let rows = bench_rows(*row_count);
        window.replace_root(cx, |_window, _cx| RowsWithCursor { rows, cursor });
    });
    cx.bench_renderer(cursor, |cursor, _window, cx| {
        cursor.cursor_visible = !cursor.cursor_visible;
        cx.notify();
    });
}

#[gpui::bench(inputs = row_counts(), group = "Blink cursor beside rows entity", input_name = "rows")]
fn blink_cursor_beside_rows_entity(row_count: &usize, cx: &mut BenchAppContext) {
    let cursor = cx.new(|_| CursorView {
        cursor_visible: true,
    });
    let rows = cx.new(|_| RowsView {
        rows: bench_rows(*row_count),
    });
    let mut window = cx.add_empty_window();
    window.update(|window, cx| {
        let cursor = cursor.clone();
        let rows = rows.clone();
        window.replace_root(cx, |_window, _cx| RowsEntityWithCursor { rows, cursor });
    });
    cx.bench_renderer(cursor, |cursor, _window, cx| {
        cursor.cursor_visible = !cursor.cursor_visible;
        cx.notify();
    });
}

#[gpui::bench(inputs = row_counts(), group = "Blink cursor beside cached rows", input_name = "rows")]
fn blink_cursor_beside_cached_rows(row_count: &usize, cx: &mut BenchAppContext) {
    let cursor = cx.new(|_| CursorView {
        cursor_visible: true,
    });
    let rows = cx.new(|_| RowsView {
        rows: bench_rows(*row_count),
    });
    let rows_height = px(*row_count as f32 * 24.);
    let mut window = cx.add_empty_window();
    window.update(|window, cx| {
        let cursor = cursor.clone();
        let rows = rows.clone();
        window.replace_root(cx, |_window, _cx| CachedRowsWithCursor {
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

#[gpui::bench(inputs = row_counts(), group = "Blink cursor beside empty rows", input_name = "rows")]
fn blink_cursor_beside_empty_rows(row_count: &usize, cx: &mut BenchAppContext) {
    let cursor = cx.new(|_| CursorView {
        cursor_visible: true,
    });
    let rows = cx.new(|_| EmptyRowsView {
        row_count: *row_count,
    });
    let mut window = cx.add_empty_window();
    window.update(|window, cx| {
        let cursor = cursor.clone();
        let rows = rows.clone();
        window.replace_root(cx, |_window, _cx| EmptyRowsWithCursor { rows, cursor });
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

struct CursorView {
    cursor_visible: bool,
}

impl Render for CursorView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(2.))
            .h(px(20.))
            .when(self.cursor_visible, |cursor| cursor.bg(rgb(0x000000)))
    }
}

struct RootView {
    rows: Vec<SharedString>,
    cursor_visible: bool,
}

impl Render for RootView {
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

struct RowsView {
    rows: Vec<SharedString>,
}

impl Render for RowsView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().flex_col().children(
            self.rows
                .iter()
                .map(|row| div().h(px(24.)).child(row.clone())),
        )
    }
}

struct EmptyRowsView {
    row_count: usize,
}

impl Render for EmptyRowsView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_col()
            .children((0..self.row_count).map(|_| div().h(px(24.))))
    }
}

struct RowsWithCursor {
    rows: Vec<SharedString>,
    cursor: Entity<CursorView>,
}

impl Render for RowsWithCursor {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex_col()
            .children(
                self.rows
                    .iter()
                    .map(|row| div().h(px(24.)).child(row.clone())),
            )
            .child(self.cursor.clone())
    }
}

struct RowsEntityWithCursor {
    rows: Entity<RowsView>,
    cursor: Entity<CursorView>,
}

impl Render for RowsEntityWithCursor {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex_col()
            .child(self.rows.clone())
            .child(self.cursor.clone())
    }
}

struct EmptyRowsWithCursor {
    rows: Entity<EmptyRowsView>,
    cursor: Entity<CursorView>,
}

impl Render for EmptyRowsWithCursor {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex_col()
            .child(self.rows.clone())
            .child(self.cursor.clone())
    }
}

struct CachedRowsWithCursor {
    rows: Entity<RowsView>,
    rows_height: gpui::Pixels,
    cursor: Entity<CursorView>,
}

impl Render for CachedRowsWithCursor {
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
    blink_cursor_in_root,
    blink_cursor_in_leaf,
    blink_cursor_beside_rows_entity,
    blink_cursor_beside_cached_rows,
    blink_cursor_beside_empty_rows,
);
gpui::bench_main!(benches);
