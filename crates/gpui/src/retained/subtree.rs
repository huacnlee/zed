use super::prepaint::PrepaintSnapshot;
use super::{Damage, ReconcileKey, RetainedElementTree, Undo};
use crate::{
    App, AtlasGeneration, Bounds, ContentMask, EntityId, GlobalElementId, PaintIndex, Pixels,
    PrepaintStateIndex, TextRenderingMode, TextStyle, Window, WindowBackgroundAppearance,
};
use collections::FxHashSet;
use std::ops::Range;

#[derive(Clone, PartialEq)]
struct SubtreeEnvironment {
    bounds: Bounds<Pixels>,
    content_mask: ContentMask<Pixels>,
    text_style: TextStyle,
    rem_size: Pixels,
    scale_factor: f32,
    opacity: f32,
    active: bool,
    font_generation: usize,
    rasterization: (TextRenderingMode, WindowBackgroundAppearance, bool),
}

impl SubtreeEnvironment {
    fn capture(bounds: Bounds<Pixels>, window: &Window, cx: &App) -> Self {
        Self {
            bounds,
            content_mask: window.content_mask(),
            text_style: window.text_style(),
            rem_size: window.rem_size(),
            scale_factor: window.scale_factor(),
            opacity: window.element_opacity(),
            active: window.is_window_active(),
            font_generation: cx.text_system().font_generation(),
            rasterization: window.text_rasterization_environment(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct SubtreeSnapshot {
    prepaint: PrepaintSnapshot,
    pub paint_range: Range<PaintIndex>,
    pub accessed_entities: FxHashSet<EntityId>,
    pub(super) generation: u64,
    environment: SubtreeEnvironment,
    atlas_generation: Option<AtlasGeneration>,
    reusable: bool,
}

impl SubtreeSnapshot {
    pub(crate) fn new(
        bounds: Bounds<Pixels>,
        prepaint_range: Range<PrepaintStateIndex>,
        accessed_entities: FxHashSet<EntityId>,
        window: &Window,
        cx: &App,
    ) -> Self {
        Self {
            prepaint: PrepaintSnapshot::new(prepaint_range, window),
            paint_range: PaintIndex::default()..PaintIndex::default(),
            accessed_entities,
            generation: window.retained_tree.generation,
            environment: SubtreeEnvironment::capture(bounds, window, cx),
            atlas_generation: None,
            reusable: false,
        }
    }

    pub(crate) fn can_reuse(&self, bounds: Bounds<Pixels>, window: &Window, cx: &App) -> bool {
        self.reusable
            && self.prepaint.is_previous_frame(window)
            && self.generation.wrapping_add(1) == window.retained_tree.generation
            && self.environment == SubtreeEnvironment::capture(bounds, window, cx)
            && self
                .atlas_generation
                .is_none_or(|generation| window.atlas_generation() == Some(generation))
            && (!window.retained_tree.active
                || (window.retained_tree.prepaint_enabled
                    && window.retained_tree.paint_enabled
                    && !window
                        .retained_tree
                        .current_damage()
                        .contains(Damage::BUILD)))
            && !window.a11y.is_active()
    }

    pub(crate) fn finish_paint(&mut self, range: Range<PaintIndex>, window: &Window) {
        self.reusable = match window
            .next_frame
            .scene
            .retained_range_uses_atlas(range.start.scene_index..range.end.scene_index)
        {
            Some(false) => true,
            Some(true) => {
                self.atlas_generation = window.atlas_generation();
                self.atlas_generation.is_some()
            }
            None => false,
        };
        self.environment.opacity = window.element_opacity();
        self.paint_range = range;
        self.generation = window.retained_tree.generation;
    }

    pub(crate) fn replay_prepaint(&mut self, window: &mut Window) -> bool {
        self.prepaint.replay(window)
    }

    pub(super) fn owned_bytes(&self) -> usize {
        size_of::<Self>() + self.accessed_entities.capacity() * size_of::<EntityId>()
    }
}

impl Window {
    pub(crate) fn with_retained_subtree<R>(
        &mut self,
        global_id: &GlobalElementId,
        update: impl FnOnce(Option<Box<SubtreeSnapshot>>, &mut Self) -> (R, Box<SubtreeSnapshot>),
    ) -> R {
        let node_id = self
            .retained_tree
            .current
            .filter(|_| self.retained_tree.active);
        let Some(node_id) = node_id else {
            return self.with_element_state(global_id, update);
        };
        let previous = self
            .retained_tree
            .nodes
            .get_mut(node_id)
            .and_then(|node| node.subtree_snapshot.take());
        if self.retained_tree.transactions > 0 {
            self.retained_tree
                .undo
                .push(Undo::Subtree(node_id, previous.clone()));
        }
        let (result, snapshot) = update(previous, self);
        if let Some(node) = self.retained_tree.nodes.get_mut(node_id) {
            node.subtree_snapshot = Some(snapshot);
            self.retained_tree.subtree_nodes.insert(node_id);
        }
        result
    }
}

impl RetainedElementTree {
    pub(crate) fn dependent_views(&self, changed: &FxHashSet<EntityId>) -> Vec<EntityId> {
        if changed.is_empty() {
            return Vec::new();
        }
        self.subtree_nodes
            .iter()
            .filter_map(|id| {
                let node = self.nodes.get(*id)?;
                let snapshot = node.subtree_snapshot.as_ref()?;
                let ReconcileKey::View(owner) = &node.key else {
                    return None;
                };
                snapshot
                    .accessed_entities
                    .iter()
                    .any(|dependency| changed.contains(dependency))
                    .then_some(*owner)
            })
            .collect()
    }
}
