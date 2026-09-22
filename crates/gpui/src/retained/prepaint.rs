use super::{RetainedElementTree, Undo};
use crate::{Bounds, Hitbox, HitboxBehavior, Pixels, PrepaintStateIndex, Window};
use std::ops::Range;

#[derive(Clone)]
pub(super) struct PrepaintSnapshot {
    range: Range<PrepaintStateIndex>,
    generation: u64,
}

impl PrepaintSnapshot {
    pub(super) fn new(range: Range<PrepaintStateIndex>, window: &Window) -> Self {
        Self {
            range,
            generation: window.retained_tree.generation,
        }
    }

    pub(super) fn is_previous_frame(&self, window: &Window) -> bool {
        self.generation.wrapping_add(1) == window.retained_tree.generation
    }

    pub(super) fn replay(&mut self, window: &mut Window) -> bool {
        if !self.is_previous_frame(window) {
            return false;
        }
        let start = window.prepaint_index();
        window.reuse_prepaint(self.range.clone());
        self.range = start..window.prepaint_index();
        self.generation = window.retained_tree.generation;
        true
    }
}

#[derive(Clone)]
pub(super) struct HitboxSnapshot {
    hitbox: Hitbox,
    pub(super) generation: u64,
}

impl RetainedElementTree {
    fn record_hitbox(&mut self, hitbox: Hitbox) {
        let Some(id) = self.current else { return };
        let Some(node) = self.nodes.get_mut(id) else {
            return;
        };
        let snapshot = HitboxSnapshot {
            hitbox,
            generation: self.generation,
        };
        if self.transactions == 0
            && let Some(previous) = node.hitbox_snapshot.as_mut()
        {
            **previous = snapshot;
            return;
        }
        let previous = node.hitbox_snapshot.replace(Box::new(snapshot));
        if self.transactions > 0 {
            self.undo.push(Undo::Hitbox(id, previous));
        }
    }
}

impl Window {
    pub(crate) fn insert_retained_hitbox(
        &mut self,
        bounds: Bounds<Pixels>,
        behavior: HitboxBehavior,
    ) -> Hitbox {
        let hitbox = self.insert_hitbox(bounds, behavior);
        if self.retained_tree.retains_prepaint() {
            self.retained_tree.record_hitbox(hitbox.clone());
        }
        hitbox
    }

    pub(crate) fn reuse_retained_hitbox(
        &mut self,
        bounds: Bounds<Pixels>,
        behavior: HitboxBehavior,
    ) -> Option<Hitbox> {
        if !self.can_reuse_retained_prepaint() {
            return None;
        }
        let snapshot = self
            .retained_tree
            .nodes
            .get(self.retained_tree.current?)?
            .hitbox_snapshot
            .as_ref()?;
        if snapshot.generation.wrapping_add(1) != self.retained_tree.generation
            || snapshot.hitbox.bounds.size != bounds.size
            || snapshot.hitbox.behavior != behavior
            || (!self.retained_tree.transform_enabled
                && (snapshot.hitbox.bounds != bounds
                    || snapshot.hitbox.content_mask != self.content_mask()))
        {
            return None;
        }
        let mut hitbox = snapshot.hitbox.clone();
        hitbox.bounds = bounds;
        hitbox.content_mask = self.content_mask();
        self.next_frame.hitboxes.push(hitbox.clone());
        self.retained_tree.record_hitbox(hitbox.clone());
        self.retained_tree.stats.hitboxes_replayed += 1;
        Some(hitbox)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        InteractiveElement, IntoElement, MouseButton, Styled, TestAppContext, div, point, px, size,
    };
    use std::any::TypeId;

    #[gpui::test]
    fn snapshot_rejects_current_and_skipped_frames(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        window.update(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            window.retained_tree.begin_frame();
            let index = window.prepaint_index();
            let mut snapshot = PrepaintSnapshot::new(index.clone()..index, window);
            assert!(!snapshot.replay(window));
            window.retained_tree.begin_frame();
            assert!(snapshot.replay(window));
            assert!(!snapshot.replay(window));
            window.retained_tree.begin_frame();
            window.retained_tree.begin_frame();
            assert!(!snapshot.replay(window));
        });
    }

    #[gpui::test]
    fn retry_restores_hitbox_snapshots(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        window.draw(point(px(0.), px(0.)), size(px(20.), px(20.)), |_, _| {
            div()
                .size_full()
                .on_mouse_down(MouseButton::Left, |_, _, _| {})
                .into_any_element()
        });
        let hitbox = window.update(|window, _| {
            window
                .rendered_frame
                .hitboxes
                .first()
                .expect("hitbox")
                .clone()
        });
        let mut tree = RetainedElementTree::new(true);
        tree.begin_frame();
        let node = tree.begin_element(Some("target".into()), TypeId::of::<()>());
        tree.enter(node);
        let checkpoint = tree.checkpoint();
        tree.record_hitbox(hitbox.clone());
        tree.end_transaction(checkpoint, false);
        assert!(
            tree.nodes
                .get(node.expect("node"))
                .expect("live node")
                .hitbox_snapshot
                .is_none()
        );

        tree.record_hitbox(hitbox.clone());
        tree.finish_frame();
        tree.begin_frame();
        let node = tree.begin_element(Some("target".into()), TypeId::of::<()>());
        tree.enter(node);
        let checkpoint = tree.checkpoint();
        let mut moved = hitbox.clone();
        moved.bounds.origin.x += px(10.);
        tree.record_hitbox(moved);
        let nested = tree.checkpoint();
        tree.record_hitbox(hitbox.clone());
        tree.end_transaction(nested, true);
        tree.end_transaction(checkpoint, false);
        let snapshot = tree
            .nodes
            .get(node.expect("node"))
            .expect("live node")
            .hitbox_snapshot
            .as_ref()
            .expect("restored snapshot");
        assert_eq!(snapshot.hitbox.bounds, hitbox.bounds);
        assert_eq!(snapshot.generation.wrapping_add(1), tree.generation);
    }
}
