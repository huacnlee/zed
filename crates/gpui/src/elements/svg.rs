use std::{
    fs,
    hash::{Hash, Hasher},
    path::Path,
    sync::Arc,
};

use super::div::InteractivityRetainedProperties;
use crate::Style;
use crate::retained::{Damage, EnvironmentDiff, Isolation, RetainableElement};
use crate::{
    App, Asset, Bounds, Element, GlobalElementId, Hitbox, InspectorElementId, InteractiveElement,
    Interactivity, IntoElement, LayoutId, Pixels, Point, Radians, SharedString, Size,
    StyleRefinement, Styled, TransformationMatrix, Window, point, px, radians, size,
};
use gpui_util::ResultExt;
use refineable::Refineable as _;

/// An SVG element.
pub struct Svg {
    interactivity: Interactivity,
    transformation: Option<Transformation>,
    path: Option<SharedString>,
    external_path: Option<SharedString>,
    data: Option<Arc<[u8]>>,
    data_path: Option<SharedString>,
}

/// Create a new SVG element.
#[track_caller]
pub fn svg() -> Svg {
    Svg {
        interactivity: Interactivity::new(),
        transformation: None,
        path: None,
        external_path: None,
        data: None,
        data_path: None,
    }
}

impl Svg {
    fn retained_properties_with_style(&self, style: Style) -> SvgRetainedProperties {
        let source = if let Some(data) = self.data.as_ref().filter(|_| self.data_path.is_some()) {
            SvgRetainedSource::Data(data.clone())
        } else if let Some(path) = &self.external_path {
            SvgRetainedSource::External(path.clone())
        } else if let Some(path) = &self.path {
            SvgRetainedSource::Asset(path.clone())
        } else {
            SvgRetainedSource::None
        };
        SvgRetainedProperties {
            interactivity: self.interactivity.retained_properties(style),
            source,
            transformation: self.transformation,
        }
    }

    /// Set the path to the SVG file for this element.
    pub fn path(mut self, path: impl Into<SharedString>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Set the path to the SVG file for this element.
    pub fn external_path(mut self, path: impl Into<SharedString>) -> Self {
        self.external_path = Some(path.into());
        self
    }

    /// Set the raw SVG data for this element.
    /// The SVG will be rendered directly from the provided bytes.
    pub fn data(mut self, data: &[u8]) -> Self {
        // Generate a unique deterministic path based on the data hash for caching
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        data.hash(&mut hasher);
        let hash = hasher.finish();
        let path = SharedString::from(format!("__binary_svg__{}", hash));
        self.data = Some(Arc::from(data));
        self.data_path = Some(path);
        self
    }

    /// Transform the SVG element with the given transformation.
    /// Note that this won't effect the hitbox or layout of the element, only the rendering.
    pub fn with_transformation(mut self, transformation: Transformation) -> Self {
        self.transformation = Some(transformation);
        self
    }
}

#[derive(PartialEq)]
enum SvgRetainedSource {
    None,
    Asset(SharedString),
    External(SharedString),
    Data(Arc<[u8]>),
}

pub(crate) struct SvgRetainedProperties {
    interactivity: InteractivityRetainedProperties,
    source: SvgRetainedSource,
    transformation: Option<Transformation>,
}

impl RetainableElement for Svg {
    type RetainedProperties = SvgRetainedProperties;

    fn retained_properties(&self) -> Self::RetainedProperties {
        let mut style = Style::default();
        style.refine(&self.interactivity.base_style);
        self.retained_properties_with_style(style)
    }

    fn diff(
        previous: &Self::RetainedProperties,
        current: &Self::RetainedProperties,
        environment: &EnvironmentDiff,
    ) -> Damage {
        let mut damage = InteractivityRetainedProperties::diff(
            &previous.interactivity,
            &current.interactivity,
            environment,
        );
        if previous.source != current.source {
            damage |= Damage::PAINT;
        }
        if previous.transformation != current.transformation {
            // SVG transformations are currently baked into the sprite paint output.
            damage |= Damage::TRANSFORM | Damage::PAINT;
        }
        damage
    }

