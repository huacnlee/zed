use super::{HitboxSnapshot, PaintSnapshot, RetainedElementTree, RetainedNode};
use std::cmp::Reverse;

pub(super) struct RetainedBudget {
    pub max_snapshot_bytes: usize,
    pub max_unused_generations: u64,
}

impl Default for RetainedBudget {
    fn default() -> Self {
        Self {
            max_snapshot_bytes: 8 * 1024 * 1024,
            max_unused_generations: 2,
        }
    }
}

impl RetainedNode {
    fn paint_snapshot_bytes(&self) -> usize {
        self.paint_snapshots.capacity() * size_of::<PaintSnapshot>()
            + self
                .paint_snapshots
                .iter()
                .map(PaintSnapshot::heap_bytes)
                .sum::<usize>()
    }

    pub(super) fn snapshot_bytes(&self) -> usize {
        self.paint_snapshot_bytes()
            + self
                .subtree_snapshot
                .as_ref()
                .map_or(0, |snapshot| snapshot.owned_bytes())
            + self
                .hitbox_snapshot
                .as_ref()
                .map_or(0, |_| size_of::<HitboxSnapshot>())
    }
}

impl RetainedElementTree {
    pub(super) fn trim_snapshots(&mut self) {
        for node in self.nodes.values_mut() {
            let previous_count = node.paint_snapshots.len();
            node.paint_snapshots.retain(|snapshot| {
                self.generation.wrapping_sub(snapshot.generation)
                    <= self.budget.max_unused_generations
            });
            if node.paint_snapshots.is_empty() {
                node.paint_snapshots = Vec::new();
                self.stats.snapshots_evicted += previous_count;
            } else if node.paint_snapshots.len() != previous_count {
                self.stats.snapshots_evicted += previous_count - node.paint_snapshots.len();
                node.paint_snapshots.shrink_to_fit();
            }
            if node.hitbox_snapshot.as_ref().is_some_and(|snapshot| {
                self.generation.wrapping_sub(snapshot.generation)
                    > self.budget.max_unused_generations
            }) {
                node.hitbox_snapshot = None;
                self.stats.snapshots_evicted += 1;
            }
            if node.subtree_snapshot.as_ref().is_some_and(|snapshot| {
                self.generation.wrapping_sub(snapshot.generation)
                    > self.budget.max_unused_generations
            }) {
                node.subtree_snapshot = None;
                self.stats.snapshots_evicted += 1;
            }
        }

        // Count owned allocation capacities, excluding shared font/assets and the
        // previous frame's storage referenced by range snapshots.
        self.stats.snapshot_bytes = self.nodes.values().map(RetainedNode::snapshot_bytes).sum();
        self.stats.property_bytes = self
            .nodes
            .values()
            .filter_map(|node| node.properties.as_ref())
            .map(|properties| properties.owned_bytes())
            .sum();
        if self.stats.snapshot_bytes <= self.budget.max_snapshot_bytes {
            return;
        }
        let mut candidates: Vec<_> = self
            .nodes
            .iter()
            .filter_map(|(id, node)| {
                let bytes = node.paint_snapshot_bytes();
                (bytes > 0).then_some((bytes, id))
            })
            .collect();
        candidates.sort_unstable_by_key(|(bytes, _)| Reverse(*bytes));
        for (bytes, id) in candidates {
            if self.stats.snapshot_bytes <= self.budget.max_snapshot_bytes {
                break;
            }
            if let Some(node) = self.nodes.get_mut(id) {
                self.stats.snapshots_evicted += node.paint_snapshots.len();
                node.paint_snapshots = Vec::new();
                self.stats.snapshot_bytes -= bytes;
            }
        }
        for node in self.nodes.values_mut() {
            if self.stats.snapshot_bytes <= self.budget.max_snapshot_bytes {
                break;
            }
            if node.hitbox_snapshot.take().is_some() {
                self.stats.snapshots_evicted += 1;
                self.stats.snapshot_bytes -= size_of::<HitboxSnapshot>();
            }
        }
        for node in self.nodes.values_mut() {
            if self.stats.snapshot_bytes <= self.budget.max_snapshot_bytes {
                break;
            }
            if let Some(snapshot) = node.subtree_snapshot.take() {
                self.stats.snapshots_evicted += 1;
                self.stats.snapshot_bytes -= snapshot.owned_bytes();
            }
        }
    }
}
