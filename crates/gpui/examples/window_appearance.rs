//! Window appearance demo.
//!
//! Run with: `cargo run -p gpui --example window_appearance`
//!
//! This app demonstrates [`Window::set_appearance`], which overrides the native
//! window chrome (the 1px window border and the titlebar) to be light or dark
//! independent of the OS-wide setting.
//!
//! To see the effect on macOS: set the system to Light mode, then click "Dark".
//! The window's border and titlebar should switch to dark to match a dark theme,
//! instead of staying light. Click "Auto" to follow the system again.

#![cfg_attr(target_family = "wasm", no_main)]

use gpui::{
    App, AppearanceMode, Bounds, Context, Rgba, Window, WindowAppearance, WindowBounds,
    WindowOptions, div, prelude::*, px, rgb, size,
};
use gpui_platform::application;

/// A palette whose colors switch together so the whole UI re-themes when the
/// appearance changes.
struct Palette {
    bg: Rgba,
    fg: Rgba,
    muted: Rgba,
    accent: Rgba,
    accent_fg: Rgba,
    control: Rgba,
}

impl Palette {
    fn new(is_dark: bool) -> Self {
        if is_dark {
            Self {
                bg: rgb(0x1e1e1e),
                fg: rgb(0xf4f4f5),
                muted: rgb(0x9a9a9a),
                accent: rgb(0x0059d1),
                accent_fg: rgb(0xffffff),
                control: rgb(0x2f2f2f),
            }
        } else {
            Self {
                bg: rgb(0xffffff),
                fg: rgb(0x18181b),
                muted: rgb(0x6a6a72),
                accent: rgb(0x0076f7),
                accent_fg: rgb(0xffffff),
                control: rgb(0xe4e4e4),
            }
        }
    }
}

struct AppearanceExample {
    mode: AppearanceMode,
}

impl AppearanceExample {
    fn button(
        &self,
        label: &'static str,
        mode: AppearanceMode,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.mode == mode;
        let (bg, fg) = if selected {
            (palette.accent, palette.accent_fg)
        } else {
            (palette.control, palette.fg)
        };

        div()
            .id(label)
            .flex()
            .items_center()
            .justify_center()
            .px_4()
            .py_1()
            .text_sm()
            .rounded_md()
            .cursor_pointer()
            .bg(bg)
            .text_color(fg)
            .child(label)
            .on_click(cx.listener(move |this, _event, window, cx| {
                this.mode = mode;
                window.set_appearance(mode);
                cx.notify();
            }))
    }
}

impl Render for AppearanceExample {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let appearance = window.appearance();
        // Theme the UI from the selected mode so the content re-themes immediately on
        // click; in `Auto` mode, follow the system's effective appearance.
        let is_dark = match self.mode {
            AppearanceMode::Light => false,
            AppearanceMode::Dark => true,
            AppearanceMode::Auto => {
                matches!(
                    appearance,
                    WindowAppearance::Dark | WindowAppearance::VibrantDark
                )
            }
        };
        let palette = Palette::new(is_dark);

        div()
            .flex()
            .size_full()
            .justify_center()
            .items_center()
            .bg(palette.bg)
            .text_color(palette.fg)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w(px(340.))
                    .p_6()
                    .child(div().text_xl().child("Window Appearance"))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .text_sm()
                            .text_color(palette.muted)
                            .child(format!("Selected mode: {:?}", self.mode))
                            .child(format!("Effective appearance: {appearance:?}")),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.button("Auto", AppearanceMode::Auto, &palette, cx))
                            .child(self.button("Light", AppearanceMode::Light, &palette, cx))
                            .child(self.button("Dark", AppearanceMode::Dark, &palette, cx)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette.muted)
                            .child(
                                "Set the system to Light mode, then choose Dark: the native \
                                 window border and titlebar switch to dark to match.",
                            ),
                    ),
            )
    }
}

fn run_example() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(440.), px(380.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    // Re-render when the effective appearance changes, so the labels
                    // stay accurate while in `Auto` mode and the system theme toggles.
                    cx.observe_window_appearance(window, |_, _, cx| {
                        cx.notify();
                    })
                    .detach();
                    AppearanceExample {
                        mode: AppearanceMode::Auto,
                    }
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    run_example();
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    gpui_platform::web_init();
    run_example();
}
