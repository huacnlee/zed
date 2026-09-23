use super::Damage;
use crate::text_system::{DecorationRun, LineLayoutIndex};
use crate::{
    AbsoluteLength, App, AtlasGeneration, BorderStyle, Bounds, BoxShadow, ContentMask, Corners,
    Edges, Fill, Hsla, ImageId, LineLayout, Pixels, Point, Primitive, RenderImage, ScaledPixels,
    ShapedLine, SharedString, Style, TextAlign, TextRenderingMode, TextStyle, TransformationMatrix,
    Window, WindowBackgroundAppearance, WrappedLine,
};
use smallvec::SmallVec;
use std::{ops::Range, rc::Rc, sync::Arc};

#[derive(Clone, PartialEq)]
struct PaintEnvironment {
    properties: PaintProperties,
    bounds: Bounds<Pixels>,
    clip: ContentMask<Pixels>,
    text_style: Option<(TextStyle, usize)>,
    text_rasterization: Option<(TextRenderingMode, WindowBackgroundAppearance, bool)>,
    rem_size: Pixels,
    scale_factor: f32,
    opacity: f32,
    active: bool,
}

#[derive(Clone, PartialEq)]
enum PaintProperties {
    Canvas(u64),
    Text(TextProperties),
    ShapedLine(ShapedLineProperties),
    Background(DecorationProperties),
    Border(DecorationProperties),
    Svg {
        path: SharedString,
        transformation: TransformationMatrix,
        color: Hsla,
    },
    Image {
        image_id: ImageId,
        frame_index: usize,
        image_bounds: Bounds<Pixels>,
        corner_radii: Corners<Pixels>,
        grayscale: bool,
    },
}

#[derive(Clone)]
struct TextProperties {
    lines: Rc<SmallVec<[WrappedLine; 1]>>,
    line_height: Pixels,
}

#[derive(Clone)]
struct ShapedLineProperties {
    layout: Arc<LineLayout>,
    decoration_runs: SmallVec<[DecorationRun; 32]>,
    line_height: Pixels,
    align: TextAlign,
    align_width: Option<Pixels>,
}

impl PartialEq for ShapedLineProperties {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.layout, &other.layout)
            && self.decoration_runs == other.decoration_runs
            && self.line_height == other.line_height
            && self.align == other.align
            && self.align_width == other.align_width
    }
}

impl PartialEq for TextProperties {
    fn eq(&self, other: &Self) -> bool {
        // Measured text restores these immutable lines only after comparing all
        // shaping inputs; retaining the Rc also prevents address reuse.
        Rc::ptr_eq(&self.lines, &other.lines) && self.line_height == other.line_height
    }
}

#[derive(Clone, PartialEq)]
struct DecorationProperties {
    background: Option<Fill>,
    border_color: Option<Hsla>,
    border_style: BorderStyle,
    border_widths: Edges<AbsoluteLength>,
    corner_radii: Corners<AbsoluteLength>,
    box_shadow: Vec<BoxShadow>,
}

pub(super) struct PaintSnapshot {
    environment: PaintEnvironment,
    pub(super) generation: u64,
    output: PaintOutput,
    pub(super) resource_generation: Option<AtlasGeneration>,
    text_layouts: Option<Box<Range<LineLayoutIndex>>>,
}

enum PaintOutput {
    FrameRange(Range<usize>),
    Geometry(Vec<Primitive>),
}

impl PaintSnapshot {
    pub(super) fn heap_bytes(&self) -> usize {
        let properties = match &self.environment.properties {
            PaintProperties::Canvas(_)
            | PaintProperties::Text(_)
            | PaintProperties::ShapedLine(_)
            | PaintProperties::Svg { .. }
            | PaintProperties::Image { .. } => 0,
            PaintProperties::Background(properties) | PaintProperties::Border(properties) => {
                properties.box_shadow.capacity() * size_of::<BoxShadow>()
            }
        };
        let output = match &self.output {
            PaintOutput::FrameRange(_) => 0,
            PaintOutput::Geometry(primitives) => primitives.capacity() * size_of::<Primitive>(),
        };
        properties
            + output
            + self
                .text_layouts
                .as_ref()
                .map_or(0, |_| size_of::<Range<LineLayoutIndex>>())
    }
}

