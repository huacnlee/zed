use crate::{
    AnyElement, App, Bounds, ContentMask, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, PaintIndex, Pixels, PrepaintStateIndex, TextStyle, Window,
};
use std::{marker::PhantomData, ops::Range, panic};

/// Builds an element subtree only when its version or external layout context changes.
///
/// The ID must be stable and unique among siblings. The version must change whenever any value
/// that affects the built element changes. Reuse is limited to roots with definite width and
/// height; content-sized roots are rebuilt normally.
#[track_caller]
pub fn retained<E, F>(id: impl Into<ElementId>, version: u64, build: F) -> RetainedElement<E, F>
where
    E: IntoElement,
    F: FnOnce() -> E,
{
    RetainedElement {
        id: id.into(),
        version,
        build: Some(build),
        _element: PhantomData,
        #[cfg(debug_assertions)]
        source: panic::Location::caller(),
    }
}

/// An element subtree whose output is identified by a caller-provided version.
pub struct RetainedElement<E, F> {
    id: ElementId,
    version: u64,
    build: Option<F>,
    _element: PhantomData<fn() -> E>,
    #[cfg(debug_assertions)]
    source: &'static panic::Location<'static>,
}

struct RetainedLayoutState {
    version: u64,
    style: Option<taffy::Style>,
    rem_size: Pixels,
    scale_factor: f32,
}

struct RetainedDrawState {
    version: u64,
    prepaint_range: Range<PrepaintStateIndex>,
    paint_range: Range<PaintIndex>,
    cache_key: RetainedDrawCacheKey,
}

struct RetainedDrawCacheKey {
    bounds: Bounds<Pixels>,
    content_mask: ContentMask<Pixels>,
    text_style: TextStyle,
}

impl<E, F> RetainedElement<E, F>
where
    E: IntoElement,
    F: FnOnce() -> E,
{
    fn build(&mut self) -> AnyElement {
        self.build
            .take()
            .expect("retained element builder must only be called once")()
        .into_any_element()
    }
}

