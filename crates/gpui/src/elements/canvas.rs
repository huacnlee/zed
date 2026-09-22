use crate::retained::{Damage, EnvironmentDiff, RetainableElement};
use refineable::Refineable as _;

use crate::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, Pixels,
    Style, StyleRefinement, Styled, Window,
};

/// Construct a canvas element with the given paint callback.
/// Useful for adding short term custom drawing to a view.
pub fn canvas<T>(
    prepaint: impl 'static + FnOnce(Bounds<Pixels>, &mut Window, &mut App) -> T,
    paint: impl 'static + FnOnce(Bounds<Pixels>, T, &mut Window, &mut App),
) -> Canvas<T> {
    Canvas {
        prepaint: Some(Box::new(prepaint)),
        paint: Some(Box::new(paint)),
        style: StyleRefinement::default(),
        retention: None,
        prepaint_effects: false,
    }
}

/// A canvas element, meant for accessing the low level paint API without defining a whole
/// custom element
pub struct Canvas<T> {
    prepaint: Option<Box<dyn FnOnce(Bounds<Pixels>, &mut Window, &mut App) -> T>>,
    paint: Option<Box<dyn FnOnce(Bounds<Pixels>, T, &mut Window, &mut App)>>,
    style: StyleRefinement,
    retention: Option<(ElementId, u64)>,
    prepaint_effects: bool,
}

impl<T> Canvas<T> {
    /// Allows reusing drawing output while `revision` and the drawing environment are unchanged.
    /// Increment the revision whenever data read by the paint callback or prepaint output changes.
    /// The paint callback must not require execution for side effects. Event registration and
    /// resource-backed drawing conservatively disable reuse.
    pub fn retained(mut self, id: impl Into<ElementId>, revision: u64) -> Self {
        self.retention = Some((id.into(), revision));
        self
    }
}

impl<T: 'static> IntoElement for Canvas<T> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<T: 'static> RetainableElement for Canvas<T> {
    type RetainedProperties = Option<u64>;

    fn retained_properties(&self) -> Self::RetainedProperties {
        self.retention.as_ref().map(|(_, revision)| *revision)
    }

    fn diff(
        previous: &Self::RetainedProperties,
        current: &Self::RetainedProperties,
        environment: &EnvironmentDiff,
    ) -> Damage {
        if current.is_none() {
            return Damage::FULL;
        }
        if previous != current || !environment.is_empty() {
            Damage::PREPAINT | Damage::PAINT
        } else {
            Damage::empty()
        }
    }
}

impl<T: 'static> Element for Canvas<T> {
    fn uses_retained_diff(&self) -> bool {
        self.retention.is_some()
    }
    type RequestLayoutState = Style;
    type PrepaintState = Option<T>;

    fn id(&self) -> Option<ElementId> {
        self.retention.as_ref().map(|(id, _)| id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (crate::LayoutId, Self::RequestLayoutState) {
        if self.retention.is_some() {
            window.reconcile_retained_properties(self, cx);
        }
        let mut style = Style::default();
        style.refine(&self.style);
        let layout_id = window.request_layout(style.clone(), [], cx);
        (layout_id, style)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Style,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<T> {
        let start = window.prepaint_index();
        let state = self.prepaint.take().unwrap()(bounds, window, cx);
        self.prepaint_effects = !start.same_effects(&window.prepaint_index());
        Some(state)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        style: &mut Style,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let prepaint = prepaint.take().unwrap();
        style.paint(bounds, window, cx, |window, cx| {
            let paint = self.paint.take().unwrap();
            if let Some((_, revision)) = &self.retention
                && !self.prepaint_effects
            {
                window.paint_retained_geometry(*revision, bounds, cx, |window, cx| {
                    paint(bounds, prepaint, window, cx)
                });
            } else {
                paint(bounds, prepaint, window, cx);
            }
        });
    }
}

impl<T> Styled for Canvas<T> {
    fn style(&mut self) -> &mut crate::StyleRefinement {
        &mut self.style
    }
}
