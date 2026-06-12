//! Native macOS window tabs hosting GPU-rendered GPUI surfaces, built on
//! `WindowOptions::host_window_handle` — the building block for adopting GPUI
//! incrementally inside an existing native application.
//!
//! The contrast is the point: the windows, the system tab bar (the same
//! native tabs Terminal, Safari, or Ghostty use), and the search field are
//! plain AppKit — while everything inside each tab is a hosted GPUI window
//! animating at the display's refresh rate: gradient-lit metric cards, a live
//! equalizer chart, layered showcase tiles. Content AppKit views don't draw,
//! inside windows AppKit owns.
//!
//! Every seam between the two worlds is wired up:
//!
//! - Two native windows are merged into one tab group — switch tabs natively,
//!   or with ⌘1/⌘2: GPUI key bindings that select the *native* tab.
//! - The native search field's text streams into GPUI live, filtering the
//!   metric cards.
//! - "Details ▾" opens a system `NSPopover` (arrow, vibrancy, transient
//!   dismissal) hosting another GPUI window — with a working ⏎ key binding.
//!
//! Run with: `cargo run -p gpui --example hosted_window` (macOS today;
//! Windows/X11 hosts work the same way through `host_window_handle`).

#![cfg_attr(target_family = "wasm", no_main)]

#[cfg(target_os = "macos")]
mod example {
    use gpui::{
        Animation, AnimationExt as _, App, Bounds, Context, FocusHandle, FontWeight, KeyBinding,
        Pixels, Window, WindowBackgroundAppearance, WindowBounds, WindowOptions, actions, canvas,
        div, linear_color_stop, linear_gradient, point, prelude::*, px, rgb, rgba, size,
    };
    use gpui_platform::application;
    use objc2::rc::Retained;
    use objc2::runtime::NSObject;
    use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{
        NSBackingStoreType, NSControl, NSPopover, NSPopoverBehavior, NSSearchField, NSView,
        NSViewController, NSWindow, NSWindowOrderingMode, NSWindowStyleMask,
    };
    use objc2_foundation::{NSNotification, NSPoint, NSRect, NSRectEdge, NSSize, NSString};
    use raw_window_handle::{AppKitWindowHandle, HasWindowHandle, RawWindowHandle};
    use std::cell::Cell;
    use std::ptr::NonNull;
    use std::rc::Rc;
    use std::sync::mpsc;
    use std::time::Duration;

    actions!(hosted_window, [SelectTab1, SelectTab2, DismissInfo, ToggleFeature]);

    const APP_CONTEXT: &str = "HostedDemo";
    const POPOVER_CONTEXT: &str = "InfoPopover";

    const WIDTH: f64 = 760.;
    const HEIGHT: f64 = 520.;

    // ------------------------------------------------------------------------
    // AppKit glue: the search-field delegate.
    // ------------------------------------------------------------------------

    /// Ivars for [`SearchHandler`]: forwards the native field's text.
    struct SearchHandlerIvars {
        tx: mpsc::Sender<String>,
    }

    define_class!(
        // Delegate of the native NSSearchField: forwards every text change to
        // the GPUI side over a channel — native input driving GPUI state.
        #[unsafe(super(NSObject))]
        #[name = "HostedWindowSearchHandler"]
        #[ivars = SearchHandlerIvars]
        struct SearchHandler;

        impl SearchHandler {
            #[unsafe(method(controlTextDidChange:))]
            fn control_text_did_change(&self, notification: &NSNotification) {
                let text = unsafe {
                    notification
                        .object()
                        .and_then(|object| object.downcast::<NSControl>().ok())
                        .map(|control| control.stringValue().to_string())
                };
                if let Some(text) = text {
                    let _ = self.ivars().tx.send(text);
                }
            }
        }
    );

    impl SearchHandler {
        fn new(tx: mpsc::Sender<String>) -> Retained<Self> {
            let this = Self::alloc().set_ivars(SearchHandlerIvars { tx });
            unsafe { msg_send![super(this), init] }
        }
    }

    // ------------------------------------------------------------------------
    // GPUI: the info popover content.
    // ------------------------------------------------------------------------