    fn property_heap_bytes(properties: &Self::RetainedProperties) -> usize {
        properties.interactivity.heap_bytes()
            + match &properties.source {
                SvgRetainedSource::None => 0,
                SvgRetainedSource::Asset(path) | SvgRetainedSource::External(path) => path.len(),
                SvgRetainedSource::Data(data) => data.len(),
            }
    }
}

#[cfg(test)]
mod retained_tests {
    use super::*;

    #[test]
    fn svg_resource_diff_respects_source_precedence() {
        let environment = EnvironmentDiff {
            metrics: false,
            text_layout: false,
            text_paint: false,
            composite: false,
        };
        for (previous, current, expected) in [
            (svg().path("a.svg"), svg().path("a.svg"), Damage::empty()),
            (svg().path("a.svg"), svg().path("b.svg"), Damage::PAINT),
            (
                svg().path("a.svg"),
                svg().external_path("a.svg"),
                Damage::PAINT,
            ),
            (svg().path("a.svg"), svg(), Damage::PAINT),
            (
                svg().data(b"same").path("a.svg"),
                svg().data(b"same").external_path("b.svg"),
                Damage::empty(),
            ),
            (svg().data(b"first"), svg().data(b"second"), Damage::PAINT),
        ] {
            assert_eq!(
                Svg::diff(
                    &previous.retained_properties(),
                    &current.retained_properties(),
                    &environment
                ),
                expected
            );
        }
    }

    #[test]
    fn svg_transform_diff_does_not_invalidate_layout_or_hitboxes() {
        let environment = EnvironmentDiff {
            metrics: false,
            text_layout: false,
            text_paint: false,
            composite: false,
        };
        let previous = svg().path("a.svg").retained_properties();
        for transformation in [
            Transformation::translate(point(px(4.), px(2.))),
            Transformation::scale(size(2., 0.5)),
            Transformation::rotate(radians(0.5)),
        ] {
            let current = svg()
                .path("a.svg")
                .with_transformation(transformation)
                .retained_properties();
            assert_eq!(
                Svg::diff(&previous, &current, &environment),
                Damage::TRANSFORM | Damage::PAINT
            );
            assert!(Svg::diff(&current, &current, &environment).is_empty());
        }
    }
}

impl Element for Svg {
    fn uses_retained_diff(&self) -> bool {
        true
    }
    type RequestLayoutState = ();
    type PrepaintState = Option<Hitbox>;

    fn id(&self) -> Option<crate::ElementId> {
        self.interactivity.element_id.clone()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.interactivity.source_location()
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut retained_style = None;
        let layout_id = self.interactivity.request_layout(
            global_id,
            inspector_id,
            window,
            cx,
            |style, window, cx| {
                if window.retains_element_properties() {
                    retained_style = Some(style.clone());
                }
                window.request_layout(style, None, cx)
            },
        );
        if let Some(style) = retained_style {
            window.reconcile_retained_property_value::<Self>(
                self.retained_properties_with_style(style),
                Isolation::empty(),
                cx,
            );
        }
        (layout_id, ())
    }