impl PaintEnvironment {
    fn matches_translated(&self, previous: &Self) -> bool {
        let delta = (self.bounds.origin - previous.bounds.origin).scale(self.scale_factor);
        self.properties == previous.properties
            && self.bounds.size == previous.bounds.size
            && self.text_style == previous.text_style
            && self.text_rasterization == previous.text_rasterization
            && self.rem_size == previous.rem_size
            && self.scale_factor == previous.scale_factor
            && self.opacity == previous.opacity
            && self.active == previous.active
            // Fractional device-pixel movement can change edge snapping and shadow coverage.
            && delta.x.0.fract() == 0.
            && delta.y.0.fract() == 0.
    }
}

fn reusable_snapshot_index(
    snapshots: &[PaintSnapshot],
    environment: &PaintEnvironment,
    generation: u64,
    atlas_generation: Option<AtlasGeneration>,
    transform_enabled: bool,
    refreshing: bool,
    mut output_is_valid: impl FnMut(&PaintSnapshot) -> bool,
) -> Option<usize> {
    snapshots.iter().position(|snapshot| {
        !refreshing
            && snapshot.generation.wrapping_add(1) == generation
            && snapshot
                .resource_generation
                .is_none_or(|expected| Some(expected) == atlas_generation)
            && match &snapshot.output {
                PaintOutput::Geometry(_) if transform_enabled => {
                    environment.matches_translated(&snapshot.environment)
                }
                _ => snapshot.environment == *environment,
            }
            && output_is_valid(snapshot)
    })
}

fn translate_geometry(
    primitive: &mut Primitive,
    offset: Point<ScaledPixels>,
    clip: Option<ContentMask<ScaledPixels>>,
) -> bool {
    match primitive {
        Primitive::Quad(quad) => {
            quad.bounds.origin += offset;
            if let Some(clip) = clip {
                quad.content_mask = clip;
            }
        }
        Primitive::Shadow(shadow) => {
            shadow.bounds.origin += offset;
            shadow.element_bounds.origin += offset;
            if let Some(clip) = clip {
                shadow.content_mask = clip;
            }
        }
        Primitive::Path(path) => {
            path.bounds.origin += offset;
            path.content_mask = clip.unwrap_or_else(|| {
                path.content_mask.bounds.origin += offset;
                path.content_mask
            });
            for vertex in &mut path.vertices {
                vertex.xy_position += offset;
                vertex.content_mask = clip.unwrap_or_else(|| {
                    vertex.content_mask.bounds.origin += offset;
                    vertex.content_mask
                });
            }
        }
        Primitive::Underline(underline) => {
            underline.bounds.origin += offset;
            if let Some(clip) = clip {
                underline.content_mask = clip;
            } else {
                underline.content_mask.bounds.origin += offset;
            }
        }
        Primitive::MonochromeSprite(sprite) => {
            sprite.bounds.origin += offset;
            if let Some(clip) = clip {
                sprite.content_mask = clip;
            } else {
                sprite.content_mask.bounds.origin += offset;
            }
        }
        Primitive::SubpixelSprite(sprite) => {
            sprite.bounds.origin += offset;
            if let Some(clip) = clip {
                sprite.content_mask = clip;
            } else {
                sprite.content_mask.bounds.origin += offset;
            }
        }
        Primitive::PolychromeSprite(sprite) => {
            sprite.bounds.origin += offset;
            if let Some(clip) = clip {
                sprite.content_mask = clip;
            } else {
                sprite.content_mask.bounds.origin += offset;
            }
        }
        Primitive::Surface(_) => return false,
    }
    true
}

impl Window {
    pub(crate) fn paint_retained_svg(
        &mut self,
        bounds: Bounds<Pixels>,
        path: SharedString,
        data: Option<&[u8]>,
        transformation: TransformationMatrix,
        color: Hsla,
        cx: &mut App,
    ) -> anyhow::Result<()> {
        if !self.retained_tree.active || !self.retained_tree.paint_enabled {
            return self.paint_svg(bounds, path, data, transformation, color, cx);
        }
        let properties = PaintProperties::Svg {
            path: path.clone(),
            transformation,
            color,
        };
        let mut result = Ok(());
        self.paint_retained_output(properties, bounds, cx, |window, cx| {
            result = window.paint_svg(bounds, path, data, transformation, color, cx);
            result.is_ok()
        });
        result
    }

