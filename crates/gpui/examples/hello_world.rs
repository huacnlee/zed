use gpui::*;

struct HelloWorld {
    text: SharedString,
}

impl Render for HelloWorld {
    fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        div()
            .flex()
            .bg(gpui::white())
            .p_6()
            .size_full()
            .justify_center()
            .items_center()
            .child(
                div()
                    .p_6()
                    .rounded_lg()
                    .justify_center()
                    .items_center()
                    .text_xl()
                    .shadow_lg()
                    .border_4()
                    .bg(rgb(0x2563eb))
                    .border_color(rgb(0x1e3a8a))
                    .text_color(rgb(0xdbeafe))
                    .hover(|this| {
                        this.border_6()
                            .bg(rgb(0xdc2626))
                            .border_color(rgb(0x7f1d1d))
                            .text_color(rgb(0x7f1d1d))
                            .text_bg(rgb(0xfee2e2))
                            .font_weight(FontWeight::BOLD)
                            .text_2xl()
                            .text_decoration_2()
                            .text_decoration_wavy()
                            .text_decoration_color(rgb(0xfee2e2))
                    })
                    .child(format!("Hello, {}!", &self.text)),
            )
    }
}

fn main() {
    App::new().run(|cx: &mut AppContext| {
        let bounds = Bounds::centered(None, size(px(300.0), px(300.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |cx| {
                cx.new_view(|_cx| HelloWorld {
                    text: "World".into(),
                })
            },
        )
        .unwrap();
    });
}