    fn try_reuse_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<(LayoutId, Self::RequestLayoutState)> {
        let style = self.interactivity.retained_layout_style(cx)?;
        let layout = window.reuse_leaf_layout(&style)?;
        if window.retains_element_properties() {
            window.reconcile_retained_property_value::<Self>(
                self.retained_properties_with_style(style),
                Isolation::empty(),
                cx,
            );
        }
        Some((layout, ()))
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Hitbox> {
        self.interactivity.prepaint(
            global_id,
            inspector_id,
            bounds,
            bounds.size,
            window,
            cx,
            |_, _, hitbox, _, _| hitbox,
        )
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Option<Hitbox>,
        window: &mut Window,
        cx: &mut App,
    ) where
        Self: Sized,
    {
        self.interactivity.paint(
            global_id,
            inspector_id,
            bounds,
            hitbox.as_ref(),
            window,
            cx,
            |style, window, cx| {
                let transformation = self
                    .transformation
                    .as_ref()
                    .map(|transformation| {
                        transformation.into_matrix(bounds.center(), window.scale_factor())
                    })
                    .unwrap_or_default();

                if let Some((data, path)) = self.data.as_ref().zip(self.data_path.as_ref()) {
                    if let Some(color) = style.text.color {
                        window
                            .paint_retained_svg(
                                bounds,
                                path.clone(),
                                Some(&**data),
                                transformation,
                                color,
                                cx,
                            )
                            .log_err();
                    }
                } else if let Some((path, color)) =
                    self.external_path.as_ref().zip(style.text.color)
                {
                    let Some(bytes) = window
                        .use_asset::<SvgAsset>(path, cx)
                        .and_then(|asset| asset.log_err())
                    else {
                        return;
                    };

                    window
                        .paint_retained_svg(
                            bounds,
                            path.clone(),
                            Some(&bytes),
                            transformation,
                            color,
                            cx,
                        )
                        .log_err();
                } else if let Some((path, color)) = self.path.as_ref().zip(style.text.color) {
                    window
                        .paint_retained_svg(bounds, path.clone(), None, transformation, color, cx)
                        .log_err();
                }
            },
        )
    }
}

impl IntoElement for Svg {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for Svg {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl InteractiveElement for Svg {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

/// A transformation to apply to an SVG element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transformation {
    scale: Size<f32>,
    translate: Point<Pixels>,
    rotate: Radians,
}

impl Default for Transformation {
    fn default() -> Self {
        Self {
            scale: size(1.0, 1.0),
            translate: point(px(0.0), px(0.0)),
            rotate: radians(0.0),
        }
    }
}

impl Transformation {
    /// Create a new Transformation with the specified scale along each axis.
    pub fn scale(scale: Size<f32>) -> Self {
        Self {
            scale,
            translate: point(px(0.0), px(0.0)),
            rotate: radians(0.0),
        }
    }

    /// Create a new Transformation with the specified translation.
    pub fn translate(translate: Point<Pixels>) -> Self {
        Self {
            scale: size(1.0, 1.0),
            translate,
            rotate: radians(0.0),
        }
    }

    /// Create a new Transformation with the specified rotation in radians.
    pub fn rotate(rotate: impl Into<Radians>) -> Self {
        let rotate = rotate.into();
        Self {
            scale: size(1.0, 1.0),
            translate: point(px(0.0), px(0.0)),
            rotate,
        }
    }

    /// Update the scaling factor of this transformation.
    pub fn with_scaling(mut self, scale: Size<f32>) -> Self {
        self.scale = scale;
        self
    }

    /// Update the translation value of this transformation.
    pub fn with_translation(mut self, translate: Point<Pixels>) -> Self {
        self.translate = translate;
        self
    }

    /// Update the rotation angle of this transformation.
    pub fn with_rotation(mut self, rotate: impl Into<Radians>) -> Self {
        self.rotate = rotate.into();
        self
    }

    fn into_matrix(self, center: Point<Pixels>, scale_factor: f32) -> TransformationMatrix {
        //Note: if you read this as a sequence of matrix multiplications, start from the bottom
        TransformationMatrix::unit()
            .translate(center.scale(scale_factor) + self.translate.scale(scale_factor))
            .rotate(self.rotate)
            .scale(self.scale)
            .translate(center.scale(-scale_factor))
    }
}

enum SvgAsset {}

impl Asset for SvgAsset {
    type Source = SharedString;
    type Output = Result<Arc<[u8]>, Arc<std::io::Error>>;

    fn load(
        source: Self::Source,
        _cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        async move {
            let bytes = fs::read(Path::new(source.as_ref())).map_err(|e| Arc::new(e))?;
            let bytes = Arc::from(bytes);
            Ok(bytes)
        }
    }
}
