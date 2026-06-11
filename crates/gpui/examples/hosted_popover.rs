//! Renders GPUI content inside a native macOS `NSPopover`, using
//! `WindowOptions::host_window_handle`.
//!
//! The popover supplies the system chrome — arrow, vibrant backdrop, show
//! animation, and transient dismissal (click outside to close) — while its
//! content is an ordinary GPUI window: any element renders, and input works
//! exactly as in a regular window.
//!
//! Run with: `cargo run -p gpui --example hosted_popover` (macOS only).

#![cfg_attr(target_family = "wasm", no_main)]

#[cfg(target_os = "macos")]
mod example {
    use gpui::{
        App, Bounds, Context, FocusHandle, FontWeight, KeyBinding, MouseButton, Pixels,
        SharedString, Size, Window, WindowBackgroundAppearance, WindowBounds, WindowOptions,
        actions, canvas, div, point, prelude::*, px, rgb, rgba, size,
    };
    use std::cell::Cell;
    use std::rc::Rc;

    actions!(hosted_popover, [NewDocument, OpenRecent, Share]);

    const KEY_CONTEXT: &str = "HostedPopover";
    use gpui_platform::application;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSPopover, NSPopoverBehavior, NSView, NSViewController};
    use objc2_foundation::{NSPoint, NSRect, NSRectEdge, NSSize};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ptr::NonNull;
    use std::time::Duration;

    /// Content rendered inside the popover — a regular GPUI view, styled like a
    /// native quick-actions panel. No opaque background is painted, so the
    /// popover's vibrant backdrop shows through. The rows' keyboard shortcuts
    /// are real GPUI key bindings, demonstrating that hosted windows receive
    /// keyboard input.
    struct PopoverContent {
        focus_handle: FocusHandle,
        last_action: Option<&'static str>,
    }

    impl PopoverContent {
        fn trigger(&mut self, label: &'static str, cx: &mut Context<Self>) {
            self.last_action = Some(label);
            cx.notify();
        }

        fn row(
            &self,
            id: &'static str,
            swatch: u32,
            label: &'static str,
            shortcut: &'static str,
            cx: &mut Context<Self>,
        ) -> impl IntoElement {
            div()
                .id(SharedString::new_static(id))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .h(px(28.))
                .rounded_md()
                .hover(|style| style.bg(rgba(0x3b82f626)))
                .active(|style| style.bg(rgba(0x3b82f640)))
                .child(div().size(px(14.)).rounded_sm().bg(rgb(swatch)))
                .child(div().flex_1().text_size(px(13.)).child(label))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgba(0x3c3c4366))
                        .child(shortcut),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.trigger(label, cx)))
        }

        fn divider(&self) -> impl IntoElement {
            div().h(px(1.)).w_full().bg(rgba(0x3c3c431f))
        }
    }

    impl Render for PopoverContent {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .size_full()
                .text_color(rgba(0x000000d9))
                .key_context(KEY_CONTEXT)
                .track_focus(&self.focus_handle)
                .on_action(cx.listener(|this, _: &NewDocument, _, cx| {
                    this.trigger("New Document", cx)
                }))
                .on_action(
                    cx.listener(|this, _: &OpenRecent, _, cx| this.trigger("Open Recent", cx)),
                )
                .on_action(cx.listener(|this, _: &Share, _, cx| this.trigger("Share…", cx)))
                .child(
                    div()
                        .px_2()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Quick Actions"),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgba(0x3c3c4380))
                                .child("Rendered by GPUI inside an NSPopover"),
                        ),
                )
                .child(self.divider())
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(self.row("new", 0x3b82f6, "New Document", "⌘N", cx))
                        .child(self.row("open", 0x22c55e, "Open Recent", "⌘O", cx))
                        .child(self.row("share", 0xf59e0b, "Share…", "⇧⌘S", cx)),
                )
                .child(self.divider())
                .child(
                    div()
                        .px_2()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(12.))
                                .text_color(rgba(0x3c3c4380))
                                .child(match self.last_action {
                                    Some(label) => format!("Last action: {label}"),
                                    None => "Click a row or press a shortcut".to_string(),
                                }),
                        )
                        .child(
                            div()
                                .id("reset")
                                .px_2p5()
                                .py_1()
                                .rounded_md()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .bg(rgba(0x7878801f))
                                .text_color(rgba(0x000000d9))
                                .hover(|style| style.bg(rgba(0x78788033)))
                                .active(|style| style.bg(rgba(0x78788047)))
                                .child("Reset")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.last_action = None;
                                    cx.notify();
                                })),
                        ),
                )
        }
    }

    /// The main window: a trigger that opens the native popover.
    struct MainView {
        /// The trigger's bounds, captured at layout so the popover can anchor
        /// to the button itself.
        trigger_bounds: Rc<Cell<Bounds<Pixels>>>,
    }

    impl Render for MainView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let capture = self.trigger_bounds.clone();
            let anchor = self.trigger_bounds.clone();
            div()
                .flex()
                .flex_col()
                .gap_2()
                .size_full()
                .justify_center()
                .items_center()
                .bg(rgb(0xf5f5f7))
                .child(
                    div()
                        .relative()
                        .child(
                            div()
                                .id("trigger")
                                .px_4()
                                .py_2()
                                .rounded_lg()
                                .bg(rgb(0x1d1d1f))
                                .text_color(gpui::white())
                                .font_weight(FontWeight::MEDIUM)
                                .hover(|style| style.bg(rgb(0x2c2c2e)))
                                .active(|style| style.bg(rgb(0x3a3a3c)))
                                .child("Open native popover")
                                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                    open_popover(anchor.get(), window, cx);
                                }),
                        )
                        .child(
                            // Records the trigger's window-relative bounds.
                            canvas(move |bounds, _, _| capture.set(bounds), |_, _, _, _| {})
                                .absolute()
                                .inset_0(),
                        ),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgba(0x3c3c4366))
                        .child("System arrow & vibrancy — content rendered by GPUI"),
                )
        }
    }

    /// Shows an `NSPopover` anchored to `anchor` (the trigger's window-relative
    /// bounds) and opens a GPUI window hosted inside its content view.
    fn open_popover(anchor: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        // The popover is anchored to the triggering window's content view.
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::AppKit(parent) = handle.as_raw() else {
            return;
        };
        // SAFETY: the pointer comes from the live window's AppKit handle and is
        // only used synchronously while the window is alive.
        let parent_view: &NSView = unsafe { &*parent.ns_view.as_ptr().cast() };

        let content_size: Size<Pixels> = size(px(320.), px(208.));

        // Native popover shell: a plain container view in a view controller.
        // SAFETY: all objects are created and used on the main thread, and the
        // popover/controller/container are kept alive until the popover closes.
        let (popover, controller, container) = unsafe {
            let container = NSView::new(mtm);
            container.setFrameSize(NSSize::new(
                f32::from(content_size.width) as f64,
                f32::from(content_size.height) as f64,
            ));
            let controller = NSViewController::new(mtm);
            controller.setView(&container);

            let popover = NSPopover::new(mtm);
            popover.setBehavior(NSPopoverBehavior::Transient);
            popover.setContentViewController(Some(&controller));

            // GPUI's anchor bounds are top-left based; the (non-flipped)
            // positioning view is bottom-left based, so flip y against its
            // height.
            let parent_height = parent_view.bounds().size.height;
            let rect = NSRect::new(
                NSPoint::new(
                    f32::from(anchor.origin.x) as f64,
                    parent_height
                        - (f32::from(anchor.origin.y) + f32::from(anchor.size.height)) as f64,
                ),
                NSSize::new(
                    f32::from(anchor.size.width) as f64,
                    f32::from(anchor.size.height) as f64,
                ),
            );
            popover.showRelativeToRect_ofView_preferredEdge(rect, parent_view, NSRectEdge::MinY);
            (popover, controller, container)
        };

        // The container is now installed in the popover's window; render a GPUI
        // window into it.
        let container_ptr = NonNull::new(objc2::rc::Retained::as_ptr(&container) as *mut _)
            .expect("container view pointer is non-null");
        let gpui_window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(0.), px(0.)),
                        size: content_size,
                    })),
                    window_background: WindowBackgroundAppearance::Transparent,
                    host_window_handle: Some(RawWindowHandle::AppKit(
                        raw_window_handle::AppKitWindowHandle::new(container_ptr),
                    )),
                    ..Default::default()
                },
                |window, cx| {
                    let content = cx.new(|cx| PopoverContent {
                        focus_handle: cx.focus_handle(),
                        last_action: None,
                    });
                    let focus_handle = content.read(cx).focus_handle.clone();
                    window.focus(&focus_handle, cx);
                    content
                },
            )
            .expect("failed to open hosted window");

        // Let the popover's window receive keyboard input immediately, so the
        // shortcuts work without clicking inside first.
        // SAFETY: main thread; the container was just installed in the
        // popover's window by `show...` above.
        unsafe {
            if let Some(popover_window) = container.window() {
                popover_window.makeKeyWindow();
            }
        }

        // When the popover is dismissed (e.g. by clicking outside), close the
        // hosted GPUI window. Production code would use an `NSPopoverDelegate`;
        // polling keeps this example small.
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                // SAFETY: checked on the main thread; the popover is kept alive
                // by this future.
                if !unsafe { popover.isShown() } {
                    break;
                }
            }
            gpui_window
                .update(cx, |_, window, _| window.remove_window())
                .ok();
            drop((popover, controller, container));
        })
        .detach();
    }

    pub fn run() {
        application().run(|cx: &mut App| {
            cx.bind_keys([
                KeyBinding::new("cmd-n", NewDocument, Some(KEY_CONTEXT)),
                KeyBinding::new("cmd-o", OpenRecent, Some(KEY_CONTEXT)),
                KeyBinding::new("shift-cmd-s", Share, Some(KEY_CONTEXT)),
            ]);
            let bounds = Bounds::centered(None, size(px(500.), px(320.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|_| MainView {
                        trigger_bounds: Rc::new(Cell::new(Bounds::default())),
                    })
                },
            )
            .unwrap();
            cx.activate(true);
        });
    }
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    #[cfg(target_os = "macos")]
    example::run();
    #[cfg(not(target_os = "macos"))]
    println!("This example demonstrates hosting GPUI inside an NSPopover and is macOS-only.");
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    println!("This example is macOS-only.");
}
