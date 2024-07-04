use std::rc::Rc;

use wry::{
    dpi::{self, LogicalSize},
    Rect,
};

use crate::{
    Bounds, Element, ElementId, GlobalElementId, IntoElement, LayoutId, Pixels, Size, Style,
    WindowContext,
};

/// A webview element can display a wry webview.
pub struct WebView {
    view: Rc<wry::WebView>,
}

impl WebView {
    /// Create a new webview element from a wry WebView.
    pub fn new(view: Rc<wry::WebView>) -> Self {
        Self { view }
    }
}

impl IntoElement for WebView {
    type Element = WebView;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for WebView {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        cx: &mut WindowContext,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.flex_grow = 1.0;
        style.size = Size::full();
        let id = cx.request_layout(style, []);
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        cx: &mut WindowContext,
    ) -> Self::PrepaintState {
        if bounds.top() > cx.viewport_size().height || bounds.bottom() < Pixels::ZERO {
            self.view.set_visible(false).unwrap();
        } else {
            self.view.set_visible(true).unwrap();

            self.view
                .set_bounds(Rect {
                    size: dpi::Size::Logical(LogicalSize {
                        width: (bounds.size.width.0).into(),
                        height: (bounds.size.height.0).into(),
                    }),
                    position: dpi::Position::Logical(dpi::LogicalPosition::new(
                        bounds.origin.x.into(),
                        bounds.origin.y.into(),
                    )),
                })
                .unwrap();
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        _: &mut WindowContext,
    ) {
        // Nothing to do here
    }
}