impl<E, F> IntoElement for RetainedElement<E, F>
where
    E: IntoElement + 'static,
    F: FnOnce() -> E + 'static,
{
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E, F> Element for RetainedElement<E, F>
where
    E: IntoElement + 'static,
    F: FnOnce() -> E + 'static,
{
    type RequestLayoutState = Option<AnyElement>;
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static panic::Location<'static>> {
        #[cfg(debug_assertions)]
        return Some(self.source);

        #[cfg(not(debug_assertions))]
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let caching_disabled = window.is_inspector_picking(cx);
        let rem_size = window.rem_size();
        let scale_factor = window.scale_factor();
        window.with_element_state::<RetainedLayoutState, _>(
            id.expect("retained elements always have an ID"),
            |state, window| {
                if let Some(state) = state
                    && state.version == self.version
                    && state.rem_size == rem_size
                    && state.scale_factor == scale_factor
                    && !caching_disabled
                    && !window.refreshing
                    && let Some(style) = state.style.clone()
                {
                    return ((window.request_retained_layout(style), None), state);
                }

                let mut element = self.build();
                let layout_id = element.request_layout(window, cx);
                let state = RetainedLayoutState {
                    version: self.version,
                    style: window.retainable_layout_style(layout_id),
                    rem_size,
                    scale_factor,
                };
                ((layout_id, Some(element)), state)
            },
        )
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let content_mask = window.content_mask();
        let text_style = window.text_style();
        window.with_element_state::<RetainedDrawState, _>(
            id.expect("retained elements always have an ID"),
            |state, window| {
                if let Some(mut state) = state
                    && state.version == self.version
                    && state.cache_key.bounds == bounds
                    && state.cache_key.content_mask == content_mask
                    && state.cache_key.text_style == text_style
                    && !window.refreshing
                    && element.is_none()
                {
                    let prepaint_start = window.prepaint_index();
                    window.reuse_prepaint(state.prepaint_range.clone());
                    let prepaint_end = window.prepaint_index();
                    state.prepaint_range = prepaint_start..prepaint_end;
                    return (None, state);
                }

                let mut element = element.take().unwrap_or_else(|| {
                    let mut element = self.build();
                    element.layout_as_root(bounds.size.into(), window, cx);
                    element
                });
                let prepaint_start = window.prepaint_index();
                element.prepaint_at(bounds.origin, window, cx);
                let prepaint_end = window.prepaint_index();
                (
                    Some(element),
                    RetainedDrawState {
                        version: self.version,
                        prepaint_range: prepaint_start..prepaint_end,
                        paint_range: PaintIndex::default()..PaintIndex::default(),
                        cache_key: RetainedDrawCacheKey {
                            bounds,
                            content_mask,
                            text_style,
                        },
                    },
                )
            },
        )
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        element: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_element_state::<RetainedDrawState, _>(
            id.expect("retained elements always have an ID"),
            |state, window| {
                let mut state = state.expect("retained element must be prepainted before painting");
                let paint_start = window.paint_index();
                if let Some(element) = element {
                    element.paint(window, cx);
                } else {
                    window.reuse_paint(state.paint_range.clone());
                }
                let paint_end = window.paint_index();
                state.paint_range = paint_start..paint_end;
                ((), state)
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Context, Render, TestAppContext, canvas, div, prelude::*, px};
    use std::{cell::Cell, rc::Rc};

    struct RetainedTestView {
        version: u64,
        width: Pixels,
        unrelated_state: bool,
        build_count: Rc<Cell<usize>>,
        observed_version: Rc<Cell<u64>>,
        observed_width: Rc<Cell<Pixels>>,
    }

    impl Render for RetainedTestView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let version = self.version;
            let build_count = self.build_count.clone();
            let observed_version = self.observed_version.clone();
            let observed_width = self.observed_width.clone();
            div()
                .w(self.width)
                .h(px(100.))
                .child(retained("content", version, move || {
                    build_count.set(build_count.get() + 1);
                    div().w_full().h(px(20.)).child(canvas(
                        move |bounds, _, _| observed_width.set(bounds.size.width),
                        move |_, _, _, _| observed_version.set(version),
                    ))
                }))
                .child(div().when(self.unrelated_state, |this| this.opacity(0.5)))
        }
    }

    #[gpui::test]
    fn retained_element_reuses_version_and_invalidates_external_context(cx: &mut TestAppContext) {
        let build_count = Rc::new(Cell::new(0));
        let observed_version = Rc::new(Cell::new(u64::MAX));
        let observed_width = Rc::new(Cell::new(Pixels::ZERO));
        let (view, cx) = cx.add_window_view({
            let build_count = build_count.clone();
            let observed_version = observed_version.clone();
            let observed_width = observed_width.clone();
            |_, _| RetainedTestView {
                version: 0,
                width: px(100.),
                unrelated_state: false,
                build_count,
                observed_version,
                observed_width,
            }
        });

        assert_eq!(build_count.get(), 1);
        assert_eq!(observed_version.get(), 0);
        assert_eq!(observed_width.get(), px(100.));

        view.update(cx, |view, cx| {
            view.unrelated_state = true;
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(build_count.get(), 1);

        view.update(cx, |view, cx| {
            view.version += 1;
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(build_count.get(), 2);
        assert_eq!(observed_version.get(), 1);

        view.update(cx, |view, cx| {
            view.width = px(200.);
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(build_count.get(), 3);
        assert_eq!(observed_width.get(), px(200.));

        cx.update(|window, _cx| window.set_rem_size(px(18.)));
        view.update(cx, |_view, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(build_count.get(), 4);

        cx.update(|window, _cx| window.refresh());
        cx.run_until_parked();
        assert_eq!(build_count.get(), 5);
    }

    struct IntrinsicRetainedTestView {
        unrelated_state: bool,
        build_count: Rc<Cell<usize>>,
    }

    impl Render for IntrinsicRetainedTestView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let build_count = self.build_count.clone();
            div()
                .size_full()
                .child(retained("content", 0, move || {
                    build_count.set(build_count.get() + 1);
                    div().child("intrinsic size")
                }))
                .child(div().when(self.unrelated_state, |this| this.opacity(0.5)))
        }
    }

    #[gpui::test]
    fn retained_element_falls_back_when_root_size_depends_on_contents(cx: &mut TestAppContext) {
        let build_count = Rc::new(Cell::new(0));
        let (view, cx) = cx.add_window_view({
            let build_count = build_count.clone();
            |_, _| IntrinsicRetainedTestView {
                unrelated_state: false,
                build_count,
            }
        });

        assert_eq!(build_count.get(), 1);
        view.update(cx, |view, cx| {
            view.unrelated_state = true;
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(build_count.get(), 2);
    }
}
