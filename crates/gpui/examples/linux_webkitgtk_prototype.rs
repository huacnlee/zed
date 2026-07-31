#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The linux_webkitgtk_prototype example is only available on Linux.");
}

// This prototype isolates GTK ownership and overlay ordering before GPUI's
// Linux window backends are changed to render into GTK-owned surfaces.
#[cfg(target_os = "linux")]
fn main() {
    prototype::run();
}

#[cfg(target_os = "linux")]
mod prototype {
    use std::{cell::Cell, rc::Rc};

    use gtk::{
        Align, Application, ApplicationWindow, Box as GtkBox, Button, DrawingArea, EventBox, Label,
        Orientation, Overlay, gdk, glib::Propagation, prelude::*,
    };
    use webkit2gtk::{WebView, WebViewExt};

    const APPLICATION_ID: &str = "dev.zed.GpuiWebKitGtkPrototype";

    const PAGE: &str = r##"
<!doctype html>
<html>
<head>
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <style>
    :root { color-scheme: dark; font-family: system-ui, sans-serif; }
    body { margin: 0; padding: 40px; background: #0d1016; color: #bfbdb6; }
    small { color: #5ac1fe; letter-spacing: .14em; }
    h1 { max-width: 560px; margin: 18px 0 12px; font-size: 40px; line-height: 1.05; }
    p { max-width: 560px; color: #8a8986; line-height: 1.6; }
    input {
      width: 100%; margin-top: 28px; padding: 14px; box-sizing: border-box;
      border: 1px solid #3f4043; border-radius: 8px;
      background: #1f2127; color: #bfbdb6; font: 14px monospace;
    }
    input:focus { outline: 2px solid #5ac1fe; }
    #keys { margin-top: 12px; color: #8a8986; font: 12px monospace; }
  </style>
</head>
<body>
  <small>WEBKITGTK / LIVE</small>
  <h1>GTK owns the Linux composition hierarchy.</h1>
  <p>
    This prototype checks WebKitGTK rendering, keyboard focus, and GTK overlay
    ordering on both the X11 and Wayland GDK backends.
  </p>
  <input autofocus value="Focus and type inside WebKitGTK">
  <div id="keys">Keyboard event: waiting for input</div>
  <script>
    const output = document.querySelector("#keys");
    for (const type of ["keydown", "keyup"]) {
      window.addEventListener(type, event => {
        const key = event.key === " " ? "Space" : event.key;
        output.textContent = `${type}: key=${key} · code=${event.code}`;
      }, true);
    }
  </script>
</body>
</html>
"##;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum OverlayState {
        None,
        Popover,
        Dialog,
    }

    pub fn run() {
        let application = Application::builder()
            .application_id(APPLICATION_ID)
            .build();
        application.connect_activate(build_window);
        application.run();
    }

    fn build_window(application: &Application) {
        let window = ApplicationWindow::builder()
            .application(application)
            .title("GPUI WebKitGTK composition prototype")
            .default_width(900)
            .default_height(640)
            .build();

        let root = GtkBox::new(Orientation::Vertical, 0);
        let toolbar = GtkBox::new(Orientation::Horizontal, 8);
        toolbar.set_margin_start(20);
        toolbar.set_margin_end(20);
        toolbar.set_margin_top(12);
        toolbar.set_margin_bottom(12);

        let title = Label::new(Some("GPUI LINUX COMPOSITION PROTOTYPE"));
        title.set_halign(Align::Start);
        title.set_hexpand(true);
        let popover_button = Button::with_label("Show popover");
        let dialog_button = Button::with_label("Open dialog");
        toolbar.pack_start(&title, true, true, 0);
        toolbar.pack_end(&dialog_button, false, false, 0);
        toolbar.pack_end(&popover_button, false, false, 0);
        root.pack_start(&toolbar, false, false, 0);

        let composition = Overlay::new();
        composition.set_hexpand(true);
        composition.set_vexpand(true);

        let base = DrawingArea::new();
        base.set_hexpand(true);
        base.set_vexpand(true);
        base.connect_draw(|_, context| {
            context.set_source_rgb(0.19, 0.20, 0.22);
            context.paint().expect("failed to paint prototype base");
            Propagation::Proceed
        });
        composition.add(&base);

        let webview = WebView::new();
        webview.set_hexpand(true);
        webview.set_vexpand(true);
        webview.set_halign(Align::Fill);
        webview.set_valign(Align::Fill);
        webview.set_margin_start(20);
        webview.set_margin_end(20);
        webview.set_margin_bottom(20);
        webview.load_html(PAGE, None);
        composition.add_overlay(&webview);

        let overlay_input = EventBox::new();
        overlay_input.set_visible_window(false);
        overlay_input.set_hexpand(true);
        overlay_input.set_vexpand(true);
        overlay_input.set_halign(Align::Fill);
        overlay_input.set_valign(Align::Fill);
        overlay_input.add_events(gdk::EventMask::BUTTON_PRESS_MASK);

        let overlay_drawing = DrawingArea::new();
        overlay_drawing.set_hexpand(true);
        overlay_drawing.set_vexpand(true);
        overlay_input.add(&overlay_drawing);
        composition.add_overlay(&overlay_input);
        composition.set_overlay_pass_through(&overlay_input, true);

        let state = Rc::new(Cell::new(OverlayState::None));
        overlay_drawing.connect_draw({
            let state = state.clone();
            move |drawing, context| {
                draw_overlay(drawing, context, state.get());
                Propagation::Proceed
            }
        });

        let overlay_input_for_click = overlay_input.clone();
        overlay_input.connect_button_press_event({
            let composition = composition.clone();
            let overlay_drawing = overlay_drawing.clone();
            let state = state.clone();
            move |_, _| {
                state.set(OverlayState::None);
                composition.set_overlay_pass_through(&overlay_input_for_click, true);
                overlay_drawing.queue_draw();
                Propagation::Stop
            }
        });

        popover_button.connect_clicked({
            let composition = composition.clone();
            let overlay_drawing = overlay_drawing.clone();
            let overlay_input = overlay_input.clone();
            let state = state.clone();
            move |_| {
                let next = if state.get() == OverlayState::Popover {
                    OverlayState::None
                } else {
                    OverlayState::Popover
                };
                state.set(next);
                composition.set_overlay_pass_through(&overlay_input, next == OverlayState::None);
                overlay_drawing.queue_draw();
            }
        });

        dialog_button.connect_clicked({
            let composition = composition.clone();
            let overlay_drawing = overlay_drawing.clone();
            let overlay_input = overlay_input.clone();
            let state = state.clone();
            move |_| {
                state.set(OverlayState::Dialog);
                composition.set_overlay_pass_through(&overlay_input, false);
                overlay_drawing.queue_draw();
            }
        });

        root.pack_start(&composition, true, true, 0);
        window.add(&root);
        window.show_all();
    }

    fn draw_overlay(drawing: &DrawingArea, context: &gtk::cairo::Context, state: OverlayState) {
        let width = f64::from(drawing.allocated_width());
        let height = f64::from(drawing.allocated_height());

        match state {
            OverlayState::None => {}
            OverlayState::Popover => {
                let x = width - 340.0;
                rounded_rectangle(context, x, 18.0, 300.0, 132.0, 10.0);
                context.set_source_rgba(0.12, 0.13, 0.15, 0.98);
                context.fill_preserve().expect("failed to fill popover");
                context.set_source_rgb(0.25, 0.25, 0.26);
                context.stroke().expect("failed to stroke popover");
                draw_text(context, x + 22.0, 52.0, "GTK OVERLAY / POPOVER", 12.0);
                draw_text(context, x + 22.0, 88.0, "Rendered above WebKitGTK", 18.0);
            }
            OverlayState::Dialog => {
                context.set_source_rgba(0.05, 0.06, 0.08, 0.76);
                context.rectangle(0.0, 0.0, width, height);
                context.fill().expect("failed to fill dialog scrim");

                let dialog_width = 500.0;
                let dialog_height = 220.0;
                let x = (width - dialog_width) / 2.0;
                let y = (height - dialog_height) / 2.0;
                rounded_rectangle(context, x, y, dialog_width, dialog_height, 14.0);
                context.set_source_rgb(0.12, 0.13, 0.15);
                context.fill_preserve().expect("failed to fill dialog");
                context.set_source_rgb(0.25, 0.25, 0.26);
                context.stroke().expect("failed to stroke dialog");
                draw_text(context, x + 30.0, y + 54.0, "GTK OVERLAY / DIALOG", 12.0);
                draw_text(
                    context,
                    x + 30.0,
                    y + 104.0,
                    "WebKitGTK remains below this layer.",
                    22.0,
                );
                draw_text(
                    context,
                    x + 30.0,
                    y + 152.0,
                    "Click anywhere to close",
                    14.0,
                );
            }
        }
    }

    fn rounded_rectangle(
        context: &gtk::cairo::Context,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
    ) {
        let right = x + width;
        let bottom = y + height;
        context.new_sub_path();
        context.arc(
            right - radius,
            y + radius,
            radius,
            -std::f64::consts::FRAC_PI_2,
            0.0,
        );
        context.arc(
            right - radius,
            bottom - radius,
            radius,
            0.0,
            std::f64::consts::FRAC_PI_2,
        );
        context.arc(
            x + radius,
            bottom - radius,
            radius,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
        );
        context.arc(
            x + radius,
            y + radius,
            radius,
            std::f64::consts::PI,
            std::f64::consts::PI * 1.5,
        );
        context.close_path();
    }

    fn draw_text(context: &gtk::cairo::Context, x: f64, y: f64, text: &str, size: f64) {
        context.set_source_rgb(0.75, 0.74, 0.71);
        context.select_font_face(
            "Sans",
            gtk::cairo::FontSlant::Normal,
            gtk::cairo::FontWeight::Normal,
        );
        context.set_font_size(size);
        context.move_to(x, y);
        context
            .show_text(text)
            .expect("failed to draw overlay text");
    }
}