    pub(crate) fn paint_retained_image(
        &mut self,
        bounds: Bounds<Pixels>,
        image_bounds: Bounds<Pixels>,
        corner_radii: Corners<Pixels>,
        data: Arc<RenderImage>,
        frame_index: usize,
        grayscale: bool,
        cx: &mut App,
    ) -> anyhow::Result<()> {
        if !self.retained_tree.active || !self.retained_tree.paint_enabled {
            return self.paint_image(
                bounds,
                image_bounds,
                corner_radii,
                data,
                frame_index,
                grayscale,
            );
        }
        let properties = PaintProperties::Image {
            image_id: data.id,
            frame_index,
            image_bounds,
            corner_radii,
            grayscale,
        };
        let mut result = Ok(());
        self.paint_retained_output(properties, bounds, cx, |window, _| {
            result = window.paint_image(
                bounds,
                image_bounds,
                corner_radii,
                data,
                frame_index,
                grayscale,
            );
            result.is_ok()
        });
        result
    }

    pub(crate) fn paint_retained_geometry(
        &mut self,
        revision: u64,
        bounds: Bounds<Pixels>,
        cx: &mut App,
        paint: impl FnOnce(&mut Window, &mut App),
    ) {
        self.paint_retained_output(
            PaintProperties::Canvas(revision),
            bounds,
            cx,
            |window, cx| {
                paint(window, cx);
                true
            },
        );
    }

    pub(crate) fn paint_retained_text(
        &mut self,
        lines: &Rc<SmallVec<[WrappedLine; 1]>>,
        line_height: Pixels,
        bounds: Bounds<Pixels>,
        cx: &mut App,
        paint: impl FnOnce(&mut Window, &mut App) -> bool,
    ) {
        if !self.retained_tree.active || !self.retained_tree.paint_enabled {
            paint(self, cx);
            return;
        }
        self.paint_retained_output(
            PaintProperties::Text(TextProperties {
                lines: lines.clone(),
                line_height,
            }),
            bounds,
            cx,
            paint,
        );
    }

    pub(crate) fn paint_retained_shaped_line(
        &mut self,
        line: &ShapedLine,
        origin: Point<Pixels>,
        line_height: Pixels,
        align: TextAlign,
        align_width: Option<Pixels>,
        cx: &mut App,
    ) -> anyhow::Result<()> {
        if !self.retained_tree.active || !self.retained_tree.paint_enabled {
            return line.paint(origin, line_height, align, align_width, self, cx);
        }
        let properties = PaintProperties::ShapedLine(ShapedLineProperties {
            layout: line.layout.clone(),
            decoration_runs: line.decoration_runs.clone(),
            line_height,
            align,
            align_width,
        });
        let bounds = Bounds::new(origin, crate::size(line.width(), line_height));
        let mut result = Ok(());
        self.paint_retained_output(properties, bounds, cx, |window, cx| {
            result = line.paint(origin, line_height, align, align_width, window, cx);
            result.is_ok()
        });
        result
    }

    pub(crate) fn paint_retained_background(
        &mut self,
        style: &Style,
        bounds: Bounds<Pixels>,
        cx: &mut App,
        paint: impl FnOnce(&mut Window, &mut App),
    ) {
        if !self.retained_tree.active
            || !self.retained_tree.paint_enabled
            || (style.background.is_none() && style.box_shadow.is_empty())
        {
            paint(self, cx);
            return;
        }
        let properties = DecorationProperties {
            background: style.background.clone(),
            border_color: None,
            border_style: style.border_style,
            border_widths: Edges::default(),
            corner_radii: style.corner_radii,
            box_shadow: style.box_shadow.clone(),
        };
        self.paint_retained_output(
            PaintProperties::Background(properties),
            bounds,
            cx,
            |window, cx| {
                paint(window, cx);
                true
            },
        );
    }

    pub(crate) fn paint_retained_border(
        &mut self,
        style: &Style,
        bounds: Bounds<Pixels>,
        cx: &mut App,
        paint: impl FnOnce(&mut Window, &mut App),
    ) {
        if !self.retained_tree.active || !self.retained_tree.paint_enabled {
            paint(self, cx);
            return;
        }
        let properties = DecorationProperties {
            background: None,
            border_color: style.border_color,
            border_style: style.border_style,
            border_widths: style.border_widths,
            corner_radii: style.corner_radii,
            box_shadow: Vec::new(),
        };
        self.paint_retained_output(
            PaintProperties::Border(properties),
            bounds,
            cx,
            |window, cx| {
                paint(window, cx);
                true
            },
        );
    }

