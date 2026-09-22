use super::{RetainedElementTree, RetainedNodeId, Undo};
use crate::EntityId;

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct Damage: u16 {
        const BUILD = 1 << 0;
        const CHILDREN = 1 << 1;
        const LAYOUT = 1 << 2;
        const PREPAINT = 1 << 3;
        const HANDLERS = 1 << 4;
        const PAINT = 1 << 5;
        const TRANSFORM = 1 << 6;
        const CLIP = 1 << 7;
        const COMPOSITE = 1 << 8;
        const FULL = Self::BUILD.bits() | Self::CHILDREN.bits() | Self::LAYOUT.bits()
            | Self::PREPAINT.bits() | Self::HANDLERS.bits() | Self::PAINT.bits();
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct Isolation: u8 {
        const LAYOUT = 1 << 0;
        const PAINT = 1 << 1;
        const TRANSFORM = 1 << 2;
    }
}

impl RetainedElementTree {
    pub(crate) fn damage_current(&mut self, damage: Damage) {
        if self.active
            && let Some(id) = self.current
        {
            self.damage_node(id, damage);
        }
    }

    pub(super) fn damage_node(&mut self, id: RetainedNodeId, damage: Damage) {
        let propagate_layout = damage.intersects(Damage::LAYOUT | Damage::CHILDREN);
        let mut current = Some(id);
        while let Some(current_id) = current {
            let Some(node) = self.nodes.get_mut(current_id) else {
                break;
            };
            let change = if current_id == id {
                damage
            } else {
                Damage::LAYOUT
            };
            if !node.damage.contains(change) {
                if self.transactions > 0 {
                    self.undo
                        .push(Undo::Damage(current_id, node.damage, node.isolation));
                }
                node.damage |= change;
            }
            if !propagate_layout
                || (node.isolation.contains(Isolation::LAYOUT)
                    && (current_id != id || damage.contains(Damage::CHILDREN)))
            {
                break;
            }
            current = node.parent;
        }
    }

    pub(crate) fn isolate_current(&mut self, isolation: Isolation) {
        if !self.active {
            return;
        }
        if let Some(id) = self.current
            && let Some(node) = self.nodes.get_mut(id)
        {
            if node.isolation != isolation {
                if self.transactions > 0 {
                    self.undo
                        .push(Undo::Damage(id, node.damage, node.isolation));
                }
                node.isolation = isolation;
            }
        }
    }

    pub(crate) fn current_damage(&self) -> Damage {
        self.current
            .and_then(|id| self.nodes.get(id))
            .map_or(Damage::FULL, |node| node.damage)
    }

    pub(crate) fn invalidate_view(&mut self, view: EntityId) {
        for id in self.view_nodes.get(&view).into_iter().flatten() {
            if let Some(node) = self.nodes.get_mut(*id) {
                node.pending_damage |= Damage::BUILD;
            }
        }
    }

    pub(crate) fn refresh_damage(&mut self) {
        for node in self.nodes.values_mut() {
            node.damage = Damage::FULL;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::TypeId;

    #[test]
    fn layout_damage_stops_at_isolation_boundary() {
        let mut tree = RetainedElementTree::new(true);
        tree.begin_frame();
        let root = tree
            .begin_element(Some("root".into()), TypeId::of::<()>())
            .expect("root");
        tree.enter(Some(root));
        let boundary = tree
            .begin_element(Some("boundary".into()), TypeId::of::<()>())
            .expect("boundary");
        tree.enter(Some(boundary));
        let child = tree
            .begin_element(Some("child".into()), TypeId::of::<()>())
            .expect("child");
        for node in tree.nodes.values_mut() {
            node.damage = Damage::empty();
        }
        tree.nodes.get_mut(boundary).expect("boundary").isolation = Isolation::LAYOUT;
        tree.enter(Some(child));
        tree.damage_current(Damage::LAYOUT);
        assert_eq!(tree.nodes.get(child).expect("child").damage, Damage::LAYOUT);
        assert_eq!(
            tree.nodes.get(boundary).expect("boundary").damage,
            Damage::LAYOUT
        );
        assert!(tree.nodes.get(root).expect("root").damage.is_empty());
        for node in tree.nodes.values_mut() {
            node.damage = Damage::empty();
        }
        tree.enter(Some(boundary));
        tree.damage_current(Damage::LAYOUT);
        assert!(
            tree.nodes
                .get(root)
                .expect("root")
                .damage
                .contains(Damage::LAYOUT)
        );
    }

    #[test]
    fn paint_and_composition_do_not_dirty_ancestor_layout() {
        let mut tree = RetainedElementTree::new(true);
        tree.begin_frame();
        let root = tree
            .begin_element(Some("root".into()), TypeId::of::<()>())
            .expect("root");
        tree.enter(Some(root));
        let child = tree
            .begin_element(Some("child".into()), TypeId::of::<()>())
            .expect("child");
        for node in tree.nodes.values_mut() {
            node.damage = Damage::empty();
        }
        tree.enter(Some(child));
        let change = Damage::PAINT | Damage::TRANSFORM | Damage::CLIP | Damage::COMPOSITE;
        tree.damage_current(change);
        assert_eq!(tree.nodes.get(child).expect("child").damage, change);
        assert!(tree.nodes.get(root).expect("root").damage.is_empty());
    }

    #[test]
    fn discarded_damage_does_not_escape_transaction() {
        let mut tree = RetainedElementTree::new(true);
        tree.begin_frame();
        let node = tree
            .begin_element(Some("node".into()), TypeId::of::<()>())
            .expect("node");
        tree.enter(Some(node));
        tree.nodes.get_mut(node).expect("node").damage = Damage::empty();
        let checkpoint = tree.checkpoint();
        tree.damage_current(Damage::LAYOUT);
        tree.end_transaction(checkpoint, false);
        assert!(tree.nodes.get(node).expect("node").damage.is_empty());
    }

    #[test]
    fn new_frame_clears_completed_damage() {
        let mut tree = RetainedElementTree::new(true);
        tree.begin_frame();
        let node = tree
            .begin_element(Some("node".into()), TypeId::of::<()>())
            .expect("node");
        tree.finish_frame();
        tree.begin_frame();
        assert!(tree.nodes.get(node).expect("node").damage.is_empty());
    }

    #[test]
    fn view_notification_survives_frame_start_and_remount() {
        let mut tree = RetainedElementTree::new(true);
        let view = EntityId::from(1_u64);
        tree.begin_frame();
        let node = tree
            .begin_element(Some(crate::ElementId::View(view)), TypeId::of::<()>())
            .expect("node");
        tree.finish_frame();
        tree.invalidate_view(view);
        tree.begin_frame();
        assert_eq!(tree.nodes.get(node).expect("node").damage, Damage::BUILD);
        tree.finish_frame();
        assert!(!tree.view_nodes.contains_key(&view));
        tree.invalidate_view(view);
        tree.begin_frame();
        let remounted = tree
            .begin_element(Some(crate::ElementId::View(view)), TypeId::of::<()>())
            .expect("remounted");
        assert_ne!(node, remounted);
        assert_eq!(
            tree.nodes.get(remounted).expect("remounted").damage,
            Damage::FULL
        );
    }
}
