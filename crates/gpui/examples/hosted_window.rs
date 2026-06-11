//! Embeds GPUI windows inside an application built with the platform's native
//! APIs, using `WindowOptions::host_window_handle` — the building block for
//! adopting GPUI incrementally inside an existing native application.
//!
//! The scenario: a mail-style AppKit application, demonstrating every way the
//! two UI stacks compose:
//!
//! - The window shell, vibrant sidebar, search field, and navigation are
//!   plain AppKit; the message pane is a hosted GPUI window.
//! - A second, component-sized GPUI window (the account card) is hosted
//!   inside the native sidebar.
//! - "More ▾" opens a system `NSPopover` (arrow, vibrancy, transient
//!   dismissal) whose content is a third hosted GPUI window — with working
//!   GPUI keyboard shortcuts.
//! - Native controls layer *above* GPUI (the Address field), and their input
//!   streams into GPUI state (the banner mirrors search & address live).
//!
//! Run with: `cargo run -p gpui --example hosted_window` (macOS today;
//! Windows/X11 hosts work the same way through `host_window_handle`).

#![cfg_attr(target_family = "wasm", no_main)]

#[cfg(target_os = "macos")]
mod example {
    use gpui::{
        App, Bounds, Context, FocusHandle, FontWeight, KeyBinding, Pixels, SharedString, Window,
        WindowBackgroundAppearance, WindowBounds, WindowOptions, actions, canvas, div, point,
        prelude::*, px, rgb, rgba, size,
    };
    use gpui_platform::application;
    use objc2::rc::Retained;
    use objc2::runtime::NSObject;
    use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSBackingStoreType, NSControl, NSPopover, NSPopoverBehavior,
        NSSearchField, NSTextField, NSView, NSViewController, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectView, NSWindow, NSWindowStyleMask,
    };
    use objc2_foundation::{NSNotification, NSPoint, NSRect, NSRectEdge, NSSize, NSString};
    use raw_window_handle::{AppKitWindowHandle, HasWindowHandle, RawWindowHandle};
    use std::cell::Cell;
    use std::ptr::NonNull;
    use std::rc::Rc;
    use std::sync::mpsc;
    use std::time::Duration;

    actions!(hosted_window, [NewDocument, OpenRecent, Share]);

    const KEY_CONTEXT: &str = "QuickActions";

    /// Ivars for [`SearchHandler`]: forwards the native search field's text.
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

    const WIDTH: f64 = 760.;
    const HEIGHT: f64 = 480.;
    const SIDEBAR: f64 = 200.;

    /// A small account card embedded at the bottom of the *native* sidebar —
    /// a second hosted GPUI window, showing that GPUI embeds at any
    /// granularity: a whole pane or a single component.
    struct AccountCard;

    impl Render for AccountCard {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .items_center()
                .gap_2()
                .size_full()
                .p_2()
                .rounded_lg()
                .bg(rgba(0x7878801a))
                .text_color(rgba(0x000000d9))
                .child(
                    div()
                        .size(px(28.))
                        .rounded_full()
                        .bg(rgb(0x10b981))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(gpui::white())
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("JL"),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .child("Jason — GPUI card"),
                        )
                        .child(
                            div()
                                .h(px(4.))
                                .w_full()
                                .mt_1()
                                .rounded_full()
                                .bg(rgba(0x3c3c4326))
                                .child(div().h_full().w_2_3().rounded_full().bg(rgb(0x10b981))),
                        ),
                )
        }
    }

    /// Quick-actions panel rendered inside a native `NSPopover` — a third
    /// hosted GPUI window, in a *system-owned* container this time (arrow,
    /// vibrancy, transient dismissal). The shortcuts are real GPUI key
    /// bindings, exercising hosted keyboard input.
    struct QuickActions {
        focus_handle: FocusHandle,
        last_action: Option<&'static str>,
    }

    impl QuickActions {
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
    }

    impl Render for QuickActions {
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
                        .flex()
                        .flex_col()
                        .child(self.row("new", 0x3b82f6, "New Document", "⌘N", cx))
                        .child(self.row("open", 0x22c55e, "Open Recent", "⌘O", cx))
                        .child(self.row("share", 0xf59e0b, "Share…", "⇧⌘S", cx)),
                )
                .child(div().h(px(1.)).w_full().bg(rgba(0x3c3c431f)))
                .child(
                    div()
                        .px_2()
                        .text_size(px(11.))
                        .text_color(rgba(0x3c3c4380))
                        .child(match self.last_action {
                            Some(label) => format!("Last action: {label}"),
                            None => "Click a row or press a shortcut".to_string(),
                        }),
                )
        }
    }

    /// Shows an `NSPopover` anchored to `anchor` (bounds within the message
    /// pane's GPUI window) and hosts a [`QuickActions`] GPUI window inside it.
    fn open_quick_actions(anchor: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        // Anchor to the message pane's own (hosted) GPUI view.
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::AppKit(parent) = handle.as_raw() else {
            return;
        };
        // SAFETY: the pointer comes from the live window's AppKit handle and is
        // only used synchronously while the window is alive.
        let parent_view: &NSView = unsafe { &*parent.ns_view.as_ptr().cast() };

        let content_size = size(px(280.), px(164.));

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
                    let content = cx.new(|cx| QuickActions {
                        focus_handle: cx.focus_handle(),
                        last_action: None,
                    });
                    let focus_handle = content.read(cx).focus_handle.clone();
                    window.focus(&focus_handle, cx);
                    content
                },
            )
            .expect("failed to open hosted popover window");

        // Let the popover receive keyboard input immediately, so the shortcuts
        // work without clicking inside first.
        // SAFETY: main thread; the container was installed by `show...` above.
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

    /// The content pane — a mail message view, rendered entirely by GPUI. Its
    /// `query` mirrors the native sidebar's NSSearchField in real time.
    struct MessagePane {
        replied: bool,
        query: String,
        address: String,
        /// The "More" button's bounds, captured at layout so the native popover
        /// can anchor to it.
        more_bounds: Rc<Cell<Bounds<Pixels>>>,
    }

    /// A small pill mirroring a native field's value.
    fn mirror_chip(value: &str, empty_hint: &str) -> impl IntoElement {
        let empty = value.is_empty();
        div()
            .px_2()
            .py_0p5()
            .rounded_full()
            .bg(if empty {
                rgba(0x7878801f)
            } else {
                rgba(0x3b82f626)
            })
            .text_color(if empty {
                rgba(0x3c3c4366)
            } else {
                rgba(0x1d4ed8ff)
            })
            .child(if empty {
                empty_hint.to_string()
            } else {
                value.to_string()
            })
    }

    impl Render for MessagePane {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .size_full()
                .bg(gpui::white())
                .text_color(rgba(0x000000d9))
                // Banner: live mirror of the native search field — native input
                // flowing into GPUI state.
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_6()
                        .py_2()
                        .bg(rgba(0x78788014))
                        .border_b_1()
                        .border_color(rgba(0x3c3c431f))
                        .text_size(px(11.))
                        .text_color(rgba(0x3c3c4380))
                        .child("Search:")
                        .child(mirror_chip(&self.query, "type in the sidebar…"))
                        .child(div().w_2())
                        .child("Address:")
                        .child(mirror_chip(&self.address, "type below…")),
                )
                .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .p_6()
                .gap_4()
                // Sender row.
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .size(px(36.))
                                .rounded_full()
                                .bg(rgb(0x6366f1))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(gpui::white())
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("AC"),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Alex Chen"),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgba(0x3c3c4380))
                                        .child("alex@example.com · 9:41 AM"),
                                ),
                        ),
                )
                // Subject.
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Migrating our content pane to GPUI"),
                )
                // Body.
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .text_size(px(13.))
                        .text_color(rgba(0x000000b3))
                        .child(
                            "The window, the vibrant sidebar, and the search field on the left \
                             are still plain AppKit. This message pane is a GPUI window hosted \
                             in a sub-view via WindowOptions::host_window_handle.",
                        )
                        .child(
                            "That lets an existing native application adopt GPUI one pane at a \
                             time — same window, two UI stacks, one input story.",
                        ),
                )
                .child(div().flex_1())
                // Reserved row: the native NSTextField "Address" (layered above
                // this GPUI window by AppKit) sits here.
                .child(div().h(px(34.)))
                .child(div().h(px(1.)).w_full().bg(rgba(0x3c3c431f)))
                // Actions.
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .id("reply")
                                .px_4()
                                .py_1p5()
                                .rounded_md()
                                .text_size(px(13.))
                                .font_weight(FontWeight::MEDIUM)
                                .bg(rgb(0x1d1d1f))
                                .text_color(gpui::white())
                                .hover(|style| style.bg(rgb(0x2c2c2e)))
                                .active(|style| style.bg(rgb(0x3a3a3c)))
                                .child(if self.replied { "Replied ✓" } else { "Reply" })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.replied = true;
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("forward")
                                .px_4()
                                .py_1p5()
                                .rounded_md()
                                .text_size(px(13.))
                                .font_weight(FontWeight::MEDIUM)
                                .bg(rgba(0x7878801f))
                                .hover(|style| style.bg(rgba(0x78788033)))
                                .active(|style| style.bg(rgba(0x78788047)))
                                .child("Forward"),
                        )
                        .child({
                            let capture = self.more_bounds.clone();
                            let anchor = self.more_bounds.clone();
                            div()
                                .relative()
                                .child(
                                    div()
                                        .id("more")
                                        .px_4()
                                        .py_1p5()
                                        .rounded_md()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .bg(rgba(0x7878801f))
                                        .hover(|style| style.bg(rgba(0x78788033)))
                                        .active(|style| style.bg(rgba(0x78788047)))
                                        .child("More ▾")
                                        .on_click(move |_, window, cx| {
                                            open_quick_actions(anchor.get(), window, cx);
                                        }),
                                )
                                .child(
                                    // Records the button's bounds for anchoring.
                                    canvas(
                                        move |bounds, _, _| capture.set(bounds),
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .inset_0(),
                                )
                        }),
                ))
        }
    }

    pub fn run() {
        application().run(|cx: &mut App| {
            cx.bind_keys([
                KeyBinding::new("cmd-n", NewDocument, Some(KEY_CONTEXT)),
                KeyBinding::new("cmd-o", OpenRecent, Some(KEY_CONTEXT)),
                KeyBinding::new("shift-cmd-s", Share, Some(KEY_CONTEXT)),
            ]);
            let mtm = MainThreadMarker::new().expect("must run on the main thread");

            let (search_tx, search_rx) = mpsc::channel::<String>();

            // --- Plain AppKit: window + vibrant sidebar + search + navigation. ---
            // SAFETY: all objects are created and used on the main thread and are
            // kept alive for the lifetime of the process (`mem::forget` below).
            let (native_window, host_view, card_host, native_sidebar) = unsafe {
                let rect = NSRect::new(NSPoint::new(160., 160.), NSSize::new(WIDTH, HEIGHT));
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
                native_window.setTitle(&NSString::from_str("Inbox — AppKit shell, GPUI content"));
                let content_view = native_window
                    .contentView()
                    .expect("native window has a content view");
                let content_height = content_view.bounds().size.height;

                // Vibrant sidebar.
                let sidebar = NSVisualEffectView::new(mtm);
                sidebar.setMaterial(NSVisualEffectMaterial::Sidebar);
                sidebar.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
                sidebar.setFrame(NSRect::new(
                    NSPoint::new(0., 0.),
                    NSSize::new(SIDEBAR, content_height),
                ));
                sidebar.setAutoresizingMask(NSAutoresizingMaskOptions::ViewHeightSizable);
                content_view.addSubview(&sidebar);

                // Native search field.
                let search = NSSearchField::new(mtm);
                search.setFrame(NSRect::new(
                    NSPoint::new(12., content_height - 40.),
                    NSSize::new(SIDEBAR - 24., 28.),
                ));
                search.setPlaceholderString(Some(&NSString::from_str("Search")));
                sidebar.addSubview(&search);

                // Forward the search field's text changes to GPUI.
                let search_handler = SearchHandler::new(search_tx);
                let _: () = msg_send![&*search, setDelegate: &*search_handler];

                // Navigation list.
                for (index, item) in ["📥  Inbox", "📤  Sent", "📝  Drafts", "🗂  Archive"]
                    .iter()
                    .enumerate()
                {
                    let label = NSTextField::labelWithString(&NSString::from_str(item), mtm);
                    label.setFrame(NSRect::new(
                        NSPoint::new(16., content_height - 76. - index as f64 * 30.),
                        NSSize::new(SIDEBAR - 32., 20.),
                    ));
                    sidebar.addSubview(&label);
                }

                // A small host at the bottom of the *native* sidebar for a
                // GPUI-rendered account card.
                let card_host = NSView::new(mtm);
                card_host.setFrame(NSRect::new(
                    NSPoint::new(12., 12.),
                    NSSize::new(SIDEBAR - 24., 52.),
                ));
                sidebar.addSubview(&card_host);

                // The pane GPUI renders into: everything right of the sidebar.
                let host_view = NSView::new(mtm);
                host_view.setFrame(NSRect::new(
                    NSPoint::new(SIDEBAR, 0.),
                    NSSize::new(WIDTH - SIDEBAR, content_height),
                ));
                host_view.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewWidthSizable
                        | NSAutoresizingMaskOptions::ViewHeightSizable,
                );
                content_view.addSubview(&host_view);

                native_window.makeKeyAndOrderFront(None);
                (
                    native_window,
                    host_view,
                    card_host,
                    (sidebar, search, search_handler),
                )
            };

            // --- GPUI: render the message pane into the host view. ---
            let host_size = unsafe { NSView::bounds(&host_view) }.size;
            let host_ptr = NonNull::new(Retained::as_ptr(&host_view) as *mut _)
                .expect("host view pointer is non-null");
            let pane = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds {
                            origin: point(px(0.), px(0.)),
                            size: size(px(host_size.width as f32), px(host_size.height as f32)),
                        })),
                        host_window_handle: Some(RawWindowHandle::AppKit(
                            AppKitWindowHandle::new(host_ptr),
                        )),
                        ..Default::default()
                    },
                    |_, cx| {
                        cx.new(|_| MessagePane {
                            replied: false,
                            query: String::new(),
                            address: String::new(),
                            more_bounds: Rc::new(Cell::new(Bounds::default())),
                        })
                    },
                )
                .expect("failed to open hosted window");

            // --- GPUI: a second hosted window — the sidebar's account card. ---
            let card_size = unsafe { NSView::bounds(&card_host) }.size;
            let card_ptr = NonNull::new(Retained::as_ptr(&card_host) as *mut _)
                .expect("card host pointer is non-null");
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(0.), px(0.)),
                        size: size(px(card_size.width as f32), px(card_size.height as f32)),
                    })),
                    host_window_handle: Some(RawWindowHandle::AppKit(AppKitWindowHandle::new(
                        card_ptr,
                    ))),
                    // Let the sidebar's vibrancy show through the card.
                    window_background: gpui::WindowBackgroundAppearance::Transparent,
                    ..Default::default()
                },
                |_, cx| cx.new(|_| AccountCard),
            )
            .expect("failed to open hosted card window");

            // --- Native above GPUI: an "Address" text field layered over the
            // GPUI pane. AppKit's view hierarchy makes this trivial — add the
            // control as a later subview of the host view, above the GPUI view.
            // It receives its own input; everything around it goes to GPUI, and
            // its text streams into the GPUI pane like the search field's.
            // SAFETY: main thread; the field is kept alive by the forget below.
            let (addr_tx, addr_rx) = mpsc::channel::<String>();
            let address_field = unsafe {
                let field = NSTextField::new(mtm);
                field.setPlaceholderString(Some(&NSString::from_str(
                    "Address — native NSTextField over GPUI",
                )));
                field.setFrame(NSRect::new(
                    NSPoint::new(24., 92.),
                    NSSize::new(host_size.width - 48., 26.),
                ));
                let handler = SearchHandler::new(addr_tx);
                let _: () = msg_send![&*field, setDelegate: &*handler];
                host_view.addSubview(&field);
                (field, handler)
            };

            // Mirror the native search field's text into the GPUI pane.
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                    let mut latest_query = None;
                    while let Ok(text) = search_rx.try_recv() {
                        latest_query = Some(text);
                    }
                    let mut latest_address = None;
                    while let Ok(text) = addr_rx.try_recv() {
                        latest_address = Some(text);
                    }
                    if latest_query.is_some() || latest_address.is_some() {
                        let _ = pane.update(cx, |pane, _, cx| {
                            if let Some(text) = latest_query {
                                pane.query = text;
                            }
                            if let Some(text) = latest_address {
                                pane.address = text;
                            }
                            cx.notify();
                        });
                    }
                }
            })
            .detach();

            // The native window lives for the rest of the process.
            std::mem::forget((
                native_window,
                host_view,
                card_host,
                native_sidebar,
                address_field,
            ));

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