    fn paint_retained_output(
        &mut self,
        properties: PaintProperties,
        bounds: Bounds<Pixels>,
        cx: &mut App,
        paint: impl FnOnce(&mut Window, &mut App) -> bool,
    ) {
        let node_id = self.retained_tree.current;
        let mut enabled = self.retained_tree.active && self.retained_tree.paint_enabled;
        #[cfg(any(feature = "inspector", debug_assertions))]
        {
            enabled &= !self.inspector_enabled();
        }
        if !enabled || node_id.is_none() || self.a11y.is_active() {
            paint(self, cx);
            return;
        }
        let text = matches!(
            &properties,
            PaintProperties::Canvas(_) | PaintProperties::Text(_) | PaintProperties::ShapedLine(_)
        );
        let sprite = matches!(
            &properties,
            PaintProperties::Svg { .. } | PaintProperties::Image { .. }
        );
        let transformed_text = matches!(
            &properties,
            PaintProperties::Text(_) | PaintProperties::ShapedLine(_)
        );
        let measured_text = matches!(&properties, PaintProperties::Text(_));
        let text_style = text.then(|| (self.text_style(), cx.text_system().font_generation()));
        let atlas_generation = if text || sprite {
            self.atlas_generation()
        } else {
            None
        };
        let capture_geometry = matches!(&properties, PaintProperties::Background(_))
            || (self.retained_tree.transform_enabled && transformed_text);
        let environment = PaintEnvironment {
            properties,
            bounds,
            clip: self.content_mask(),
            text_style,
            text_rasterization: text.then(|| self.text_rasterization_environment()),
            rem_size: self.rem_size(),
            scale_factor: self.scale_factor(),
            opacity: self.element_opacity(),
            active: self.is_window_active(),
        };
        let generation = self.retained_tree.generation;
        let previous_index = node_id
            .and_then(|id| self.retained_tree.nodes.get(id))
            .and_then(|node| {
                reusable_snapshot_index(
                    &node.paint_snapshots,
                    &environment,
                    generation,
                    atlas_generation,
                    self.retained_tree.transform_enabled,
                    self.refreshing,
                    |snapshot| match &snapshot.output {
                        PaintOutput::FrameRange(range) => {
                            if snapshot.resource_generation.is_some() {
                                self.rendered_frame
                                    .scene
                                    .retained_range_uses_atlas(range.clone())
                                    == Some(true)
                            } else {
                                self.rendered_frame.scene.is_geometry_range(range.clone())
                            }
                        }
                        PaintOutput::Geometry(_) => true,
                    },
                )
            });
        let previous = node_id
            .zip(previous_index)
            .and_then(|(id, index)| {
                self.retained_tree
                    .nodes
                    .get_mut(id)
                    .map(|node| (node, index))
            })
            .map(|(node, index)| node.paint_snapshots.swap_remove(index));
        let start = self.paint_index();
        let text_start = text.then(|| self.text_system().layout_index());
        let mut geometry = None;
        let mut cacheable = true;
        if let Some(snapshot) = previous {
            if let Some(text_layouts) = snapshot.text_layouts {
                self.text_system().reuse_layouts(*text_layouts);
            }
            if snapshot.environment.bounds.origin != bounds.origin
                || snapshot.environment.clip != environment.clip
            {
                self.retained_tree.stats.transform_only += 1;
                if snapshot.environment.bounds.origin != bounds.origin {
                    self.retained_tree.damage_current(Damage::TRANSFORM);
                }
                if snapshot.environment.clip != environment.clip {
                    self.retained_tree.damage_current(Damage::CLIP);
                }
            }
            match snapshot.output {
                PaintOutput::FrameRange(range) => self
                    .next_frame
                    .scene
                    .replay(range, &self.rendered_frame.scene),
                PaintOutput::Geometry(primitives) => {
                    let offset = bounds.origin.scale(environment.scale_factor);
                    let clip = self.snapped_content_mask();
                    if matches!(
                        snapshot.environment.properties,
                        PaintProperties::Text(_) | PaintProperties::ShapedLine(_)
                    ) {
                        let translated = primitives.iter().filter_map(|primitive| {
                            let mut primitive = primitive.clone();
                            if translate_geometry(&mut primitive, offset, Some(clip)) {
                                Some(primitive)
                            } else {
                                None
                            }
                        });
                        let clipped_bounds = bounds.intersect(&self.content_mask().bounds);
                        if !clipped_bounds.is_empty() {
                            let layer_bounds = self.cover_bounds(clipped_bounds);
                            self.next_frame
                                .scene
                                .insert_retained_layer(layer_bounds, translated);
                        }
                    } else {
                        for primitive in &primitives {
                            let mut primitive = primitive.clone();
                            if translate_geometry(&mut primitive, offset, Some(clip)) {
                                self.next_frame.scene.insert_primitive(primitive);
                            }
                        }
                    }
                    geometry = Some(primitives);
                }
            }
            self.retained_tree.stats.paint_replayed += 1;
        } else {
            self.retained_tree.damage_current(Damage::PAINT);
            if capture_geometry {
                self.next_frame.scene.begin_geometry_capture();
            }
            cacheable = paint(self, cx);
            if capture_geometry {
                let mut primitives = self.next_frame.scene.finish_geometry_capture();
                let offset = Point::default() - bounds.origin.scale(environment.scale_factor);
                let safe_text_layer = !measured_text
                    || primitives.iter().all(|primitive| {
                        matches!(
                            primitive,
                            Primitive::MonochromeSprite(_)
                                | Primitive::SubpixelSprite(_)
                                | Primitive::PolychromeSprite(_)
                        )
                    });
                if safe_text_layer
                    && primitives
                        .iter_mut()
                        .all(|primitive| translate_geometry(primitive, offset, None))
                {
                    geometry = Some(primitives);
                }
            }
        }
        let end = self.paint_index();
        let range = self
            .retained_paint_range(start..end)
            .and_then(|(range, uses_atlas)| {
                // Failed or not-yet-ready assets must be retried, not retained as empty output.
                if sprite && !uses_atlas {
                    return None;
                }
                if !uses_atlas {
                    Some((range, None))
                } else {
                    let generation = atlas_generation?;
                    (self.atlas_generation() == Some(generation))
                        .then_some((range, Some(generation)))
                }
            });
        let text_layouts =
            text_start.map(|start| Box::new(start..self.text_system().layout_index()));
        if let Some(node) = node_id.and_then(|id| self.retained_tree.nodes.get_mut(id)) {
            if cacheable && let Some((range, resource_generation)) = range {
                node.paint_snapshots.push(PaintSnapshot {
                    environment,
                    generation,
                    output: geometry.map_or(PaintOutput::FrameRange(range), PaintOutput::Geometry),
                    resource_generation,
                    text_layouts,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retained::RetainedElementTree;
    use crate::{
        AtlasTextureId, AtlasTextureKind, AtlasTile, MonochromeSprite, Primitive, Quad, Scene,
        TileId, point, scene::PaintOperation,
    };
    use std::any::TypeId;

    fn snapshot(generation: u64, primitives: usize) -> PaintSnapshot {
        PaintSnapshot {
            environment: PaintEnvironment {
                properties: PaintProperties::Canvas(0),
                bounds: Bounds::default(),
                clip: ContentMask::default(),
                text_style: None,
                text_rasterization: None,
                rem_size: crate::px(16.),
                scale_factor: 1.,
                opacity: 1.,
                active: true,
            },
            generation,
            output: PaintOutput::Geometry(vec![Primitive::Quad(Quad::default()); primitives]),
            resource_generation: None,
            text_layouts: None,
        }
    }

    #[test]
    fn budget_evicts_largest_paint_cache_first() {
        let mut tree = RetainedElementTree::new(true);
        tree.begin_frame();
        let small = tree
            .begin_element(Some("small".into()), TypeId::of::<()>())
            .expect("active");
        let large = tree
            .begin_element(Some("large".into()), TypeId::of::<()>())
            .expect("active");
        let small_snapshot = snapshot(tree.generation, 1);
        let expected_bytes = size_of::<PaintSnapshot>() + small_snapshot.heap_bytes();
        tree.nodes.get_mut(small).expect("node").paint_snapshots = vec![small_snapshot];
        tree.nodes.get_mut(large).expect("node").paint_snapshots =
            vec![snapshot(tree.generation, 100)];
        tree.budget.max_snapshot_bytes = expected_bytes;
        tree.finish_frame();
        assert_eq!(tree.stats.snapshot_bytes, expected_bytes);
        assert_eq!(tree.stats.snapshots_evicted, 1);
        assert_eq!(
            tree.nodes.get(small).expect("node").paint_snapshots.len(),
            1
        );
        assert_eq!(
            tree.nodes
                .get(large)
                .expect("node")
                .paint_snapshots
                .capacity(),
            0
        );
    }

    #[test]
    fn unused_snapshots_expire_without_removing_live_nodes() {
        let mut tree = RetainedElementTree::new(true);
        tree.begin_frame();
        let node = tree
            .begin_element(Some("node".into()), TypeId::of::<()>())
            .expect("active");
        tree.nodes.get_mut(node).expect("node").paint_snapshots =
            vec![snapshot(tree.generation, 1)];
        tree.finish_frame();
        for age in 1..=3 {
            tree.begin_frame();
            assert_eq!(
                tree.begin_element(Some("node".into()), TypeId::of::<()>()),
                Some(node)
            );
            tree.finish_frame();
            assert_eq!(tree.stats.snapshot_bytes == 0, age > 2);
            assert_eq!(tree.stats.snapshots_evicted, usize::from(age > 2));
            assert_eq!(tree.stats.nodes_removed, 0);
        }
    }

    #[test]
    fn empty_snapshot_storage_is_released() {
        let mut tree = RetainedElementTree::new(true);
        tree.begin_frame();
        let node = tree
            .begin_element(Some("node".into()), TypeId::of::<()>())
            .expect("active");
        tree.nodes.get_mut(node).expect("node").paint_snapshots = Vec::with_capacity(4);
        tree.finish_frame();
        assert_eq!(tree.stats.snapshot_bytes, 0);
        assert_eq!(
            tree.nodes
                .get(node)
                .expect("node")
                .paint_snapshots
                .capacity(),
            0
        );
    }

    #[test]
    fn geometry_snapshot_requires_valid_balanced_range() {
        let mut scene = Scene::default();
        scene.paint_operations = vec![
            PaintOperation::StartLayer(Default::default()),
            PaintOperation::Primitive(Primitive::Quad(Quad::default())),
            PaintOperation::EndLayer,
        ];
        assert!(scene.is_geometry_range(0..3));
        assert!(scene.is_geometry_range(1..2));
        assert!(!scene.is_geometry_range(0..2));
        assert!(!scene.is_geometry_range(1..3));
        assert!(!scene.is_geometry_range(0..4));
    }

    #[test]
    fn translated_text_sprite_uses_new_origin_and_clip() {
        let old_clip = ContentMask {
            bounds: Bounds::new(
                point(ScaledPixels(2.), ScaledPixels(3.)),
                Default::default(),
            ),
        };
        let new_clip = ContentMask {
            bounds: Bounds::new(
                point(ScaledPixels(20.), ScaledPixels(30.)),
                Default::default(),
            ),
        };
        let mut primitive = Primitive::MonochromeSprite(MonochromeSprite {
            order: 0,
            pad: 0,
            bounds: Bounds::new(
                point(ScaledPixels(4.), ScaledPixels(5.)),
                Default::default(),
            ),
            content_mask: old_clip,
            color: Default::default(),
            tile: AtlasTile {
                texture_id: AtlasTextureId {
                    index: 0,
                    kind: AtlasTextureKind::Monochrome,
                },
                tile_id: TileId(0),
                padding: 0,
                bounds: Default::default(),
            },
            transformation: Default::default(),
        });

        assert!(translate_geometry(
            &mut primitive,
            point(ScaledPixels(7.), ScaledPixels(11.)),
            Some(new_clip),
        ));
        let Primitive::MonochromeSprite(sprite) = primitive else {
            panic!("expected monochrome sprite");
        };
        assert_eq!(
            sprite.bounds.origin,
            point(ScaledPixels(11.), ScaledPixels(16.))
        );
        assert_eq!(sprite.content_mask, new_clip);
    }

    #[test]
    fn reusable_snapshot_matches_full_paint_environment() {
        let mut first = snapshot(4, 1);
        first.environment.properties = PaintProperties::Canvas(1);
        let mut second = snapshot(4, 1);
        second.environment.properties = PaintProperties::Canvas(2);
        let snapshots = vec![first, second];
        let mut environment = snapshots[1].environment.clone();
        environment.bounds.origin.x += crate::px(10.);

        assert_eq!(
            reusable_snapshot_index(&snapshots, &environment, 5, None, true, false, |_| true),
            Some(1)
        );
    }
}
