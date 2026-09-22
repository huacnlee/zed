use super::RetainedElementTree;
use crate::{EntityId, InspectorElementId};

pub(super) struct InspectorNode {
    id: InspectorElementId,
    element_type: &'static str,
    owner: Option<EntityId>,
}

impl RetainedElementTree {
    pub(crate) fn bind_inspector(
        &mut self,
        id: Option<&InspectorElementId>,
        element_type: &'static str,
        owner: Option<EntityId>,
    ) {
        let Some(node) = self.current.and_then(|id| self.nodes.get_mut(id)) else {
            return;
        };
        node.inspector = id.map(|id| {
            Box::new(InspectorNode {
                id: id.clone(),
                element_type,
                owner,
            })
        });
    }

    pub(crate) fn inspector_lines(&self, id: &InspectorElementId) -> Vec<String> {
        if !self.enabled {
            return vec!["Retained tree disabled (GPUI_RETAINED_TREE=1 to enable)".into()];
        }
        let mut lines = vec!["Inspector forces full rebuild for retained phase hooks.".into()];
        if let Some((node_id, node, inspector)) = self.nodes.iter().find_map(|(node_id, node)| {
            node.inspector
                .as_ref()
                .filter(|inspector| &inspector.id == id)
                .map(|inspector| (node_id, node, inspector))
        }) {
            lines.extend([
                format!("Retained node: {node_id:?}"),
                format!("Key: {:?}", node.key),
                format!("Type: {}", inspector.element_type),
                format!("Owner view: {:?}", inspector.owner),
                format!(
                    "Parent: {:?}; last committed children: {}",
                    node.parent,
                    node.children.len()
                ),
                format!("Layout IDs: {:?}", node.layouts),
                format!("Handler slots: {}", node.handler_shapes.len()),
                format!(
                    "Typed properties generation: {:?}",
                    node.properties
                        .as_ref()
                        .map(|properties| properties.generation)
                ),
                format!("Damage: {:?}; isolation: {:?}", node.damage, node.isolation),
                format!("Owned snapshot storage: {} bytes", node.snapshot_bytes()),
                format!(
                    "Owned property storage: {} bytes",
                    node.properties
                        .as_ref()
                        .map_or(0, |properties| properties.owned_bytes())
                ),
                format!(
                    "Paint snapshot generations: {:?}",
                    node.paint_snapshots
                        .iter()
                        .map(|snapshot| snapshot.generation)
                        .collect::<Vec<_>>()
                ),
                format!(
                    "Hitbox snapshot generation: {:?}",
                    node.hitbox_snapshot
                        .as_ref()
                        .map(|snapshot| snapshot.generation)
                ),
                format!(
                    "Paint resource epochs: {:?}",
                    node.paint_snapshots
                        .iter()
                        .map(|snapshot| snapshot.resource_generation)
                        .collect::<Vec<_>>()
                ),
            ]);
        } else {
            lines.push("No retained node recorded for this element.".into());
        }
        if let Some((generation, stats)) = &self.completed_stats {
            lines.extend([
                format!("Previous completed frame: {generation}"),
                format!(
                    "Nodes: {} total, {} created, {} removed",
                    stats.nodes_total, stats.nodes_created, stats.nodes_removed
                ),
                format!(
                    "Layout: {} rebuilt, {} reused; {} measurements",
                    stats.layout_recomputed, stats.layout_reused, stats.measure_recomputed
                ),
                format!(
                    "Prepaint: {} rebuilt, {} reused; {} hitboxes replayed",
                    stats.prepaint_rebuilt, stats.prepaint_reused, stats.hitboxes_replayed
                ),
                format!(
                    "Paint methods: {}; snapshots replayed: {}",
                    stats.paint_rebuilt, stats.paint_replayed
                ),
                format!(
                    "Handlers: {} updated, {} replayed",
                    stats.handlers_updated, stats.handlers_replayed
                ),
                format!(
                    "Transform-only: {}; scene order reused: {}",
                    stats.transform_only, stats.scene_order_reused
                ),
                format!(
                    "Owned snapshots: {} / {} bytes; {} evicted; properties: {} bytes",
                    stats.snapshot_bytes,
                    self.budget.max_snapshot_bytes,
                    stats.snapshots_evicted,
                    stats.property_bytes
                ),
            ]);
        }
        lines
    }
}