    /// Info panel hosted inside a native `NSPopover`.
    /// Demonstrates click, hover, and keyboard input inside a hosted window.
    struct InfoPopover {
        focus_handle: FocusHandle,
        enter_pressed: bool,
        toggle_on: bool,
        hover_count: u32,
    }

    impl Render for InfoPopover {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let toggle_on = self.toggle_on;
            let hover_count = self.hover_count;

            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .size_full()
                .text_color(rgb(0x09090b))
                .key_context(POPOVER_CONTEXT)
                .track_focus(&self.focus_handle)
                .on_action(cx.listener(|this, _: &DismissInfo, _, cx| {
                    this.enter_pressed = true;
                    cx.notify();
                }))
                .on_action(cx.listener(|this, _: &ToggleFeature, _, cx| {
                    this.toggle_on = !this.toggle_on;
                    cx.notify();
                }))
                // header
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("NSPopover + GPUI"),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(0x52525b))
                                .child("⏎ · ⌘K"),
                        ),
                )
                // description
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(0x52525b))
                        .child("Arrow, vibrancy & transient dismissal are AppKit. Interaction below is GPUI."),
                )
                // interactive row
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .mt_1()
                        // click button
                        .child(
                            div()
                                .id("popover-btn")
                                .flex()
                                .items_center()
                                .px_2p5()
                                .py_1()
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(0xe4e4e7))
                                .bg(rgb(0xfafafa))
                                .text_size(px(12.))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(0xf4f4f5)).border_color(rgb(0xd4d4d8)))
                                .active(|s| s.bg(rgb(0xe4e4e7)))
                                .on_mouse_move(cx.listener(|this, _, _, cx| {
                                    this.hover_count += 1;
                                    cx.notify();
                                }))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_on = !this.toggle_on;
                                    cx.notify();
                                }))
                                .child(if toggle_on { "● On" } else { "○ Off" }),
                        )
                        // ⌘K toggle
                        .child(
                            div()
                                .id("popover-cmd-k")
                                .flex()
                                .items_center()
                                .px_2p5()
                                .py_1()
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(0xe4e4e7))
                                .bg(rgb(0xfafafa))
                                .text_size(px(12.))
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(0xf4f4f5)))
                                .active(|s| s.bg(rgb(0xe4e4e7)))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.enter_pressed = !this.enter_pressed;
                                    cx.notify();
                                }))
                                .child(if self.enter_pressed { "⌘K ✓" } else { "⌘K —" }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(11.))
                                .text_color(rgb(0x71717a))
                                .text_align(gpui::TextAlign::Right)
                                .child(format!("hover moves: {hover_count}")),
                        ),
                )
                // status row
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_size(px(11.))
                        .text_color(rgb(0x71717a))
                        .child(
                            div()
                                .w(px(6.))
                                .h(px(6.))
                                .rounded_full()
                                .bg(if toggle_on { rgb(CHART_2) } else { rgb(0xd4d4d8) }),
                        )
                        .child(if toggle_on { "feature enabled" } else { "feature disabled" })
                        .child(div().flex_1())
                        .child(if self.enter_pressed {
                            "⏎ received ✓"
                        } else {
                            "press ⏎ to confirm"
                        }),
                )
        }
    }

    /// Shows an `NSPopover` anchored to `anchor` (bounds within the hosting
    /// GPUI window) and hosts an [`InfoPopover`] GPUI window inside it.
    fn open_info_popover(anchor: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::AppKit(parent) = handle.as_raw() else {
            return;
        };
        // SAFETY: the pointer comes from the live window's AppKit handle and is
        // only used synchronously while the window is alive.
        let parent_view: &NSView = unsafe { &*parent.ns_view.as_ptr().cast() };

        let content_size = size(px(280.), px(124.));

        // SAFETY: main thread; the popover objects are kept alive by the
        // dismissal task below.
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

            // GPUI bounds are top-left based; the (non-flipped) view is
            // bottom-left based, so flip y against its height.
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

        let container_ptr = NonNull::new(Retained::as_ptr(&container) as *mut _)
            .expect("container view pointer is non-null");
        let gpui_window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(0.), px(0.)),
                        size: content_size,
                    })),
                    window_background: WindowBackgroundAppearance::Transparent,
                    host_window_handle: Some(RawWindowHandle::AppKit(AppKitWindowHandle::new(
                        container_ptr,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    let content = cx.new(|cx| InfoPopover {
                        focus_handle: cx.focus_handle(),
                        enter_pressed: false,
                        toggle_on: false,
                        hover_count: 0,
                    });
                    let focus_handle = content.read(cx).focus_handle.clone();
                    window.focus(&focus_handle, cx);
                    content
                },
            )
            .expect("failed to open hosted popover window");

        // Let the popover receive keyboard input immediately.
        // SAFETY: main thread; the container was installed by `show...` above.
        unsafe {
            if let Some(popover_window) = container.window() {
                popover_window.makeKeyWindow();
            }
        }

        // When the popover is dismissed, close the hosted GPUI window.
        // Production code would use an `NSPopoverDelegate`; polling keeps this
        // example small.
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

    // ------------------------------------------------------------------------
    // GPUI: the GPU-rendered tab contents.
    // ------------------------------------------------------------------------

    struct Metric {
        label: &'static str,
        value: &'static str,
        accent: u32,
    }

    // The shadcn default chart palette.
    const CHART_1: u32 = 0xe76e50;
    const CHART_2: u32 = 0x2a9d90;
    const CHART_4: u32 = 0xe8c468;

    const METRICS: [Metric; 3] = [
        Metric {
            label: "Frame time",
            value: "8.3 ms",
            accent: CHART_2,
        },
        Metric {
            label: "Draw calls",
            value: "1,284",
            accent: CHART_1,
        },
        Metric {
            label: "Layers",
            value: "96",
            accent: CHART_4,
        },
    ];

    #[derive(Clone, Copy, PartialEq)]
    enum Pane {
        Dashboard,
        Showcase,
    }

    struct DemoApp {
        focus_handle: FocusHandle,
        pane: Pane,
        /// Mirrors the native NSSearchField in real time; filters the cards.
        query: String,
        /// Both native windows of the tab group, for ⌘1/⌘2 (GPUI key bindings
        /// selecting the *native* tab).
        tab_windows: (usize, usize),
        info_anchor: Rc<Cell<Bounds<Pixels>>>,
    }

    /// A small outline badge (shadcn-style) labelling which UI stack draws a
    /// region, with a colored dot.
    fn stack_badge(color: u32, label: &'static str) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .px_2()
            .py_0p5()
            .rounded_md()
            .border_1()
            .border_color(rgb(0xe4e4e7))
            .bg(gpui::white())
            .text_size(px(11.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(0x52525b))
            .child(div().size(px(6.)).rounded_full().bg(rgb(color)))
            .child(label)
    }

    impl DemoApp {
        /// Select a native window tab from a GPUI key binding (GPUI → AppKit
        /// control).
        fn select_tab(&mut self, index: usize, _cx: &mut Context<Self>) {
            let window = if index == 0 {
                self.tab_windows.0
            } else {
                self.tab_windows.1
            } as *const NSWindow;
            if !window.is_null() {
                // SAFETY: main thread; both windows outlive the process (they
                // are forgotten in `run`).
                unsafe {
                    (*window).makeKeyAndOrderFront(None);
                }
            }
        }

        fn metric_card(&self, metric: &Metric) -> impl IntoElement {
            let dimmed = !self.query.is_empty()
                && !metric
                    .label
                    .to_lowercase()
                    .contains(&self.query.to_lowercase());
            div()
                .flex_1()
                .h(px(92.))
                .rounded_lg()
                .border_1()
                .border_color(rgb(0xe4e4e7))
                .bg(gpui::white())
                .p_4()
                .flex()
                .flex_col()
                .justify_between()
                
                .opacity(if dimmed { 0.35 } else { 1. })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(div().size(px(8.)).rounded_full().bg(rgb(metric.accent)))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(0x52525b))
                                .child(metric.label),
                        ),
                )
                .child(
                    div()
                        .text_size(px(24.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(0x09090b))
                        .child(metric.value),
                )
        }

        /// A live equalizer: every bar animates continuously, driven by GPUI's
        /// animation system at the display refresh rate.
        fn equalizer(&self) -> impl IntoElement {
            div()
                .h(px(180.))
                .rounded_lg()
                .border_1()
                .border_color(rgb(0xe4e4e7))
                .bg(gpui::white())
                
                .p_4()
                .flex()
                .items_end()
                .gap(px(5.))
                .children((0..44usize).map(|i| {
                    div()
                        .flex_1()
                        .rounded_sm()
                        .bg(rgb(0x09090b))
                        .with_animation(
                            ("bar", i),
                            Animation::new(Duration::from_millis(2400)).repeat(),
                            move |bar, delta| {
                                let phase = delta * std::f32::consts::TAU;
                                let wave = (phase + i as f32 * 0.45).sin() * 0.5 + 0.5;
                                let ripple = (phase * 2. + i as f32 * 0.9).cos() * 0.5 + 0.5;
                                let level = 0.15 + 0.85 * (0.65 * wave + 0.35 * ripple);
                                bar.h(px(8. + 140. * level))
                            },
                        )
                }))
        }

        fn pane_dashboard(&self, _cx: &mut Context<Self>) -> gpui::AnyElement {
            let capture = self.info_anchor.clone();
            let anchor = self.info_anchor.clone();
            div()
                .flex()
                .flex_col()
                .gap_3()
                // Title row; the native search field floats over its right end.
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Live Metrics"),
                        )
                        .child({
                            div()
                                .relative()
                                .child(
                                    div()
                                        .id("details")
                                        .px_2p5()
                                        .py_0p5()
                                        .rounded_md()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .bg(rgb(0xf4f4f5))
                                        .text_color(rgb(0x18181b))
                                        .hover(|style| style.bg(rgb(0xe4e4e7)))
                                        .active(|style| style.bg(rgb(0xd4d4d8)))
                                        .child("Details ▾")
                                        .on_click(move |_, window, cx| {
                                            open_info_popover(anchor.get(), window, cx);
                                        }),
                                )
                                .child(
                                    canvas(
                                        move |bounds, _, _| capture.set(bounds),
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .inset_0(),
                                )
                        })
                        .child(div().flex_1())
                        // Reserved for the native search field floating above.
                        .child(div().w(px(200.)).h(px(26.))),
                )
                // Annotation row: what is AppKit, what is GPUI.
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(stack_badge(CHART_1, "▲ system tab bar — AppKit"))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(11.))
                                .text_color(rgb(0x71717a))
                                .child(if self.query.is_empty() {
                                    "⌘1/⌘2 switch the native tabs via GPUI key bindings."
                                        .to_string()
                                } else {
                                    format!("native search → GPUI filter: “{}”", self.query)
                                }),
                        )
                        .child(
                            div()
                                .w(px(200.))
                                .flex()
                                .justify_center()
                                .child(stack_badge(CHART_1, "NSSearchField — AppKit ▲")),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .children(METRICS.iter().map(|metric| self.metric_card(metric))),
                )
                .child(self.equalizer())
                .into_any_element()
        }

        fn pane_showcase(&self, _cx: &mut Context<Self>) -> gpui::AnyElement {
            fn gpui_badge() -> impl IntoElement {
                div()
                    .flex()
                    .items_center()
                    .px_1p5()
                    .py_0p5()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(0xe4e4e7))
                    .text_size(px(9.))
                    .text_color(rgb(0xa1a1aa))
                    .child("GPUI")
            }

            fn card(index: usize, label: &'static str, content: impl IntoElement) -> impl IntoElement {
                div()
                    .id(index)
                    .flex_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0xe4e4e7))
                    .bg(gpui::white())
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(content)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .mt_auto()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(0xa1a1aa))
                                    .child(label),
                            )
                            .child(gpui_badge()),
                    )
            }

            let gradient_card = card(
                0,
                "Gradient",
                div().h(px(56.)).rounded_md().bg(linear_gradient(
                    145.,
                    linear_color_stop(rgb(0x3b82f6), 0.),
                    linear_color_stop(rgb(0x6366f1), 1.),
                )),
            )
            .into_any_element();

            let text_items: [(&str, &str, FontWeight, Pixels); 5] = [
                ("Semibold", "The quick brown fox", FontWeight::SEMIBOLD, px(14.)),
                ("Medium", "jumps over the lazy dog", FontWeight::MEDIUM, px(13.)),
                ("Normal", "0123456789 — !@#$%", FontWeight::NORMAL, px(12.)),
                ("Light", "Aa Bb Cc Dd Ee Ff Gg", FontWeight::LIGHT, px(12.)),
                ("Bold", "GPUI renders text", FontWeight::BOLD, px(15.)),
            ];

            let mut text_cards: Vec<gpui::AnyElement> = text_items
                .iter()
                .enumerate()
                .map(|(i, (label, sample, weight, size))| {
                    card(
                        i + 1,
                        label,
                        div()
                            .text_size(*size)
                            .font_weight(*weight)
                            .text_color(rgb(0x09090b))
                            .child(*sample),
                    )
                    .into_any_element()
                })
                .collect();

            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .child(gradient_card)
                        .child(text_cards.remove(0))
                        .child(text_cards.remove(0)),
                )
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .child(text_cards.remove(0))
                        .child(text_cards.remove(0))
                        .child(text_cards.remove(0)),
                )
                .child(
                    // Staggered activity bars — GPUI per-element animation at display rate.
                    div()
                        .h(px(40.))
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(0xe4e4e7))
                        .bg(gpui::white())
                        .px_4()
                        .flex()
                        .gap(px(4.))
                        .items_center()
                        .children((0..18usize).map(|i| {
                            div()
                                .w(px(6.))
                                .rounded_sm()
                                .bg(rgb(0x09090b))
                                .with_animation(
                                    ("act", i),
                                    Animation::new(Duration::from_millis(1200)).repeat(),
                                    move |bar, delta| {
                                        let phase = delta * std::f32::consts::TAU;
                                        let h = 6.
                                            + 18.
                                                * ((phase + i as f32 * 0.55).sin() * 0.5 + 0.5)
                                                    .powf(2.0);
                                        bar.h(px(h))
                                    },
                                )
                        })),
                )
                .into_any_element()
        }
    }

    impl Render for DemoApp {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .relative()
                .flex()
                .flex_col()
                .size_full()
                .bg(gpui::white())
                .p_6()
                .text_color(rgb(0x09090b))
                .key_context(APP_CONTEXT)
                .track_focus(&self.focus_handle)
                .on_action(cx.listener(|this, _: &SelectTab1, _, cx| this.select_tab(0, cx)))
                .on_action(cx.listener(|this, _: &SelectTab2, _, cx| this.select_tab(1, cx)))
                .child(match self.pane {
                    Pane::Dashboard => self.pane_dashboard(cx),
                    Pane::Showcase => self.pane_showcase(cx),
                })
                .child(
                    div()
                        .absolute()
                        .bottom_3()
                        .right_3()
                        .child(stack_badge(CHART_2, "this whole pane — GPUI")),
                )
        }
    }

    // ------------------------------------------------------------------------
    // The native shell: two windows merged into one native tab group.
    // ------------------------------------------------------------------------

    /// Builds one native window with a hosted GPUI surface, returning the
    /// window and the GPUI view's handle for later wiring.
    fn build_tab_window(
        cx: &mut App,
        mtm: MainThreadMarker,
        title: &str,
        pane: Pane,
        with_search: Option<mpsc::Sender<String>>,
    ) -> (Retained<NSWindow>, gpui::WindowHandle<DemoApp>) {
        // --- Plain AppKit: the window. ---
        // SAFETY: all objects are created and used on the main thread and are
        // kept alive for the lifetime of the process (`mem::forget` in `run`).
        let native_window = unsafe {
            let rect = NSRect::new(NSPoint::new(180., 160.), NSSize::new(WIDTH, HEIGHT));
            let style = NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable;
            let native_window = NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            );
            native_window.setTitle(&NSString::from_str(title));
            native_window
        };

        // Pass the window's contentView directly as the host.  open_window
        // detects external_view == contentView and promotes the GPUI native
        // view to be the contentView via setContentView:, giving it the same
        // CA-transaction resize path as a regular GPUI window — no edge jitter.
        let (content_ptr, content_bounds) = unsafe {
            let cv = native_window
                .contentView()
                .expect("native window has a content view");
            let bounds = cv.bounds().size;
            let ptr = NonNull::new(Retained::as_ptr(&cv) as *mut _)
                .expect("content view pointer is non-null");
            (ptr, bounds)
        };

        let gpui_window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(0.), px(0.)),
                        size: size(px(content_bounds.width as f32), px(content_bounds.height as f32)),
                    })),
                    host_window_handle: Some(RawWindowHandle::AppKit(AppKitWindowHandle::new(
                        content_ptr,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    let app = cx.new(|cx| DemoApp {
                        focus_handle: cx.focus_handle(),
                        pane,
                        query: String::new(),
                        tab_windows: (0, 0),
                        info_anchor: Rc::new(Cell::new(Bounds::default())),
                    });
                    let focus_handle = app.read(cx).focus_handle.clone();
                    window.focus(&focus_handle, cx);
                    app
                },
            )
            .expect("failed to open hosted window");

        // After open_window, the GPUI native view is the window's contentView.
        // Add the native search field as a subview of it so it layers on top.
        if let Some(tx) = with_search {
            unsafe {
                let gpui_view = native_window
                    .contentView()
                    .expect("contentView is now the GPUI native view");
                let gpui_bounds = gpui_view.bounds().size;
                // A native search field in the pane's header row; pinned to
                // the top-right corner and follows resize via autoresizing.
                let search = NSSearchField::new(mtm);
                search.setFrame(NSRect::new(
                    NSPoint::new(
                        gpui_bounds.width - 24. - 200.,
                        gpui_bounds.height - 24. - 25.,
                    ),
                    NSSize::new(200., 26.),
                ));
                search.setAutoresizingMask(
                    objc2_app_kit::NSAutoresizingMaskOptions::ViewMinYMargin
                        | objc2_app_kit::NSAutoresizingMaskOptions::ViewMinXMargin,
                );
                search.setPlaceholderString(Some(&NSString::from_str("Filter metrics")));
                let search_handler = SearchHandler::new(tx);
                let _: () = msg_send![&*search, setDelegate: &*search_handler];
                gpui_view.addSubview(&search);
                // Both live for the rest of the process.
                std::mem::forget((search, search_handler));
            }
        }

        (native_window, gpui_window)
    }

    pub fn run() {
        application().run(|cx: &mut App| {
            cx.bind_keys([
                KeyBinding::new("cmd-1", SelectTab1, Some(APP_CONTEXT)),
                KeyBinding::new("cmd-2", SelectTab2, Some(APP_CONTEXT)),
                KeyBinding::new("enter", DismissInfo, Some(POPOVER_CONTEXT)),
                KeyBinding::new("cmd-k", ToggleFeature, Some(POPOVER_CONTEXT)),
            ]);
            let mtm = MainThreadMarker::new().expect("must run on the main thread");

            let (search_tx, search_rx) = mpsc::channel::<String>();

            let (dashboard_window, dashboard_app) =
                build_tab_window(cx, mtm, "Dashboard", Pane::Dashboard, Some(search_tx));
            let (showcase_window, showcase_app) =
                build_tab_window(cx, mtm, "Showcase", Pane::Showcase, None);

            // Merge the two native windows into one tab group — the same
            // native tab bar Terminal, Safari, or Ghostty use.
            // SAFETY: main thread; both windows are alive (forgotten below).
            unsafe {
                dashboard_window.makeKeyAndOrderFront(None);
                dashboard_window
                    .addTabbedWindow_ordered(&showcase_window, NSWindowOrderingMode::Above);
                dashboard_window.makeKeyAndOrderFront(None);
            }

            // Tell both panes about the native windows, for ⌘1/⌘2.
            let tab_windows = (
                Retained::as_ptr(&dashboard_window) as usize,
                Retained::as_ptr(&showcase_window) as usize,
            );
            for handle in [&dashboard_app, &showcase_app] {
                let _ = handle.update(cx, |app, _, _| {
                    app.tab_windows = tab_windows;
                });
            }

            // Stream the native search field's text into the dashboard pane.
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                    let mut latest_query = None;
                    while let Ok(text) = search_rx.try_recv() {
                        latest_query = Some(text);
                    }
                    if let Some(text) = latest_query {
                        let _ = dashboard_app.update(cx, |app, _, cx| {
                            app.query = text;
                            cx.notify();
                        });
                    }
                }
            })
            .detach();

            // The native windows live for the rest of the process.
            std::mem::forget((dashboard_window, showcase_window));

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
