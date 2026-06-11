//! Embeds a GPUI window inside a window created with the platform's native
//! APIs, using `WindowOptions::host_window_handle` — the building block for
//! adopting GPUI incrementally inside an existing native application.
//!
//! The window itself, its title bar, and the label at the top are plain
//! AppKit; the interactive area below is rendered and driven entirely by GPUI.
//!
//! Run with: `cargo run -p gpui --example hosted_window` (macOS today;
//! Windows/X11 hosts work the same way through `host_window_handle`).

#![cfg_attr(target_family = "wasm", no_main)]

#[cfg(target_os = "macos")]
mod example {
    use gpui::{
        App, Bounds, Context, Window, WindowBounds, WindowOptions, div, point, prelude::*, px,
        rgb, size,
    };
    use gpui_platform::application;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSBackingStoreType, NSTextField, NSView, NSWindow,
        NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use raw_window_handle::{AppKitWindowHandle, RawWindowHandle};
    use std::ptr::NonNull;

    const WIDTH: f64 = 560.;
    const HEIGHT: f64 = 400.;
    const HEADER: f64 = 48.;

    /// The GPUI content embedded in the native window.
    struct Embedded {
        clicks: usize,
    }

    impl Render for Embedded {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .gap_3()
                .p_4()
                .size_full()
                .bg(rgb(0x14161b))
                .text_color(rgb(0xe6e6e6))
                .child(div().text_xl().child("This area is GPUI"))
                .child(div().text_sm().text_color(rgb(0x9a9a9a)).child(
                    "Rendered into a plain NSView of an AppKit window via \
                     WindowOptions::host_window_handle.",
                ))
                .child(
                    div()
                        .id("counter")
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(0x3b82f6))
                        .text_color(gpui::white())
                        .child(format!("Clicked {} times", self.clicks))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.clicks += 1;
                            cx.notify();
                        })),
                )
                .child(
                    div().flex().gap_2().children((0..6).map(|i| {
                        div()
                            .id(i)
                            .size_8()
                            .rounded_md()
                            .bg(rgb(0x2b2f3a))
                            .hover(|style| style.bg(rgb(0x3b82f6)))
                    })),
                )
        }
    }

    pub fn run() {
        application().run(|cx: &mut App| {
            let mtm = MainThreadMarker::new().expect("must run on the main thread");

            // --- Plain AppKit: a native window with a native label. ---
            // SAFETY: all objects are created and used on the main thread and
            // are kept alive for the lifetime of the process (see the
            // `mem::forget` below).
            let (native_window, host_view, label) = unsafe {
                let rect = NSRect::new(NSPoint::new(200., 200.), NSSize::new(WIDTH, HEIGHT));
                let style = NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable;
                let native_window = NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(mtm),
                    rect,
                    style,
                    NSBackingStoreType::Buffered,
                    false,
                );
                native_window.setTitle(&NSString::from_str("Native AppKit window"));
                let content_view = native_window
                    .contentView()
                    .expect("native window has a content view");

                let label = NSTextField::labelWithString(
                    &NSString::from_str("This label and window are plain AppKit ↓ below is GPUI"),
                    mtm,
                );
                label.setFrame(NSRect::new(
                    NSPoint::new(16., HEIGHT - HEADER + 14.),
                    NSSize::new(WIDTH - 32., 20.),
                ));
                content_view.addSubview(&label);

                // The host view GPUI renders into: the area below the header.
                let host_view = NSView::new(mtm);
                host_view.setFrame(NSRect::new(
                    NSPoint::new(0., 0.),
                    NSSize::new(WIDTH, HEIGHT - HEADER),
                ));
                host_view.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewWidthSizable
                        | NSAutoresizingMaskOptions::ViewHeightSizable,
                );
                content_view.addSubview(&host_view);

                native_window.makeKeyAndOrderFront(None);
                (native_window, host_view, label)
            };

            // --- GPUI: render into the host view. ---
            let host_ptr = NonNull::new(objc2::rc::Retained::as_ptr(&host_view) as *mut _)
                .expect("host view pointer is non-null");
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(0.), px(0.)),
                        size: size(px(WIDTH as f32), px((HEIGHT - HEADER) as f32)),
                    })),
                    host_window_handle: Some(RawWindowHandle::AppKit(AppKitWindowHandle::new(
                        host_ptr,
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| Embedded { clicks: 0 }),
            )
            .expect("failed to open hosted window");

            // The native window lives for the rest of the process.
            std::mem::forget((native_window, host_view, label));

            cx.activate(true);
        });
    }
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    #[cfg(target_os = "macos")]
    example::run();
    #[cfg(not(target_os = "macos"))]
    println!(
        "This example currently builds its native host with AppKit and is macOS-only; \
         Windows/X11 hosts work the same way through WindowOptions::host_window_handle."
    );
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    println!("This example is macOS-only.");
}
