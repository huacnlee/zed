use crate::{ElementId, EntityId, LayoutId};
use collections::{FxHashMap, FxHashSet};
use slotmap::{SlotMap, new_key_type};
use std::any::TypeId;
mod damage;
pub(crate) use damage::{Damage, Isolation};

#[cfg(any(feature = "inspector", debug_assertions))]
mod debug;
#[cfg(any(feature = "inspector", debug_assertions))]
use debug::InspectorNode;
mod budget;
mod properties;
mod style;
use budget::RetainedBudget;
use properties::PropertySnapshot;
pub(crate) use properties::{EnvironmentDiff, RetainableElement};
mod paint;
use paint::PaintSnapshot;
mod prepaint;
use prepaint::HitboxSnapshot;
mod handlers;
use handlers::HandlerShape;
pub(crate) use handlers::{HandlerKind, HandlerSlotId, HandlerTable, MouseHandler, SlottedHandler};
mod subtree;
pub(crate) use subtree::SubtreeSnapshot;

new_key_type! {
    pub(crate) struct RetainedNodeId;
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ReconcileKey {
    Explicit(ElementId),
    View(EntityId),
    Positional { element_type: TypeId, slot: usize },
}

struct RetainedNode {
    damage: Damage,
    pending_damage: Damage,
    isolation: Isolation,
    key: ReconcileKey,
    element_type: TypeId,
    parent: Option<RetainedNodeId>,
    children: Vec<RetainedNodeId>,
    next_children: Vec<RetainedNodeId>,
    seen: bool,
    layouts: Vec<LayoutId>,
    layout_cursor: usize,
    handler_shapes: Vec<HandlerShape>,
    handler_cursor: usize,
    properties: Option<Box<PropertySnapshot>>,
    paint_snapshots: Vec<PaintSnapshot>,
    hitbox_snapshot: Option<Box<HitboxSnapshot>>,
    subtree_snapshot: Option<Box<SubtreeSnapshot>>,
    #[cfg(any(feature = "inspector", debug_assertions))]
    inspector: Option<Box<InspectorNode>>,
}

#[derive(Clone, Default, Debug)]
pub(crate) struct RetainedFrameStats {
    pub nodes_total: usize,
    pub nodes_created: usize,
    pub nodes_removed: usize,
    pub nodes_reconciled: usize,
    pub layout_recomputed: usize,
    pub layout_reused: usize,
    pub measure_recomputed: usize,
    pub prepaint_rebuilt: usize,
    pub prepaint_reused: usize,
    pub hitboxes_replayed: usize,
    pub handlers_updated: usize,
    pub handlers_replayed: usize,
    pub paint_rebuilt: usize,
    pub paint_replayed: usize,
    pub transform_only: usize,
    pub scene_order_reused: usize,
    pub snapshot_bytes: usize,
    pub property_bytes: usize,
    pub snapshots_evicted: usize,
}

/// Retained-rendering counters exposed to production benchmarks.
#[cfg(feature = "bench-support")]
#[derive(Clone, Copy, Debug)]
pub struct RetainedFrameSnapshot {
    /// Whether the retained tree is enabled for this window.
    pub enabled: bool,
    /// Number of live retained nodes after the last frame.
    pub nodes_total: usize,
    /// Number of live nodes retaining typed properties.
    pub property_snapshots: usize,
    /// Bytes owned by typed property snapshots.
    pub property_bytes: usize,
    /// Bytes charged to the replay snapshot budget.
    pub snapshot_bytes: usize,
    /// Replay snapshots evicted while finishing the last frame.
    pub snapshots_evicted: usize,
    /// Layout nodes reused during the last frame.
    pub layout_reused: usize,
    /// Retained nodes reconciled during the last frame.
    pub nodes_reconciled: usize,
    /// Prepaint snapshots reused during the last frame.
    pub prepaint_reused: usize,
    /// Paint ranges replayed during the last frame.
    pub paint_replayed: usize,
    /// Nodes updated using transform-only composition during the last frame.
    pub transform_only: usize,
}

pub(crate) struct RetainedElementTree {
    enabled: bool,
    layout_enabled: bool,
    prepaint_enabled: bool,
    paint_enabled: bool,
    transform_enabled: bool,
    generation: u64,
    pub effect_generation: u64,
    active: bool,
    nodes: SlotMap<RetainedNodeId, RetainedNode>,
    view_nodes: FxHashMap<EntityId, Vec<RetainedNodeId>>,
    subtree_nodes: FxHashSet<RetainedNodeId>,
    identities: FxHashMap<(Option<RetainedNodeId>, ReconcileKey), RetainedNodeId>,
    current: Option<RetainedNodeId>,
    pub(crate) handler_target: Option<crate::HitboxId>,
    pub(crate) handler_dispatch_target: Option<crate::DispatchNodeId>,
    root_slot: usize,
    duplicate_warned: bool,
    transactions: usize,
    undo: Vec<Undo>,
    budget: RetainedBudget,
    pub stats: RetainedFrameStats,
    #[cfg(any(feature = "inspector", debug_assertions))]
    completed_stats: Option<(u64, RetainedFrameStats)>,
}

enum Undo {
    Insert {
        id: RetainedNodeId,
        identity: (Option<RetainedNodeId>, ReconcileKey),
        previous: Option<RetainedNodeId>,
    },
    Seen(RetainedNodeId, bool),
    Child(RetainedNodeId),
    Children(RetainedNodeId, Vec<RetainedNodeId>),
    Layouts(RetainedNodeId, Vec<LayoutId>, usize),
    Hitbox(RetainedNodeId, Option<Box<HitboxSnapshot>>),
    Subtree(RetainedNodeId, Option<Box<SubtreeSnapshot>>),
    Damage(RetainedNodeId, Damage, Isolation),
    Handlers(RetainedNodeId, Vec<HandlerShape>, usize),
    Properties(RetainedNodeId, Option<Box<PropertySnapshot>>),
}

pub(crate) struct RetainedCheckpoint {
    undo_length: usize,
    root_slot: usize,
    current: Option<RetainedNodeId>,
    stats: RetainedFrameStats,
}

impl Default for RetainedElementTree {
    fn default() -> Self {
        Self::new(std::env::var("GPUI_RETAINED_TREE").is_ok_and(|value| value == "1"))
    }
}

impl RetainedElementTree {
    #[cfg(feature = "bench-support")]
    pub(super) fn bench_snapshot(&self) -> RetainedFrameSnapshot {
        RetainedFrameSnapshot {
            enabled: self.enabled,
            nodes_total: self.stats.nodes_total,
            property_snapshots: self
                .nodes
                .values()
                .filter(|node| node.properties.is_some())
                .count(),
            property_bytes: self.stats.property_bytes,
            snapshot_bytes: self.stats.snapshot_bytes,
            snapshots_evicted: self.stats.snapshots_evicted,
            layout_reused: self.stats.layout_reused,
            nodes_reconciled: self.stats.nodes_reconciled,
            prepaint_reused: self.stats.prepaint_reused,
            paint_replayed: self.stats.paint_replayed,
            transform_only: self.stats.transform_only,
        }
    }

    #[cfg(feature = "bench-support")]
    pub(super) fn set_bench_snapshot_budget(&mut self, bytes: usize) {
        self.budget.max_snapshot_bytes = bytes;
    }

    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            layout_enabled: std::env::var("GPUI_RETAINED_LAYOUT")
                .map_or(true, |value| value != "0"),
            prepaint_enabled: std::env::var("GPUI_RETAINED_PREPAINT")
                .map_or(true, |value| value != "0"),
            paint_enabled: std::env::var("GPUI_RETAINED_PAINT").map_or(true, |value| value != "0"),
            transform_enabled: std::env::var("GPUI_RETAINED_TRANSFORM")
                .map_or(true, |value| value != "0"),
            generation: 0,
            effect_generation: 0,
            active: false,
            nodes: SlotMap::with_key(),
            view_nodes: FxHashMap::default(),
            subtree_nodes: FxHashSet::default(),
            identities: FxHashMap::default(),
            current: None,
            handler_target: None,
            handler_dispatch_target: None,
            root_slot: 0,
            duplicate_warned: false,
            transactions: 0,
            undo: Vec::new(),
            budget: RetainedBudget::default(),
            stats: RetainedFrameStats::default(),
            #[cfg(any(feature = "inspector", debug_assertions))]
            completed_stats: None,
        }
    }

    pub fn begin_frame(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.active = self.enabled;
        self.current = None;
        self.handler_target = None;
        self.handler_dispatch_target = None;
        self.root_slot = 0;
        self.stats = RetainedFrameStats::default();
        self.stats.nodes_total = self.nodes.len();
        for node in self.nodes.values_mut() {
            node.damage = std::mem::take(&mut node.pending_damage);
            node.seen = false;
            node.layout_cursor = 0;
            node.handler_cursor = 0;
            node.next_children.clear();
        }
    }

    pub fn begin_element(
        &mut self,
        element_id: Option<ElementId>,
        element_type: TypeId,
    ) -> Option<RetainedNodeId> {
        if !self.active {
            return None;
        }
        let parent = self.current;
        let slot = if let Some(parent) = parent.and_then(|parent| self.nodes.get(parent)) {
            parent.next_children.len()
        } else {
            let slot = self.root_slot;
            self.root_slot += 1;
            slot
        };
        let mut key = match element_id {
            Some(ElementId::View(view)) => ReconcileKey::View(view),
            Some(id) => ReconcileKey::Explicit(id),
            None => ReconcileKey::Positional { element_type, slot },
        };
        let mut previous = self.identities.get(&(parent, key.clone())).copied();
        if previous
            .and_then(|id| self.nodes.get(id))
            .is_some_and(|node| node.seen)
        {
            let mut path = vec![format!("{key:?}")];
            let mut ancestor = parent;
            while let Some(node) = ancestor.and_then(|id| self.nodes.get(id)) {
                path.push(format!("{:?}", node.key));
                ancestor = node.parent;
            }
            path.reverse();
            debug_assert!(
                false,
                "duplicate retained element key: {}",
                path.join(" / ")
            );
            if !self.duplicate_warned {
                log::warn!("duplicate retained element key: {}", path.join(" / "));
                self.duplicate_warned = true;
            }
            key = ReconcileKey::Positional { element_type, slot };
            previous = self.identities.get(&(parent, key.clone())).copied();
        }
        let id = match previous.filter(|id| {
            self.nodes
                .get(*id)
                .is_some_and(|node| node.element_type == element_type && !node.seen)
        }) {
            Some(id) => id,
            None => {
                self.stats.nodes_created += 1;
                let id = self.nodes.insert(RetainedNode {
                    damage: Damage::FULL,
                    pending_damage: Damage::empty(),
                    isolation: Isolation::empty(),
                    key: key.clone(),
                    element_type,
                    parent,
                    children: Vec::new(),
                    next_children: Vec::new(),
                    seen: false,
                    layouts: Vec::new(),
                    layout_cursor: 0,
                    handler_shapes: Vec::new(),
                    handler_cursor: 0,
                    properties: None,
                    paint_snapshots: Vec::new(),
                    hitbox_snapshot: None,
                    subtree_snapshot: None,
                    #[cfg(any(feature = "inspector", debug_assertions))]
                    inspector: None,
                });
                if let ReconcileKey::View(view) = &key {
                    self.view_nodes.entry(*view).or_default().push(id);
                }
                let identity = (parent, key);
                let previous = self.identities.insert(identity.clone(), id);
                if self.transactions > 0 {
                    self.undo.push(Undo::Insert {
                        id,
                        identity,
                        previous,
                    });
                }
                id
            }
        };
        if let Some(node) = self.nodes.get_mut(id) {
            if self.transactions > 0 {
                self.undo.push(Undo::Seen(id, node.seen));
            }
            node.seen = true;
        }
        if let Some(parent_id) = parent {
            if let Some(parent) = self.nodes.get_mut(parent_id) {
                if self.transactions > 0 {
                    self.undo.push(Undo::Child(parent_id));
                }
                parent.next_children.push(id);
            }
        }
        self.stats.nodes_reconciled += 1;
        Some(id)
    }

    pub fn enter(&mut self, node: Option<RetainedNodeId>) -> Option<RetainedNodeId> {
        std::mem::replace(&mut self.current, node)
    }

    pub fn record_effect(&mut self) {
        self.effect_generation = self.effect_generation.wrapping_add(1);
    }

    pub fn suspend(&mut self) -> bool {
        std::mem::replace(&mut self.active, false)
    }

    pub fn resume(&mut self, active: bool) {
        self.active = active;
    }

    pub fn retains_layout(&self) -> bool {
        self.enabled && self.layout_enabled
    }

    pub fn retains_current_layout(&self) -> bool {
        self.active && self.retains_layout() && self.current.is_some()
    }

    pub fn retains_prepaint(&self) -> bool {
        self.active && self.prepaint_enabled && self.current.is_some()
    }

    pub fn retains_paint(&self) -> bool {
        self.enabled && self.paint_enabled
    }

    pub fn previous_layout(&self) -> Option<LayoutId> {
        if !self.active || !self.retains_layout() {
            return None;
        }
        let node = self.nodes.get(self.current?)?;
        node.layouts.get(node.layout_cursor).copied()
    }

    pub fn record_layout(&mut self, layout: LayoutId) {
        if !self.active || !self.retains_layout() {
            return;
        }
        let Some(id) = self.current else { return };
        if let Some(node) = self.nodes.get_mut(id) {
            if self.transactions > 0 {
                self.undo
                    .push(Undo::Layouts(id, node.layouts.clone(), node.layout_cursor));
            }
            if let Some(previous) = node.layouts.get_mut(node.layout_cursor) {
                *previous = layout;
            } else {
                node.layouts.push(layout);
            }
            node.layout_cursor += 1;
        }
    }

    pub fn layout_ids(&self) -> impl Iterator<Item = LayoutId> + '_ {
        self.nodes
            .values()
            .flat_map(|node| node.layouts.iter().copied())
    }

    pub fn preserve_current_children(&mut self) {
        let Some(current_id) = self.current else {
            return;
        };
        let Some(current) = self.nodes.get_mut(current_id) else {
            return;
        };
        if self.transactions > 0 {
            self.undo
                .push(Undo::Children(current_id, current.next_children.clone()));
        }
        current.next_children.clone_from(&current.children);
        let mut pending = current.children.clone();
        while let Some(id) = pending.pop() {
            if let Some(node) = self.nodes.get_mut(id) {
                if self.transactions > 0 {
                    self.undo.push(Undo::Seen(id, node.seen));
                    self.undo
                        .push(Undo::Children(id, node.next_children.clone()));
                    self.undo
                        .push(Undo::Layouts(id, node.layouts.clone(), node.layout_cursor));
                }
                node.seen = true;
                node.layout_cursor = node.layouts.len();
                node.next_children.clone_from(&node.children);
                pending.extend(node.children.iter().copied());
            }
        }
    }

    pub fn checkpoint(&mut self) -> RetainedCheckpoint {
        self.transactions += 1;
        RetainedCheckpoint {
            undo_length: self.undo.len(),
            root_slot: self.root_slot,
            current: self.current,
            stats: self.stats.clone(),
        }
    }

    pub fn end_transaction(&mut self, checkpoint: RetainedCheckpoint, commit: bool) {
        if !commit {
            while self.undo.len() > checkpoint.undo_length {
                match self.undo.pop() {
                    Some(Undo::Insert {
                        id,
                        identity,
                        previous,
                    }) => {
                        self.nodes.remove(id);
                        if let Some(previous) = previous {
                            self.identities.insert(identity, previous);
                        } else {
                            self.identities.remove(&identity);
                        }
                    }
                    Some(Undo::Seen(id, seen)) => {
                        if let Some(node) = self.nodes.get_mut(id) {
                            node.seen = seen;
                        }
                    }
                    Some(Undo::Child(id)) => {
                        if let Some(node) = self.nodes.get_mut(id) {
                            node.next_children.pop();
                        }
                    }
                    Some(Undo::Children(id, children)) => {
                        if let Some(node) = self.nodes.get_mut(id) {
                            node.next_children = children;
                        }
                    }
                    Some(Undo::Layouts(id, layouts, cursor)) => {
                        if let Some(node) = self.nodes.get_mut(id) {
                            node.layouts = layouts;
                            node.layout_cursor = cursor;
                        }
                    }
                    Some(Undo::Hitbox(id, snapshot)) => {
                        if let Some(node) = self.nodes.get_mut(id) {
                            node.hitbox_snapshot = snapshot;
                        }
                    }
                    Some(Undo::Subtree(id, snapshot)) => {
                        if let Some(node) = self.nodes.get_mut(id) {
                            node.subtree_snapshot = snapshot;
                        }
                    }
                    Some(Undo::Damage(id, damage, isolation)) => {
                        if let Some(node) = self.nodes.get_mut(id) {
                            node.damage = damage;
                            node.isolation = isolation;
                        }
                    }
                    Some(Undo::Handlers(id, shapes, cursor)) => {
                        if let Some(node) = self.nodes.get_mut(id) {
                            node.handler_shapes = shapes;
                            node.handler_cursor = cursor;
                        }
                    }
                    Some(Undo::Properties(id, properties)) => {
                        if let Some(node) = self.nodes.get_mut(id) {
                            node.properties = properties;
                        }
                    }
                    None => break,
                }
            }
            self.root_slot = checkpoint.root_slot;
            self.current = checkpoint.current;
            self.stats = checkpoint.stats;
        }
        self.transactions -= 1;
        if self.transactions == 0 {
            self.undo.clear();
        }
    }

    pub fn finish_frame(&mut self) {
        self.active = false;
        self.current = None;
        self.nodes.retain(|_, node| node.seen);
        self.identities.retain(|_, id| self.nodes.contains_key(*id));
        self.view_nodes.retain(|_, nodes| {
            nodes.retain(|id| self.nodes.contains_key(*id));
            !nodes.is_empty()
        });
        let changed_children: Vec<_> = self
            .nodes
            .iter()
            .filter_map(|(id, node)| (node.children != node.next_children).then_some(id))
            .collect();
        for id in changed_children {
            self.damage_node(id, Damage::CHILDREN | Damage::LAYOUT);
        }
        for node in self.nodes.values_mut() {
            node.layouts.truncate(node.layout_cursor);
            std::mem::swap(&mut node.children, &mut node.next_children);
            node.next_children.clear();
        }
        self.stats.nodes_removed =
            self.stats.nodes_total + self.stats.nodes_created - self.nodes.len();
        self.stats.nodes_total = self.nodes.len();
        self.trim_snapshots();
        self.subtree_nodes.retain(|id| {
            self.nodes
                .get(*id)
                .is_some_and(|node| node.subtree_snapshot.is_some())
        });
        #[cfg(any(feature = "inspector", debug_assertions))]
        {
            self.completed_stats = Some((self.generation, self.stats.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::FluentBuilder;
    use crate::{
        AppContext, Context, InputEvent, InteractiveElement, IntoElement, MouseButton,
        MouseDownEvent, ObjectFit, ParentElement, Render, RequestFrameOptions,
        StatefulInteractiveElement, Styled, StyledImage, StyledText, TestAppContext, Text,
        TextLayout, Window, div, point, px, rgb,
    };
    use std::{cell::Cell, rc::Rc};

    struct RetainedTestView {
        reversed: bool,
    }

    struct ConservativeCanvasView;

    impl Render for ConservativeCanvasView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            crate::canvas(|_, _, _| (), |_, (), _, _| {}).size(px(20.))
        }
    }

    #[gpui::test]
    fn unversioned_canvas_keeps_conservative_phase_damage(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            ConservativeCanvasView
        });
        for _ in 0..3 {
            window
                .update(cx, |_, _, cx| cx.notify())
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    let node = window
                        .retained_tree
                        .nodes
                        .values()
                        .find(|node| node.element_type == TypeId::of::<crate::Canvas<()>>())
                        .expect("canvas node");
                    assert!(node.damage.contains(
                        Damage::LAYOUT | Damage::PREPAINT | Damage::HANDLERS | Damage::PAINT
                    ));
                })
                .expect("window exists");
        }
    }

    #[derive(Default)]
    struct CachedSubtreeChild {
        renders: Rc<Cell<usize>>,
        dependency: Option<crate::Entity<u32>>,
        image: Option<std::sync::Arc<crate::RenderImage>>,
        clicks: Rc<Cell<u32>>,
    }

    impl Render for CachedSubtreeChild {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let color = self
                .dependency
                .as_ref()
                .map_or(0xff0000, |dependency| *dependency.read(cx));
            let clicks = self.clicks.clone();
            div()
                .size(px(20.))
                .bg(rgb(color))
                .on_mouse_down(MouseButton::Left, move |_, _, _| clicks.set(color))
                .window_control_area(crate::WindowControlArea::Drag)
                .when_some(self.image.clone(), |element, image| {
                    element.child(crate::img(image).size(px(16.)))
                })
        }
    }

    struct CachedSubtreeRoot {
        child: crate::Entity<CachedSubtreeChild>,
        opacity: f32,
    }

    impl Render for CachedSubtreeRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().opacity(self.opacity).child(
                self.child
                    .clone()
                    .cached(crate::StyleRefinement::default().size(px(20.))),
            )
        }
    }

    #[gpui::test]
    fn retained_cached_subtree_obeys_snapshot_budget(cx: &mut TestAppContext) {
        let renders = Rc::new(Cell::new(0));
        let window = cx.add_window(|window, cx| {
            window.retained_tree = RetainedElementTree::new(true);
            CachedSubtreeRoot {
                child: cx.new(|_| CachedSubtreeChild {
                    renders: renders.clone(),
                    ..Default::default()
                }),
                opacity: 1.,
            }
        });
        assert_eq!(renders.get(), 1);
        for (budget, expected_renders) in [
            (usize::MAX, 1),
            (0, 1),
            (0, 2),
            (usize::MAX, 3),
            (usize::MAX, 3),
        ] {
            window
                .update(cx, |_, window, cx| {
                    window.retained_tree.budget.max_snapshot_bytes = budget;
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            assert_eq!(renders.get(), expected_renders);
            window
                .update(cx, |_, window, _| {
                    assert_eq!(window.rendered_frame.scene.quads.len(), 1);
                    assert!(window.retained_tree.stats.snapshot_bytes <= budget);
                })
                .expect("window exists");
        }
    }

    #[gpui::test]
    fn retained_cached_subtree_invalidates_inherited_opacity(cx: &mut TestAppContext) {
        let renders = Rc::new(Cell::new(0));
        let window = cx.add_window(|window, cx| {
            window.retained_tree = RetainedElementTree::new(true);
            CachedSubtreeRoot {
                child: cx.new(|_| CachedSubtreeChild {
                    renders: renders.clone(),
                    ..Default::default()
                }),
                opacity: 1.,
            }
        });
        for opacity in [1., 0.5, 0.5, 1.] {
            window
                .update(cx, |view, _, cx| {
                    view.opacity = opacity;
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            let output = window
                .update(cx, |_, window, _| {
                    format!("{:?}", window.rendered_frame.scene.quads)
                })
                .expect("window exists");
            window
                .update(cx, |_, window, _| window.refresh())
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    assert_eq!(output, format!("{:?}", window.rendered_frame.scene.quads));
                })
                .expect("window exists");
        }
    }

    #[gpui::test]
    fn retained_cached_subtree_replays_window_controls(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, cx| {
            window.retained_tree = RetainedElementTree::new(true);
            CachedSubtreeRoot {
                child: cx.new(|_| CachedSubtreeChild::default()),
                opacity: 1.,
            }
        });
        let mut previous_slots = None;
        for _ in 0..4 {
            window
                .update(cx, |_, _, cx| cx.notify())
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    let slots: Vec<_> = window
                        .rendered_frame
                        .mouse_listeners
                        .iter()
                        .flatten()
                        .filter_map(|handler| handler.slot)
                        .collect();
                    assert!(!slots.is_empty());
                    if let Some(previous) = &previous_slots {
                        assert_eq!(&slots, previous);
                    }
                    previous_slots = Some(slots);
                    assert!(window.retained_tree.stats.handlers_replayed > 0);
                    assert_eq!(window.rendered_frame.window_control_hitboxes.len(), 1);
                    assert_eq!(
                        window
                            .rendered_frame
                            .window_control_hitboxes
                            .first()
                            .map(|(area, _)| *area),
                        Some(crate::WindowControlArea::Drag)
                    );
                })
                .expect("window exists");
        }
    }

    #[gpui::test]
    fn retained_cached_subtree_tracks_entity_dependencies(cx: &mut TestAppContext) {
        let dependency = cx.new(|_| 0xff0000);
        let renders = Rc::new(Cell::new(0));
        let clicks = Rc::new(Cell::new(0));
        let window = cx.add_window(|window, cx| {
            window.retained_tree = RetainedElementTree::new(true);
            CachedSubtreeRoot {
                child: cx.new(|_| CachedSubtreeChild {
                    renders: renders.clone(),
                    dependency: Some(dependency.clone()),
                    clicks: clicks.clone(),
                    ..Default::default()
                }),
                opacity: 1.,
            }
        });
        for expected in 2..5 {
            dependency.update(cx, |color, cx| {
                *color ^= 0xffff00;
                cx.notify();
            });
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            assert_eq!(renders.get(), expected);
            window
                .update(cx, |_, window, cx| {
                    window.simulate_mouse_move(point(px(5.), px(5.)), cx);
                    window.dispatch_event(
                        crate::PlatformInput::MouseDown(MouseDownEvent {
                            button: MouseButton::Left,
                            position: point(px(5.), px(5.)),
                            modifiers: Default::default(),
                            click_count: 1,
                            first_mouse: false,
                        }),
                        cx,
                    );
                    assert_eq!(clicks.get(), *dependency.read(cx));
                })
                .expect("window exists");
        }
    }

    #[gpui::test]
    fn retained_cached_subtree_rebases_frame_ranges(cx: &mut TestAppContext) {
        struct PrefixRoot {
            child: crate::Entity<CachedSubtreeChild>,
            prefix: bool,
        }
        impl Render for PrefixRoot {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div()
                    .when(self.prefix, |element| {
                        element.child(
                            div()
                                .absolute()
                                .size(px(10.))
                                .bg(rgb(0xffffff))
                                .window_control_area(crate::WindowControlArea::Close),
                        )
                    })
                    .child(
                        self.child
                            .clone()
                            .cached(crate::StyleRefinement::default().size(px(20.))),
                    )
            }
        }
        let renders = Rc::new(Cell::new(0));
        let window = cx.add_window(|window, cx| {
            window.retained_tree = RetainedElementTree::new(true);
            PrefixRoot {
                child: cx.new(|_| CachedSubtreeChild {
                    renders: renders.clone(),
                    ..Default::default()
                }),
                prefix: false,
            }
        });
        for prefix in [true, false, true, true, false] {
            window
                .update(cx, |view, _, cx| {
                    view.prefix = prefix;
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            assert_eq!(renders.get(), 1);
            window
                .update(cx, |_, window, _| {
                    assert_eq!(
                        window.rendered_frame.scene.quads.len(),
                        1 + usize::from(prefix)
                    );
                    let controls = &window.rendered_frame.window_control_hitboxes;
                    assert_eq!(controls.len(), 1 + usize::from(prefix));
                    assert_eq!(
                        controls.last().map(|(area, _)| *area),
                        Some(crate::WindowControlArea::Drag)
                    );
                })
                .expect("window exists");
        }
    }

    #[gpui::test]
    fn retained_dependency_capture_includes_prior_reads(cx: &mut TestAppContext) {
        let shared = cx.new(|_| 1_u32);
        let unrelated = cx.new(|_| 2_u32);
        cx.update(|cx| {
            shared.read(cx);
            unrelated.read(cx);
            let (inner, outer) = cx.detect_accessed_entities(|cx| {
                let (_, inner) = cx.detect_accessed_entities(|cx| {
                    shared.read(cx);
                });
                inner
            });
            assert_eq!(inner, FxHashSet::from_iter([shared.entity_id()]));
            assert_eq!(outer, inner);
            assert!(
                cx.entities
                    .accessed_entities
                    .get_mut()
                    .contains(&unrelated.entity_id())
            );
        });
    }

    #[gpui::test]
    fn retained_cached_subtree_revalidates_resources(cx: &mut TestAppContext) {
        let image = std::sync::Arc::new(crate::RenderImage::new(vec![image::Frame::new(
            image::ImageBuffer::from_pixel(16, 16, image::Rgba([255, 0, 0, 255])),
        )]));
        let renders = Rc::new(Cell::new(0));
        let window = cx.add_window(|window, cx| {
            window.retained_tree = RetainedElementTree::new(true);
            CachedSubtreeRoot {
                child: cx.new(|_| CachedSubtreeChild {
                    renders: renders.clone(),
                    image: Some(image.clone()),
                    ..Default::default()
                }),
                opacity: 1.,
            }
        });
        for (evict, expected) in [(false, 1), (true, 2), (false, 2)] {
            window
                .update(cx, |_, window, cx| {
                    if evict {
                        window.drop_image(image.clone()).expect("drop image");
                    }
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            assert_eq!(renders.get(), expected);
            window
                .update(cx, |_, window, _| {
                    assert_eq!(window.rendered_frame.scene.polychrome_sprites.len(), 1);
                })
                .expect("window exists");
        }
    }

    #[cfg(any(feature = "inspector", debug_assertions))]
    #[gpui::test]
    fn inspector_reports_retained_identity_and_completed_frame(cx: &mut TestAppContext) {
        let rendered_diagnostics = Rc::new(Cell::new(false));
        cx.update(|cx| {
            let rendered_diagnostics = rendered_diagnostics.clone();
            cx.set_inspector_renderer(Box::new(move |inspector, window, cx| {
                let states = inspector.render_inspector_states(window, cx);
                rendered_diagnostics.set(!states.is_empty());
                div().children(states).into_any_element()
            }));
        });
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            RetainedTestView { reversed: false }
        });
        window
            .update(cx, |_, window, cx| window.toggle_inspector(cx))
            .expect("window exists");
        cx.test_window(window.into())
            .simulate_frame_request(RequestFrameOptions::default());
        let (selected_id, stable_id) = window
            .update(cx, |_, window, cx| {
                let id = window
                    .rendered_frame
                    .inspector_hitboxes
                    .values()
                    .find(|id| id.path.global_id.0.last() == Some(&ElementId::Integer(1)))
                    .expect("first child is inspectable")
                    .clone();
                let lines = window.retained_tree.inspector_lines(&id);
                assert!(lines.iter().any(|line| line.starts_with("Retained node:")));
                assert!(
                    lines
                        .iter()
                        .any(|line| line.contains("Inspector forces full rebuild"))
                );
                assert!(
                    lines
                        .iter()
                        .any(|line| line.starts_with("Previous completed frame:"))
                );
                assert!(
                    lines
                        .iter()
                        .any(|line| line.starts_with("Owner view: Some("))
                );
                let stable_id = lines
                    .iter()
                    .find(|line| line.starts_with("Retained node:"))
                    .expect("node id")
                    .clone();
                let position = window
                    .rendered_frame
                    .hitboxes
                    .iter()
                    .find(|hitbox| {
                        window.rendered_frame.inspector_hitboxes.get(&hitbox.id) == Some(&id)
                    })
                    .expect("pickable hitbox")
                    .bounds
                    .center();
                window.simulate_mouse_move(position, cx);
                window.dispatch_event(
                    MouseDownEvent {
                        position,
                        button: MouseButton::Left,
                        modifiers: Default::default(),
                        click_count: 1,
                        first_mouse: false,
                    }
                    .to_platform_input(),
                    cx,
                );
                (id, stable_id)
            })
            .expect("window exists");
        window
            .update(cx, |view, _, cx| {
                view.reversed = true;
                cx.notify();
            })
            .expect("window exists");
        cx.test_window(window.into())
            .simulate_frame_request(RequestFrameOptions::default());
        assert!(rendered_diagnostics.get());
        window
            .update(cx, |_, window, _| {
                assert!(
                    window
                        .retained_tree
                        .inspector_lines(&selected_id)
                        .contains(&stable_id)
                );
                window.retained_tree.enabled = false;
                assert!(
                    window
                        .retained_tree
                        .inspector_lines(&selected_id)
                        .iter()
                        .any(|line| line.contains("Retained tree disabled"))
                );
            })
            .expect("window exists");
    }

    struct BudgetView {
        children: usize,
    }

    impl Render for BudgetView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().flex().children((0..self.children).map(|index| {
                div()
                    .id(index)
                    .size(px(10.))
                    .bg(rgb(0x123456))
                    .on_mouse_down(MouseButton::Left, |_, _, _| {})
            }))
        }
    }

    #[gpui::test]
    fn snapshot_budget_evicts_without_losing_layout_or_pixels(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            BudgetView { children: 16 }
        });
        let original = window
            .update(cx, |_, window, _| {
                window
                    .retained_tree
                    .nodes
                    .values()
                    .flat_map(|node| node.layouts.iter().copied())
                    .collect::<Vec<_>>()
            })
            .expect("window exists");
        let mut expected = None;
        for (budget, cache_expected, replay_expected) in [
            (0, false, true),
            (0, false, false),
            (1024 * 1024, true, false),
            (1024 * 1024, true, true),
        ] {
            window
                .update(cx, |_, window, cx| {
                    window.retained_tree.budget.max_snapshot_bytes = budget;
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    let tree = &window.retained_tree;
                    assert_eq!(
                        tree.nodes
                            .values()
                            .any(|node| !node.paint_snapshots.is_empty()),
                        cache_expected
                    );
                    assert_eq!(
                        tree.nodes
                            .values()
                            .any(|node| node.hitbox_snapshot.is_some()),
                        cache_expected
                    );
                    assert_eq!(tree.stats.snapshot_bytes > 0, cache_expected);
                    assert!(tree.stats.snapshot_bytes <= budget);
                    assert_eq!(tree.stats.paint_replayed > 0, replay_expected);
                    assert!(tree.stats.layout_reused >= 16);
                    let layouts = tree
                        .nodes
                        .values()
                        .flat_map(|node| node.layouts.iter().copied())
                        .collect::<Vec<_>>();
                    assert_eq!(layouts, original);
                    let pixels = format!("{:?}", window.rendered_frame.scene.quads);
                    if let Some(expected) = &expected {
                        assert_eq!(&pixels, expected);
                    } else {
                        expected = Some(pixels);
                    }
                })
                .expect("window exists");
        }
        window
            .update(cx, |view, _, cx| {
                view.children = 0;
                cx.notify();
            })
            .expect("window exists");
        cx.test_window(window.into())
            .simulate_frame_request(RequestFrameOptions::default());
        window
            .update(cx, |_, window, _| {
                let tree = &window.retained_tree;
                let properties = tree
                    .nodes
                    .values()
                    .filter_map(|node| node.properties.as_ref())
                    .collect::<Vec<_>>();
                assert_eq!(properties.len(), 1, "only the root Div retains properties");
                assert_eq!(tree.stats.snapshot_bytes, 0);
                assert_eq!(
                    tree.stats.property_bytes,
                    properties
                        .iter()
                        .map(|properties| properties.owned_bytes())
                        .sum::<usize>()
                );
                assert!(
                    tree.nodes
                        .values()
                        .all(|node| node.paint_snapshots.is_empty()
                            && node.hitbox_snapshot.is_none())
                );
                assert_eq!(window.retained_tree.stats.nodes_removed, 16);
            })
            .expect("window exists");
    }

    impl Render for RetainedTestView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let keys = if self.reversed { [2, 1] } else { [1, 2] };
            div().children(keys.map(|key| {
                div().id(key).size(px(key as f32 * 20.)).bg(if key == 1 {
                    rgb(0x123456)
                } else {
                    rgb(0xabcdef)
                })
            }))
        }
    }

    #[gpui::test]
    fn window_reconciliation_preserves_identity_and_frame_output(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            RetainedTestView { reversed: false }
        });
        let mut identities = None;
        let mut layouts = None;
        let mut outputs = Vec::new();
        for reversed in [false, true, false, true] {
            window
                .update(cx, |view, window, _| {
                    view.reversed = reversed;
                    window.refresh();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            let retained_output = window
                .update(cx, |_, window, _| {
                    let tree = &window.retained_tree;
                    if let Some(previous) = &identities {
                        assert_eq!(&tree.identities, previous);
                        assert_eq!(tree.stats.nodes_created, 0);
                        assert_eq!(tree.stats.nodes_removed, 0);
                    }
                    identities = Some(tree.identities.clone());
                    let current_layouts: FxHashMap<_, _> = tree
                        .nodes
                        .iter()
                        .map(|(id, node)| (id, node.layouts.clone()))
                        .collect();
                    if let Some(previous) = &layouts {
                        assert_eq!(&current_layouts, previous);
                    }
                    layouts = Some(current_layouts);
                    assert!(tree.stats.nodes_total >= 4);
                    assert!(tree.stats.prepaint_rebuilt >= 4);
                    assert!(tree.stats.paint_rebuilt >= 4);
                    format!("{:?}", window.rendered_frame.scene.quads)
                })
                .expect("window exists");
            outputs.push(retained_output);
        }
        for (reversed, retained_output) in [false, true, false, true].into_iter().zip(outputs) {
            window
                .update(cx, |view, window, _| {
                    view.reversed = reversed;
                    window.retained_tree.enabled = false;
                    window.refresh();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    assert_eq!(
                        format!("{:?}", window.rendered_frame.scene.quads),
                        retained_output
                    );
                    assert!(window.retained_tree.nodes.is_empty());
                })
                .expect("window exists");
        }
    }

    fn element(tree: &mut RetainedElementTree, key: u64) -> RetainedNodeId {
        tree.begin_element(Some(ElementId::Integer(key)), TypeId::of::<()>())
            .expect("retained frame is active")
    }

    struct CanvasView {
        revision: u64,
        width: f32,
        prefix: bool,
        interactive: bool,
        keyboard: bool,
        paints: Rc<Cell<usize>>,
    }

    struct LeafLayoutView {
        changed_color: bool,
        changed_size: bool,
    }

    struct RetainedTextView {
        text: &'static str,
        width: f32,
    }

    struct RetainedImageView {
        source: crate::ImageSource,
    }

    impl Render for RetainedImageView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().child(crate::img(self.source.clone()).id("retained-image"))
        }
    }

    #[gpui::test]
    fn retained_image_layout_tracks_intrinsic_size(cx: &mut TestAppContext) {
        let images: Vec<_> = [(32, 16, 10), (32, 16, 200), (64, 16, 200)]
            .into_iter()
            .map(|(width, height, red)| {
                std::sync::Arc::new(crate::RenderImage::new(smallvec::smallvec![
                    image::Frame::new(image::ImageBuffer::from_pixel(
                        width,
                        height,
                        image::Rgba([red, 0, 0, 255])
                    ))
                ]))
            })
            .collect();
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            RetainedImageView {
                source: crate::ImageSource::Render(images.first().expect("fixture").clone()),
            }
        });
        let mut previous_layout = None;
        let mut previous_tile = None;
        for (index, image) in images.iter().cloned().enumerate() {
            window
                .update(cx, |view, _, cx| {
                    view.source = crate::ImageSource::Render(image);
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    assert_eq!(
                        window.retained_tree.stats.layout_reused,
                        if index < 2 { 1 } else { 0 }
                    );
                    assert_eq!(
                        window.retained_tree.stats.paint_replayed,
                        usize::from(index == 0)
                    );
                    let node = window
                        .retained_tree
                        .nodes
                        .values()
                        .find(|node| node.key == ReconcileKey::Explicit("retained-image".into()))
                        .expect("image node");
                    assert!(
                        node.properties.is_some(),
                        "image records resolved properties"
                    );
                    assert_eq!(node.damage.contains(Damage::LAYOUT), index == 2);
                    let layout = *node.layouts.first().expect("image layout");
                    if let Some(previous) = previous_layout {
                        assert_eq!(layout, previous);
                    }
                    previous_layout = Some(layout);
                    let sprite = window
                        .rendered_frame
                        .scene
                        .polychrome_sprites
                        .first()
                        .expect("image sprite");
                    if let Some(previous) = previous_tile {
                        assert_ne!(sprite.tile.tile_id, previous);
                    }
                    previous_tile = Some(sprite.tile.tile_id);
                })
                .expect("window exists");
        }
    }

    #[gpui::test]
    fn retained_image_custom_loader_still_runs_each_frame(cx: &mut TestAppContext) {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let calls = Arc::new(AtomicUsize::new(0));
        let data = std::sync::Arc::new(crate::RenderImage::new(smallvec::smallvec![
            image::Frame::new(image::ImageBuffer::from_pixel(
                16,
                16,
                image::Rgba([255, 0, 0, 255])
            ))
        ]));
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            RetainedImageView {
                source: crate::ImageSource::Custom(std::sync::Arc::new({
                    let calls = calls.clone();
                    move |_, _| {
                        calls.fetch_add(1, Ordering::Relaxed);
                        Some(Ok(data.clone()))
                    }
                })),
            }
        });
        for _ in 0..3 {
            let calls_before = calls.load(Ordering::Relaxed);
            window
                .update(cx, |_, _, cx| cx.notify())
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    assert_eq!(window.retained_tree.stats.layout_reused, 0);
                    assert_eq!(calls.load(Ordering::Relaxed), calls_before + 2);
                })
                .expect("window exists");
        }
    }

    struct SizedImageView {
        source: crate::ImageSource,
        variant: usize,
    }

    impl Render for SizedImageView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let image = crate::img(self.source.clone())
                .id("sized-image")
                .flex_shrink_0()
                .rounded_lg();
            let image = match self.variant {
                0 => image.h(px(40.)),
                1 => image.w(px(40.)),
                2 => image
                    .size_full()
                    .aspect_square()
                    .object_fit(ObjectFit::Contain),
                3 => image
                    .size_full()
                    .aspect_square()
                    .object_fit(ObjectFit::Cover),
                _ => image
                    .size_full()
                    .aspect_square()
                    .object_fit(ObjectFit::Fill),
            };
            div().size(px(80.)).overflow_hidden().child(image)
        }
    }

    #[gpui::test]
    fn retained_image_sizing_and_cropping_match_full_frames(cx: &mut TestAppContext) {
        let image = std::sync::Arc::new(crate::RenderImage::new(smallvec::smallvec![
            image::Frame::new(image::ImageBuffer::from_pixel(
                32,
                64,
                image::Rgba([255, 0, 0, 255])
            ))
        ]));
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            SizedImageView {
                source: crate::ImageSource::Render(image),
                variant: 0,
            }
        });
        let mut outputs = Vec::new();
        for retained in [true, false] {
            for variant in 0..5 {
                for repeat in 0..2 {
                    window
                        .update(cx, |view, window, cx| {
                            view.variant = variant;
                            window.retained_tree.enabled = retained;
                            cx.notify();
                        })
                        .expect("window exists");
                    cx.test_window(window.into())
                        .simulate_frame_request(RequestFrameOptions::default());
                    let output = window
                        .update(cx, |_, window, _| {
                            if retained && repeat == 1 {
                                assert_eq!(window.retained_tree.stats.layout_reused, 1);
                                assert_eq!(window.retained_tree.stats.paint_replayed, 1);
                            }
                            assert!(!window.rendered_frame.scene.polychrome_sprites.is_empty());
                            format!("{:?}", window.rendered_frame.scene.polychrome_sprites)
                        })
                        .expect("window exists");
                    if retained {
                        outputs.push(output);
                    } else {
                        assert_eq!(outputs.get(variant * 2 + repeat), Some(&output));
                    }
                }
            }
        }
    }

    #[gpui::test]
    fn retained_animated_images_use_full_layout(cx: &mut TestAppContext) {
        let frame = image::Frame::new(image::ImageBuffer::from_pixel(
            16,
            16,
            image::Rgba([255, 0, 0, 255]),
        ));
        let still =
            std::sync::Arc::new(crate::RenderImage::new(smallvec::smallvec![frame.clone()]));
        let animated = std::sync::Arc::new(crate::RenderImage::new(smallvec::smallvec![
            frame.clone(),
            frame
        ]));
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            RetainedImageView {
                source: crate::ImageSource::Render(still.clone()),
            }
        });
        for (source, reused) in [(still.clone(), 1), (animated, 0), (still, 1)] {
            window
                .update(cx, |view, _, cx| {
                    view.source = crate::ImageSource::Render(source);
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    assert_eq!(window.retained_tree.stats.layout_reused, reused);
                    assert_eq!(window.rendered_frame.scene.polychrome_sprites.len(), 1);
                })
                .expect("window exists");
        }
    }

    struct RetainedSvgView {
        width: f32,
        alternate: bool,
    }

    struct SpritePropertiesView {
        image: std::sync::Arc<crate::RenderImage>,
        variant: usize,
    }

    impl Render for SpritePropertiesView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let variant = self.variant;
            div()
                .flex()
                .w(px(if variant == 1 { 24. } else { 40. }))
                .h(px(20.))
                .overflow_hidden()
                .opacity(if variant == 2 { 0.5 } else { 1. })
                .child(
                    crate::img(self.image.clone())
                        .id("image")
                        .size(px(20.))
                        .flex_shrink_0()
                        .grayscale(variant == 3)
                        .rounded(px(if variant == 4 { 5. } else { 0. })),
                )
                .child(
                    crate::svg()
                        .data(br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16"/></svg>"#)
                        .size(px(20.))
                        .flex_shrink_0()
                        .text_color(rgb(if variant == 5 { 0xff0000 } else { 0x00ff00 }))
                        .with_transformation(if variant == 6 {
                            crate::Transformation::rotate(crate::radians(0.5))
                        } else {
                            crate::Transformation::default()
                        })
                        .id("svg"),
                )
        }
    }

    #[gpui::test]
    fn retained_sprite_properties_match_full_paint(cx: &mut TestAppContext) {
        let image = std::sync::Arc::new(crate::RenderImage::new(vec![image::Frame::new(
            image::ImageBuffer::from_pixel(16, 16, image::Rgba([255, 0, 0, 255])),
        )]));
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            SpritePropertiesView { image, variant: 0 }
        });
        let mut retained_outputs = Vec::new();
        for paint_enabled in [true, false] {
            for variant in 0..7 {
                for repeat in 0..2 {
                    window
                        .update(cx, |view, window, cx| {
                            view.variant = variant;
                            window.retained_tree.paint_enabled = paint_enabled;
                            cx.notify();
                        })
                        .expect("window exists");
                    cx.test_window(window.into())
                        .simulate_frame_request(RequestFrameOptions::default());
                    let output = window
                        .update(cx, |_, window, _| {
                            if paint_enabled && repeat == 1 {
                                assert_eq!(window.retained_tree.stats.paint_replayed, 2);
                            }
                            let scene = &window.rendered_frame.scene;
                            assert_eq!(scene.polychrome_sprites.len(), 1);
                            assert_eq!(scene.monochrome_sprites.len(), 1);
                            format!(
                                "{:?}{:?}",
                                scene.polychrome_sprites, scene.monochrome_sprites
                            )
                        })
                        .expect("window exists");
                    if paint_enabled {
                        retained_outputs.push(output);
                    } else {
                        assert_eq!(
                            retained_outputs.get(variant * 2 + repeat),
                            Some(&output),
                            "variant {variant}"
                        );
                    }
                }
            }
        }
        for (index, evict) in [false, true, false].into_iter().enumerate() {
            window
                .update(cx, |view, window, cx| {
                    window.retained_tree.paint_enabled = true;
                    if evict {
                        window.drop_image(view.image.clone()).expect("drop image");
                    }
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    if evict {
                        assert_eq!(window.retained_tree.stats.paint_replayed, 0);
                    } else if index == 2 {
                        assert_eq!(window.retained_tree.stats.paint_replayed, 2);
                    }
                    assert_eq!(window.rendered_frame.scene.polychrome_sprites.len(), 1);
                    assert_eq!(window.rendered_frame.scene.monochrome_sprites.len(), 1);
                })
                .expect("window exists");
        }
    }

    impl Render for RetainedSvgView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let data = if self.alternate {
                br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><circle cx="8" cy="8" r="6"/></svg>"#.as_slice()
            } else {
                br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16"/></svg>"#.as_slice()
            };
            div().child(
                crate::svg()
                    .data(data)
                    .id("retained-svg")
                    .size(px(self.width))
                    .text_color(rgb(if self.alternate { 0xff0000 } else { 0x00ff00 })),
            )
        }
    }

    #[gpui::test]
    fn retained_svg_reuses_layout_without_reusing_stale_pixels(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            RetainedSvgView {
                width: 16.,
                alternate: false,
            }
        });
        let mut retained_outputs = Vec::new();
        for retained in [true, false] {
            for (index, (width, alternate, reused)) in [
                (16., false, 1),
                (16., false, 1),
                (16., true, 1),
                (24., true, 0),
                (24., true, 1),
            ]
            .into_iter()
            .enumerate()
            {
                window
                    .update(cx, |view, window, cx| {
                        view.width = width;
                        view.alternate = alternate;
                        window.retained_tree.enabled = retained;
                        cx.notify();
                    })
                    .expect("window exists");
                cx.test_window(window.into())
                    .simulate_frame_request(RequestFrameOptions::default());
                let output = window
                    .update(cx, |_, window, _| {
                        assert_eq!(
                            window.retained_tree.stats.layout_reused,
                            if retained { reused } else { 0 }
                        );
                        assert_eq!(
                            window.retained_tree.stats.paint_replayed,
                            usize::from(retained && matches!(index, 0 | 1 | 4))
                        );
                        assert!(!window.rendered_frame.scene.monochrome_sprites.is_empty());
                        if retained {
                            let node = window
                                .retained_tree
                                .nodes
                                .values()
                                .find(|node| {
                                    node.key == ReconcileKey::Explicit("retained-svg".into())
                                })
                                .expect("SVG node");
                            assert!(node.properties.is_some(), "SVG records typed properties");
                            assert_eq!(node.damage.contains(Damage::LAYOUT), index == 3);
                        }
                        format!("{:?}", window.rendered_frame.scene.monochrome_sprites)
                    })
                    .expect("window exists");
                if retained {
                    retained_outputs.push(output);
                } else {
                    assert_eq!(retained_outputs.get(index), Some(&output));
                }
            }
        }
    }

    impl Render for RetainedTextView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .w(px(self.width))
                .child(Text::new("retained-text".into(), self.text.into()))
        }
    }

    #[gpui::test]
    fn retained_text_preserves_measurement_identity(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            RetainedTextView {
                text: "one two three four five six",
                width: 120.,
            }
        });
        let mut previous_layout = None;
        for (index, (text, width)) in [
            ("one two three four five six", 120.),
            ("one two three four five six", 60.),
            ("one two three four five six", 120.),
            ("changed text with more words", 120.),
        ]
        .into_iter()
        .enumerate()
        {
            window
                .update(cx, |view, _, cx| {
                    view.text = text;
                    view.width = width;
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    if index == 0 {
                        assert_eq!(window.retained_tree.stats.measure_recomputed, 0);
                        assert_eq!(window.retained_tree.stats.paint_replayed, 1);
                    } else {
                        assert!(window.retained_tree.stats.measure_recomputed > 0);
                    }
                    let node = window
                        .retained_tree
                        .nodes
                        .values()
                        .find(|node| node.key == ReconcileKey::Explicit("retained-text".into()))
                        .expect("text retained node");
                    assert!(node.properties.is_some(), "text retains typed properties");
                    assert_eq!(
                        node.layouts.len(),
                        1,
                        "text owns one persistent measured layout"
                    );
                    let layout = *node.layouts.first().expect("measured layout");
                    if let Some(previous) = previous_layout {
                        assert_eq!(layout, previous);
                    }
                    previous_layout = Some(layout);
                })
                .expect("window exists");
        }
    }

    struct RetainedStyledTextView {
        width: f32,
        font_size: f32,
        clamp: bool,
        layout: Option<TextLayout>,
    }

    struct DecoratedTextView {
        variant: usize,
        layout: Option<TextLayout>,
    }

    struct RetainedFontOverrideView {
        alternate: bool,
        layout: Option<TextLayout>,
    }

    struct RetainedExplicitRunsView {
        explicit: bool,
    }

    impl Render for RetainedExplicitRunsView {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let content = "equivalent text runs";
            let text = StyledText::new(content);
            let text = if self.explicit {
                text.with_runs(vec![window.text_style().to_run(content.len())])
            } else {
                text
            };
            div().w(px(90.)).child(text)
        }
    }

    #[gpui::test]
    fn retained_styled_text_normalizes_default_runs(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            RetainedExplicitRunsView { explicit: false }
        });
        for explicit in [true, false, true] {
            window
                .update(cx, |view, _, cx| {
                    view.explicit = explicit;
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    let node = window
                        .retained_tree
                        .nodes
                        .values()
                        .find(|node| node.element_type == TypeId::of::<StyledText>())
                        .expect("styled text node");
                    assert!(
                        node.damage.is_empty(),
                        "equivalent resolved runs: {:?}",
                        node.damage
                    );
                    assert_eq!(window.retained_tree.stats.measure_recomputed, 0);
                    assert_eq!(window.retained_tree.stats.paint_replayed, 1);
                })
                .expect("window exists");
        }
    }

    impl Render for RetainedFontOverrideView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let text = StyledText::new("first second third")
                .with_highlights([(
                    0..5,
                    crate::HighlightStyle {
                        color: Some(rgb(0xff0000).into()),
                        ..Default::default()
                    },
                )])
                .with_font_family_overrides([(
                    0..5,
                    if self.alternate {
                        "Helvetica"
                    } else {
                        ".SystemUIFont"
                    }
                    .into(),
                )]);
            self.layout = Some(text.layout().clone());
            div().w(px(90.)).child(text)
        }
    }

    #[gpui::test]
    fn retained_styled_text_resolves_font_overrides_before_diff(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            RetainedFontOverrideView {
                alternate: false,
                layout: None,
            }
        });
        for (index, alternate) in [true, true, false, false].into_iter().enumerate() {
            window
                .update(cx, |view, _, cx| {
                    view.alternate = alternate;
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            let retained = window
                .update(cx, |view, window, _| {
                    let node = window
                        .retained_tree
                        .nodes
                        .values()
                        .find(|node| node.element_type == TypeId::of::<StyledText>())
                        .expect("styled text node");
                    assert!(node.properties.is_some());
                    if index.is_multiple_of(2) {
                        assert!(node.damage.contains(Damage::LAYOUT));
                        assert!(window.retained_tree.stats.measure_recomputed > 0);
                    } else {
                        assert!(node.damage.is_empty());
                        assert_eq!(window.retained_tree.stats.measure_recomputed, 0);
                    }
                    let layout = view.layout.as_ref().expect("layout");
                    let retained = (layout.wrapped_text(), layout.bounds());
                    window.retained_tree.enabled = false;
                    window.refresh();
                    retained
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |view, window, _| {
                    let layout = view.layout.as_ref().expect("layout");
                    assert_eq!(retained, (layout.wrapped_text(), layout.bounds()));
                    window.retained_tree.enabled = true;
                    window.refresh();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
        }
    }

    struct PaintOnlyTextView {
        alternate: bool,
        truncation: usize,
        text: &'static str,
        segmented: bool,
        layout: Option<TextLayout>,
    }

    impl Render for PaintOnlyTextView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let text = self.text;
            let ranges = if self.segmented {
                vec![0..6, 6..19, 19..text.len()]
            } else {
                vec![0..text.len()]
            };
            let element = StyledText::new(text).with_highlights(
                ranges.into_iter().enumerate().map(|(index, range)| {
                    (
                        range,
                        crate::HighlightStyle {
                            color: Some(
                                rgb(if self.alternate {
                                    0xff0000 + index as u32
                                } else {
                                    0x00ff00 + index as u32
                                })
                                .into(),
                            ),
                            background_color: Some(
                                rgb(if self.alternate { 0x442266 } else { 0x226644 }).into(),
                            ),
                            underline: Some(crate::UnderlineStyle {
                                thickness: px(1.),
                                color: None,
                                wavy: self.alternate,
                            }),
                            ..Default::default()
                        },
                    )
                }),
            );
            self.layout = Some(element.layout().clone());
            let parent = div().w(px(70.));
            let parent = match self.truncation {
                1 => parent.truncate(),
                2 => parent.whitespace_nowrap().text_ellipsis_start(),
                3 => parent.whitespace_nowrap().text_ellipsis_middle(),
                4 => parent.line_clamp(2),
                _ => parent,
            };
            parent.child(element)
        }
    }

    #[gpui::test]
    fn retained_text_decoration_changes_do_not_measure(cx: &mut TestAppContext) {
        for (text, segmented) in [
            ("first second third fourth fifth sixth", false),
            ("first second third fourth fifth sixth", true),
            ("first\nsecond third\nfourth fifth sixth", true),
        ] {
            for truncation in 0..5 {
                let window = cx.add_window(|window, _| {
                    window.retained_tree = RetainedElementTree::new(true);
                    PaintOnlyTextView {
                        alternate: false,
                        truncation,
                        text,
                        segmented,
                        layout: None,
                    }
                });
                for alternate in [true, false, true] {
                    window
                        .update(cx, |view, _, cx| {
                            view.alternate = alternate;
                            cx.notify();
                        })
                        .expect("window exists");
                    cx.test_window(window.into())
                        .simulate_frame_request(RequestFrameOptions::default());
                    let output = window
                        .update(cx, |view, window, _| {
                            assert_eq!(
                                window.retained_tree.stats.measure_recomputed, 0,
                                "truncation {truncation}"
                            );
                            let node = window
                                .retained_tree
                                .nodes
                                .values()
                                .find(|node| node.element_type == TypeId::of::<StyledText>())
                                .expect("styled text node");
                            assert!(
                                node.properties.is_some(),
                                "styled text retains resolved properties"
                            );
                            assert_eq!(node.damage, Damage::PAINT);
                            let layout = view.layout.as_ref().expect("layout");
                            (
                                format!(
                                    "{:?}{:?}",
                                    window.rendered_frame.scene.quads,
                                    window.rendered_frame.scene.underlines
                                ),
                                layout.wrapped_text(),
                                layout.bounds(),
                            )
                        })
                        .expect("window exists");
                    window
                        .update(cx, |_, window, _| {
                            window.retained_tree.enabled = false;
                            window.refresh();
                        })
                        .expect("window exists");
                    cx.test_window(window.into())
                        .simulate_frame_request(RequestFrameOptions::default());
                    window
                        .update(cx, |view, window, _| {
                            let layout = view.layout.as_ref().expect("layout");
                            assert_eq!(
                                output,
                                (
                                    format!(
                                        "{:?}{:?}",
                                        window.rendered_frame.scene.quads,
                                        window.rendered_frame.scene.underlines
                                    ),
                                    layout.wrapped_text(),
                                    layout.bounds()
                                )
                            );
                        })
                        .expect("window exists");
                    window
                        .update(cx, |_, window, _| {
                            window.retained_tree.enabled = true;
                            window.refresh();
                        })
                        .expect("window exists");
                    cx.test_window(window.into())
                        .simulate_frame_request(RequestFrameOptions::default());
                }
            }
        }
    }

    impl Render for DecoratedTextView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let text = if self.variant == 1 {
                "changed text with different words"
            } else {
                "one two three four five six"
            };
            let text = StyledText::new(text).with_highlights([(
                0..text.len(),
                crate::HighlightStyle {
                    background_color: Some(
                        rgb(if self.variant == 2 {
                            0x226644
                        } else {
                            0x442266
                        })
                        .into(),
                    ),
                    underline: Some(crate::UnderlineStyle {
                        thickness: px(if self.variant == 3 { 2. } else { 1. }),
                        color: Some(rgb(0xff0000).into()),
                        wavy: self.variant == 3,
                    }),
                    ..Default::default()
                },
            )]);
            self.layout = Some(text.layout().clone());
            div()
                .w(px(if self.variant == 4 { 60. } else { 120. }))
                .text_size(px(if self.variant == 6 { 20. } else { 14. }))
                .when(self.variant == 5, |element| element.text_right())
                .when(self.variant == 7, |element| element.line_clamp(1))
                .child(text)
        }
    }

    #[gpui::test]
    fn retained_text_paint_matches_full_decorations(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            DecoratedTextView {
                variant: 0,
                layout: None,
            }
        });
        let mut outputs = Vec::new();
        for retained in [true, false] {
            for variant in 0..8 {
                for repeat in 0..2 {
                    window
                        .update(cx, |view, window, cx| {
                            view.variant = variant;
                            window.retained_tree.enabled = retained;
                            cx.notify();
                        })
                        .expect("window exists");
                    cx.test_window(window.into())
                        .simulate_frame_request(RequestFrameOptions::default());
                    let output = window
                        .update(cx, |view, window, _| {
                            if retained && repeat == 1 {
                                assert_eq!(window.retained_tree.stats.paint_replayed, 1);
                            }
                            let scene = &window.rendered_frame.scene;
                            assert!(!scene.quads.is_empty());
                            assert!(!scene.underlines.is_empty());
                            let layout = view.layout.as_ref().expect("current layout handle");
                            (
                                format!("{:?}{:?}", scene.quads, scene.underlines),
                                layout.wrapped_text(),
                                layout.bounds(),
                                layout.position_for_index(0),
                            )
                        })
                        .expect("window exists");
                    if retained {
                        outputs.push(output);
                    } else {
                        assert_eq!(
                            outputs.get(variant * 2 + repeat),
                            Some(&output),
                            "variant {variant}"
                        );
                    }
                }
            }
        }
    }

    #[gpui::test]
    fn retained_text_invalidates_when_fonts_change(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            RetainedTextView {
                text: "unchanged text",
                width: 120.,
            }
        });
        for fonts_changed in [false, true, false] {
            window
                .update(cx, |_, _, cx| {
                    if fonts_changed {
                        cx.text_system()
                            .add_fonts(Vec::new())
                            .expect("font registration succeeds");
                    }
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    assert_eq!(
                        window.retained_tree.stats.measure_recomputed > 0,
                        fonts_changed
                    );
                })
                .expect("window exists");
        }
    }

    impl Render for RetainedStyledTextView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let text = StyledText::new("one two three four five six seven eight nine ten");
            self.layout = Some(text.layout().clone());
            div()
                .w(px(self.width))
                .text_size(px(self.font_size))
                .when(self.clamp, |element| element.line_clamp(2))
                .child(text)
        }
    }

    #[gpui::test]
    fn retained_text_matches_full_layout_across_constraint_changes(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            RetainedStyledTextView {
                width: 120.,
                font_size: 14.,
                clamp: false,
                layout: None,
            }
        });
        let mut retained_outputs = Vec::new();
        let cases = [
            (120., 14., false),
            (60., 14., false),
            (120., 14., false),
            (120., 20., false),
            (60., 20., true),
            (120., 20., true),
            (60., 20., true),
            (120., 14., false),
        ];
        for retained in [true, false] {
            for (index, (width, font_size, clamp)) in cases.into_iter().enumerate() {
                window
                    .update(cx, |view, window, cx| {
                        view.width = width;
                        view.font_size = font_size;
                        view.clamp = clamp;
                        window.retained_tree.enabled = retained;
                        cx.notify();
                    })
                    .expect("window exists");
                cx.test_window(window.into())
                    .simulate_frame_request(RequestFrameOptions::default());
                let output = window
                    .update(cx, |view, window, _| {
                        let layout = view.layout.as_ref().expect("rendered text layout");
                        assert!(layout.bounds().size.height > px(0.));
                        assert!(layout.position_for_index(0).is_some());
                        (
                            layout.wrapped_text(),
                            layout.bounds(),
                            format!(
                                "{:?}{:?}",
                                window.rendered_frame.scene.monochrome_sprites,
                                window.rendered_frame.scene.subpixel_sprites
                            ),
                        )
                    })
                    .expect("window exists");
                if retained {
                    retained_outputs.push(output);
                } else {
                    assert_eq!(retained_outputs.get(index), Some(&output), "case {index}");
                }
            }
        }
    }

    impl Render for LeafLayoutView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().flex().children((0..100).map(|index| {
                div()
                    .id(index)
                    .w(px(if index == 0 && self.changed_size {
                        12.
                    } else {
                        10.
                    }))
                    .h(px(10.))
                    .bg(rgb(if index == 0 && self.changed_color {
                        0xabcdef
                    } else {
                        0x123456
                    }))
            }))
        }
    }

    #[gpui::test]
    fn unchanged_leaf_layouts_skip_request_layout(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            LeafLayoutView {
                changed_color: false,
                changed_size: false,
            }
        });
        for (changed_color, changed_size, reused) in
            [(true, false, 100), (true, true, 99), (false, true, 100)]
        {
            window
                .update(cx, |view, _, cx| {
                    view.changed_color = changed_color;
                    view.changed_size = changed_size;
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    assert_eq!(window.retained_tree.stats.layout_reused, reused);
                    assert!(window.retained_tree.stats.layout_recomputed < 5);
                    assert_eq!(window.retained_tree.stats.prepaint_reused, 100);
                    assert!(window.retained_tree.stats.prepaint_rebuilt < 5);
                })
                .expect("window exists");
        }
    }

    #[gpui::test]
    fn scene_ordering_is_retained_for_color_only_changes(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            LeafLayoutView {
                changed_color: false,
                changed_size: false,
            }
        });
        for changed_color in [true, false, true] {
            window
                .update(cx, |view, _, cx| {
                    view.changed_color = changed_color;
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
        }
        let retained_output = window
            .update(cx, |_, window, _| {
                assert_eq!(window.rendered_frame.scene.ordering_reused, 100);
                format!("{:?}", window.rendered_frame.scene.quads)
            })
            .expect("window exists");
        window
            .update(cx, |_, window, cx| {
                window.retained_tree.enabled = false;
                cx.notify();
            })
            .expect("window exists");
        cx.test_window(window.into())
            .simulate_frame_request(RequestFrameOptions::default());
        window
            .update(cx, |_, window, _| {
                assert_eq!(window.rendered_frame.scene.ordering_reused, 0);
                assert_eq!(
                    format!("{:?}", window.rendered_frame.scene.quads),
                    retained_output
                );
            })
            .expect("window exists");
    }

    struct PrepaintFallbackView {
        interactive: bool,
        revision: usize,
        prepaints: Rc<Cell<usize>>,
        handler_revision: Rc<Cell<usize>>,
    }

    impl Render for PrepaintFallbackView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let prepaints = self.prepaints.clone();
            let handler_revision = self.handler_revision.clone();
            let revision = self.revision;
            div()
                .flex()
                .child(
                    div()
                        .w(px(10.))
                        .h(px(10.))
                        .on_children_prepainted(move |bounds, _, _| {
                            assert!(bounds.is_empty());
                            prepaints.set(prepaints.get() + 1);
                        }),
                )
                .child(
                    div()
                        .id("target")
                        .w(px(20.))
                        .h(px(20.))
                        .bg(rgb(0x123456))
                        .when(self.interactive, |element| {
                            element.on_mouse_down(MouseButton::Left, move |_, _, _| {
                                handler_revision.set(revision)
                            })
                        }),
                )
        }
    }

    #[gpui::test]
    fn retained_prepaint_preserves_callbacks_and_falls_back(cx: &mut TestAppContext) {
        let prepaints = Rc::new(Cell::new(0));
        let handler_revision = Rc::new(Cell::new(0));
        let mut previous_slot = None;
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            PrepaintFallbackView {
                interactive: false,
                revision: 0,
                prepaints: prepaints.clone(),
                handler_revision: handler_revision.clone(),
            }
        });
        for (revision, (interactive, enabled, reused)) in [
            (false, true, 1),
            (true, true, 0),
            (true, true, 1),
            (false, true, 1),
            (false, false, 0),
            (false, true, 1),
        ]
        .into_iter()
        .enumerate()
        {
            let prepaints_before = prepaints.get();
            window
                .update(cx, |view, window, cx| {
                    view.interactive = interactive;
                    view.revision = revision + 1;
                    window.retained_tree.prepaint_enabled = enabled;
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            assert_eq!(prepaints.get(), prepaints_before + 1);
            window
                .update(cx, |_, window, cx| {
                    assert_eq!(
                        window.retained_tree.stats.prepaint_reused, reused,
                        "revision {revision}"
                    );
                    let node = window
                        .retained_tree
                        .nodes
                        .values()
                        .find(|node| node.key == ReconcileKey::Explicit("target".into()))
                        .expect("target node");
                    if matches!(revision, 1..=3) {
                        assert!(node.damage.contains(Damage::HANDLERS));
                        assert_eq!(node.damage.contains(Damage::PREPAINT), revision != 2);
                    }
                    if interactive {
                        let slot = window
                            .rendered_frame
                            .mouse_listeners
                            .iter()
                            .flatten()
                            .find_map(|handler| handler.slot)
                            .expect("retained mouse slot");
                        if let Some(previous) = previous_slot {
                            assert_eq!(slot, previous);
                        }
                        previous_slot = Some(slot);
                        assert!(window.retained_tree.stats.handlers_updated > 0);
                    } else {
                        assert!(window.rendered_frame.mouse_listeners.is_empty());
                    }
                    let previous_revision = handler_revision.get();
                    window.simulate_mouse_move(point(px(15.), px(5.)), cx);
                    window.dispatch_event(
                        MouseDownEvent {
                            position: point(px(15.), px(5.)),
                            button: MouseButton::Left,
                            modifiers: Default::default(),
                            click_count: 1,
                            first_mouse: false,
                        }
                        .to_platform_input(),
                        cx,
                    );
                    assert_eq!(
                        handler_revision.get(),
                        if interactive {
                            revision + 1
                        } else {
                            previous_revision
                        }
                    );
                })
                .expect("window exists");
        }
    }

    struct HitboxView {
        left: f32,
        viewport: f32,
        width: f32,
        blocking: bool,
        revision: usize,
        invoked: Rc<Cell<usize>>,
    }

    impl Render for HitboxView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let revision = self.revision;
            let invoked = self.invoked.clone();
            div()
                .w(px(self.viewport))
                .h(px(20.))
                .overflow_hidden()
                .child(
                    div()
                        .id("moving-hitbox")
                        .ml(px(self.left))
                        .w(px(self.width))
                        .h(px(20.))
                        .flex_shrink_0()
                        .when(self.blocking, |element| element.occlude())
                        .on_mouse_down(MouseButton::Left, move |_, _, _| invoked.set(revision)),
                )
        }
    }

    #[gpui::test]
    fn retained_hitboxes_recompose_clip_and_use_current_handlers(cx: &mut TestAppContext) {
        let invoked = Rc::new(Cell::new(0));
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            HitboxView {
                left: 0.,
                viewport: 60.,
                width: 20.,
                blocking: false,
                revision: 1,
                invoked: invoked.clone(),
            }
        });
        let mut outputs = Vec::new();
        for retained in [true, false] {
            for (index, (left, viewport, width, blocking, transforms, reused)) in [
                (0., 60., 20., false, true, true),
                (30., 60., 20., false, true, true),
                (50., 60., 20., false, true, true),
                (50., 100., 20., false, true, true),
                (80., 60., 20., false, true, true),
                (20., 60., 30., false, true, false),
                (20., 60., 30., true, true, false),
                (0., 60., 30., true, false, false),
            ]
            .into_iter()
            .enumerate()
            {
                window
                    .update(cx, |_, window, cx| {
                        window.retained_tree.enabled = retained;
                        cx.notify();
                    })
                    .expect("window exists");
                cx.test_window(window.into())
                    .simulate_frame_request(RequestFrameOptions::default());
                let previous = window
                    .update(cx, |view, window, cx| {
                        view.left = left;
                        view.viewport = viewport;
                        view.width = width;
                        view.blocking = blocking;
                        view.revision = index + 2;
                        window.retained_tree.transform_enabled = transforms;
                        cx.notify();
                        window.rendered_frame.hitboxes.first().expect("hitbox").id
                    })
                    .expect("window exists");
                cx.test_window(window.into())
                    .simulate_frame_request(RequestFrameOptions::default());
                let output = window
                    .update(cx, |_, window, cx| {
                        assert_eq!(
                            window.retained_tree.stats.hitboxes_replayed,
                            usize::from(retained && reused)
                        );
                        assert_eq!(window.rendered_frame.hitboxes.len(), 1);
                        let hitbox = window.rendered_frame.hitboxes.first().expect("hitbox");
                        assert_eq!(hitbox.id == previous, retained && reused);
                        let geometry = (hitbox.bounds, hitbox.content_mask, hitbox.behavior);
                        let mut clicks = Vec::new();
                        for x in [5., 25., 35., 55., 65., 85.] {
                            invoked.set(0);
                            let position = point(px(x), px(5.));
                            window.simulate_mouse_move(position, cx);
                            window.dispatch_event(
                                MouseDownEvent {
                                    position,
                                    button: MouseButton::Left,
                                    modifiers: Default::default(),
                                    click_count: 1,
                                    first_mouse: false,
                                }
                                .to_platform_input(),
                                cx,
                            );
                            let expected = if x >= left && x < (left + width).min(viewport) {
                                index + 2
                            } else {
                                0
                            };
                            assert_eq!(invoked.get(), expected);
                            clicks.push(invoked.get());
                        }
                        (geometry, clicks)
                    })
                    .expect("window exists");
                if retained {
                    outputs.push(output);
                } else {
                    assert_eq!(outputs.get(index), Some(&output));
                }
            }
        }
    }

    struct ScrollingView {
        scroll: crate::ScrollHandle,
    }

    impl Render for ScrollingView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("scroll")
                .flex()
                .flex_col()
                .w(px(100.))
                .h(px(50.))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .children((0..5).map(|index| {
                    div()
                        .id(index)
                        .w(px(60.))
                        .h(px(40.))
                        .flex_shrink_0()
                        .rounded_md()
                        .shadow_md()
                        .bg(rgb(0x112233 + index as u32 * 0x111111))
                }))
        }
    }

    #[gpui::test]
    fn scrolling_backgrounds_reclip_previously_invisible_geometry(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            ScrollingView {
                scroll: crate::ScrollHandle::new(),
            }
        });
        let mut expected = Vec::new();
        for enabled in [true, false] {
            for (index, offset) in [0., 20., 100., 0., 0.25, 0.].into_iter().enumerate() {
                window
                    .update(cx, |view, window, cx| {
                        window.retained_tree.enabled = enabled;
                        view.scroll.set_offset(point(px(0.), px(-offset)));
                        cx.notify();
                    })
                    .expect("window exists");
                cx.test_window(window.into())
                    .simulate_frame_request(RequestFrameOptions::default());
                window
                    .update(cx, |_, window, _| {
                        let scene = &window.rendered_frame.scene;
                        let output = format!("{:?} {:?}", scene.quads, scene.shadows);
                        if enabled {
                            if index == 1 || index == 2 || index == 3 {
                                assert!(window.retained_tree.stats.transform_only >= 5);
                            }
                            expected.push(output);
                        } else {
                            assert_eq!(Some(&output), expected.get(index), "scroll frame {index}");
                        }
                    })
                    .expect("window exists");
            }
        }
    }

    struct StyledView {
        alternate: bool,
        width: f32,
        opacity: f32,
    }

    #[gpui::test]
    fn retained_damage_distinguishes_color_from_layout(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            StyledView {
                alternate: false,
                width: 120.,
                opacity: 1.,
            }
        });
        for (alternate, width, layout_changed) in [
            (false, 120., false),
            (true, 120., false),
            (true, 160., true),
            (true, 160., false),
        ] {
            window
                .update(cx, |view, _, cx| {
                    view.alternate = alternate;
                    view.width = width;
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    let node = window
                        .retained_tree
                        .nodes
                        .values()
                        .find(|node| node.key == ReconcileKey::Explicit("styled".into()))
                        .expect("styled node");
                    assert!(node.properties.is_some(), "Div records resolved properties");
                    assert_eq!(node.damage.contains(Damage::LAYOUT), layout_changed);
                })
                .expect("window exists");
        }
    }

    impl Render for StyledView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("styled")
                .w(px(self.width))
                .h(px(60.))
                .bg(rgb(0x223344))
                .border_1()
                .border_color(rgb(0xffaa55))
                .rounded_md()
                .shadow_md()
                .opacity(self.opacity)
                .child(div().size(px(20.)).bg(rgb(if self.alternate {
                    0xabcdef
                } else {
                    0x123456
                })))
        }
    }

    #[gpui::test]
    fn builtin_decorations_replay_with_identical_full_frame_output(cx: &mut TestAppContext) {
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            StyledView {
                alternate: false,
                width: 60.,
                opacity: 1.,
            }
        });
        let states = [
            (false, 60., 1.),
            (false, 60., 1.),
            (true, 60., 1.),
            (true, 80., 1.),
            (true, 80., 0.5),
        ];
        let mut retained_output = Vec::new();
        for enabled in [true, false] {
            for (index, &(alternate, width, opacity)) in states.iter().enumerate() {
                window
                    .update(cx, |view, window, cx| {
                        window.retained_tree.enabled = enabled;
                        view.alternate = alternate;
                        view.width = width;
                        view.opacity = opacity;
                        cx.notify();
                    })
                    .expect("window exists");
                cx.test_window(window.into())
                    .simulate_frame_request(RequestFrameOptions::default());
                window
                    .update(cx, |_, window, _| {
                        let scene = &window.rendered_frame.scene;
                        let output = format!("{:?} {:?}", scene.quads, scene.shadows);
                        if enabled {
                            if index == 1 || index == 2 {
                                assert!(window.retained_tree.stats.paint_replayed >= 2);
                            }
                            retained_output.push(output);
                        } else {
                            assert_eq!(Some(&output), retained_output.get(index));
                        }
                    })
                    .expect("window exists");
            }
        }
    }

    impl Render for CanvasView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let paints = self.paints.clone();
            let interactive = self.interactive;
            let keyboard = self.keyboard;
            let revision = self.revision;
            let canvas = crate::canvas(
                move |bounds, window, _| {
                    if interactive {
                        window.insert_hitbox(bounds, crate::HitboxBehavior::Normal);
                    }
                },
                move |bounds, (), window, _| {
                    paints.set(paints.get() + 1);
                    if keyboard {
                        window.on_key_event::<crate::KeyDownEvent>(|_, _, _, _| {});
                    }
                    window.paint_quad(crate::fill(
                        bounds,
                        rgb(if revision.is_multiple_of(2) {
                            0xabcdef
                        } else {
                            0x123456
                        }),
                    ));
                },
            )
            .retained("canvas", revision)
            .w(px(self.width))
            .h(px(20.));
            div()
                .child(
                    div()
                        .w(px(10.))
                        .h(px(10.))
                        .when(self.prefix, |element| element.bg(rgb(0xffffff))),
                )
                .child(canvas)
        }
    }

    struct ImageCanvasView {
        image: std::sync::Arc<crate::RenderImage>,
        paints: Rc<Cell<usize>>,
    }

    impl Render for ImageCanvasView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let image = self.image.clone();
            let paints = self.paints.clone();
            div().child(
                crate::canvas(
                    |_, _, _| (),
                    move |bounds, (), window, _| {
                        paints.set(paints.get() + 1);
                        window
                            .paint_image(bounds, bounds, Default::default(), image, 0, false)
                            .expect("paint image");
                    },
                )
                .retained("image-canvas", 0)
                .size(px(16.)),
            )
        }
    }

    #[gpui::test]
    fn retained_canvas_revalidates_atlas_resources(cx: &mut TestAppContext) {
        let image = std::sync::Arc::new(crate::RenderImage::new(vec![image::Frame::new(
            image::ImageBuffer::from_pixel(16, 16, image::Rgba([255, 0, 0, 255])),
        )]));
        let paints = Rc::new(Cell::new(0));
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            ImageCanvasView {
                image: image.clone(),
                paints: paints.clone(),
            }
        });
        let mut previous = window
            .update(cx, |_, window, _| {
                window
                    .rendered_frame
                    .scene
                    .polychrome_sprites
                    .first()
                    .expect("sprite")
                    .tile
            })
            .expect("window exists");
        for drop_image in [false, false, true, false, false] {
            let before = paints.get();
            window
                .update(cx, |_, window, cx| {
                    if drop_image {
                        window.drop_image(image.clone()).expect("drop image");
                    }
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    assert_eq!(paints.get(), before + usize::from(drop_image));
                    let tile = window
                        .rendered_frame
                        .scene
                        .polychrome_sprites
                        .first()
                        .expect("sprite")
                        .tile;
                    assert_eq!(tile == previous, !drop_image);
                    previous = tile;
                })
                .expect("window exists");
        }
    }

    struct TextCanvasView {
        paints: Rc<Cell<usize>>,
        prefix: bool,
    }

    impl Render for TextCanvasView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let paints = self.paints.clone();
            div().when(self.prefix, |element| element.child(div().absolute().left(px(200.)).child("prefix")))
                .child(crate::canvas(|_, _, _| (), move |bounds, (), window, cx| {
                paints.set(paints.get() + 1);
                let style = window.text_style();
                let line = window.text_system().shape_line("text".into(), px(14.), &[style.to_run(4)], None);
                line.paint(bounds.origin, px(20.), crate::TextAlign::Left, None, window, cx).expect("paint text");
                let svg_bounds = crate::Bounds::new(bounds.origin + point(px(60.), px(0.)), crate::size(px(16.), px(16.)));
                window.paint_svg(svg_bounds, "retained-svg-canvas".into(), Some(br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16"/></svg>"#), Default::default(), rgb(0xff0000).into(), cx).expect("paint svg");
            }).retained("text-canvas", 0).w(px(100.)).h(px(20.)))
        }
    }

    #[gpui::test]
    fn retained_svg_replay_rebases_text_layouts(cx: &mut TestAppContext) {
        let paints = Rc::new(Cell::new(0));
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            TextCanvasView {
                paints: paints.clone(),
                prefix: false,
            }
        });
        let mut expected = None;
        for (index, (retained, fonts_changed, rendering_changed)) in [
            (true, false, false),
            (true, false, false),
            (true, true, false),
            (true, false, false),
            (true, false, true),
            (true, false, false),
            (false, false, false),
        ]
        .into_iter()
        .enumerate()
        {
            let before = paints.get();
            window
                .update(cx, |view, window, cx| {
                    view.prefix = index % 2 != 0;
                    window.retained_tree.paint_enabled = retained;
                    if fonts_changed {
                        cx.text_system().add_fonts(Vec::new()).expect("font update");
                    }
                    if rendering_changed {
                        cx.set_text_rendering_mode(crate::TextRenderingMode::Grayscale);
                    }
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            window
                .update(cx, |_, window, _| {
                    assert_eq!(
                        paints.get(),
                        before + usize::from(!retained || fonts_changed || rendering_changed)
                    );
                    // TestAppContext shapes text but its NoopTextSystem does not rasterize glyphs.
                    assert_eq!(window.rendered_frame.scene.monochrome_sprites.len(), 1);
                    let pixels = format!(
                        "{:?} {:?}",
                        window.rendered_frame.scene.monochrome_sprites,
                        window.rendered_frame.scene.subpixel_sprites
                    );
                    if let Some(expected) = &expected {
                        assert_eq!(&pixels, expected);
                    } else {
                        expected = Some(pixels);
                    }
                })
                .expect("window exists");
        }
    }

    #[gpui::test]
    fn canvas_geometry_rebases_and_invalidates(cx: &mut TestAppContext) {
        let paints = Rc::new(Cell::new(0));
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            CanvasView {
                revision: 0,
                width: 20.,
                prefix: false,
                interactive: false,
                keyboard: false,
                paints: paints.clone(),
            }
        });
        for (revision, width, interactive, expected_paints) in [
            (0, 20., false, 1),
            (0, 20., false, 1),
            (0, 20., false, 1),
            (1, 20., false, 2),
            (1, 30., false, 3),
            (1, 30., true, 4),
            (1, 30., true, 5),
            (1, 30., false, 6),
            (1, 30., false, 6),
        ] {
            window
                .update(cx, |view, _, cx| {
                    view.revision = revision;
                    view.width = width;
                    view.interactive = interactive;
                    view.prefix = !view.prefix;
                    cx.notify();
                })
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            assert_eq!(paints.get(), expected_paints);
        }
        let retained_output = window
            .update(cx, |_, window, _| {
                format!("{:?}", window.rendered_frame.scene.quads)
            })
            .expect("window exists");
        window
            .update(cx, |_, window, cx| {
                window.retained_tree.paint_enabled = false;
                cx.notify();
            })
            .expect("window exists");
        cx.test_window(window.into())
            .simulate_frame_request(RequestFrameOptions::default());
        window
            .update(cx, |_, window, _| {
                assert_eq!(
                    format!("{:?}", window.rendered_frame.scene.quads),
                    retained_output
                );
            })
            .expect("window exists");
        assert_eq!(paints.get(), 7);
    }

    #[gpui::test]
    fn canvas_keyboard_callbacks_force_rebuild(cx: &mut TestAppContext) {
        let paints = Rc::new(Cell::new(0));
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            CanvasView {
                revision: 0,
                width: 20.,
                prefix: false,
                interactive: false,
                keyboard: true,
                paints: paints.clone(),
            }
        });
        let initial_paints = paints.get();
        for expected in initial_paints + 1..=initial_paints + 3 {
            window
                .update(cx, |_, _, cx| cx.notify())
                .expect("window exists");
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            assert_eq!(paints.get(), expected);
            window
                .update(cx, |_, window, _| {
                    assert_eq!(window.retained_tree.stats.paint_replayed, 0)
                })
                .expect("window exists");
        }
    }

    #[test]
    fn keyed_reorder_preserves_identity_and_removal_expires_handles() {
        let mut tree = RetainedElementTree::new(true);
        tree.begin_frame();
        let first = element(&mut tree, 1);
        let second = element(&mut tree, 2);
        tree.finish_frame();
        tree.begin_frame();
        assert_eq!(element(&mut tree, 2), second);
        assert_eq!(element(&mut tree, 1), first);
        assert_eq!(tree.stats.nodes_created, 0);
        tree.finish_frame();
        tree.begin_frame();
        element(&mut tree, 2);
        assert!(tree.nodes.contains_key(first));
        tree.finish_frame();
        assert!(!tree.nodes.contains_key(first));
    }

    #[test]
    fn type_changes_and_parent_changes_remount() {
        let mut tree = RetainedElementTree::new(true);
        tree.begin_frame();
        let parent = element(&mut tree, 1);
        tree.enter(Some(parent));
        let child = element(&mut tree, 2);
        tree.enter(None);
        tree.finish_frame();
        tree.begin_frame();
        assert_eq!(element(&mut tree, 1), parent);
        assert_ne!(element(&mut tree, 2), child);
        tree.finish_frame();
        tree.begin_frame();
        assert_ne!(
            tree.begin_element(Some(ElementId::Integer(1)), TypeId::of::<bool>()),
            Some(parent)
        );
        tree.finish_frame();
        assert!(!tree.nodes.contains_key(parent));
    }

    #[test]
    fn cached_children_survive_frames_without_traversal() {
        let mut tree = RetainedElementTree::new(true);
        tree.begin_frame();
        let parent = element(&mut tree, 1);
        tree.enter(Some(parent));
        let child = element(&mut tree, 2);
        tree.enter(None);
        tree.finish_frame();
        for _ in 0..3 {
            tree.begin_frame();
            element(&mut tree, 1);
            tree.enter(Some(parent));
            tree.preserve_current_children();
            tree.enter(None);
            tree.finish_frame();
            assert!(tree.nodes.contains_key(child));
        }
    }

    #[test]
    fn retry_restores_replaced_identity_and_nested_transactions() {
        let mut tree = RetainedElementTree::new(true);
        tree.begin_frame();
        let parent = element(&mut tree, 1);
        tree.enter(Some(parent));
        let child = element(&mut tree, 2);
        tree.enter(None);
        tree.finish_frame();

        tree.begin_frame();
        element(&mut tree, 1);
        tree.enter(Some(parent));
        let outer = tree.checkpoint();
        let inner = tree.checkpoint();
        let replacement = tree
            .begin_element(Some(ElementId::Integer(2)), TypeId::of::<bool>())
            .expect("active frame");
        tree.end_transaction(inner, true);
        tree.end_transaction(outer, false);
        assert!(!tree.nodes.contains_key(replacement));
        assert_eq!(element(&mut tree, 2), child);
        tree.enter(None);
        tree.finish_frame();
        assert_eq!(tree.stats.nodes_created, 0);
        assert_eq!(tree.stats.nodes_removed, 0);
        assert_eq!(tree.stats.nodes_total, 2);
    }

    #[test]
    fn positional_identity_depends_on_slot_and_type() {
        let mut tree = RetainedElementTree::new(true);
        tree.begin_frame();
        let first = tree.begin_element(None, TypeId::of::<()>());
        let second = tree.begin_element(None, TypeId::of::<bool>());
        tree.finish_frame();
        tree.begin_frame();
        assert_eq!(tree.begin_element(None, TypeId::of::<()>()), first);
        assert_eq!(tree.begin_element(None, TypeId::of::<bool>()), second);
        tree.finish_frame();
        tree.begin_frame();
        assert_ne!(tree.begin_element(None, TypeId::of::<bool>()), second);
        assert_ne!(tree.begin_element(None, TypeId::of::<()>()), first);
        tree.finish_frame();
        assert_eq!(tree.stats.nodes_removed, 2);
    }

    #[test]
    fn disabled_tree_does_not_allocate_nodes() {
        let mut tree = RetainedElementTree::new(false);
        tree.begin_frame();
        assert_eq!(tree.begin_element(None, TypeId::of::<()>()), None);
        tree.finish_frame();
        assert!(tree.nodes.is_empty());
        assert!(tree.identities.is_empty());
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "duplicate retained element key")]
    fn duplicate_keys_report_path() {
        let mut tree = RetainedElementTree::new(true);
        tree.begin_frame();
        element(&mut tree, 1);
        element(&mut tree, 1);
    }
}
