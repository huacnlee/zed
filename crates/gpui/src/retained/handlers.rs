use super::{Damage, RetainedElementTree, RetainedNodeId, Undo};
use crate::{AnyMouseListener, FocusId, HitboxId, KeyContext, Window};
use std::any::TypeId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HandlerSlotId {
    node: RetainedNodeId,
    kind: HandlerKind,
    ordinal: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum HandlerKind {
    Mouse(TypeId),
    Key(TypeId),
    Action(TypeId),
    Modifiers,
}

#[derive(Clone, PartialEq, Eq)]
enum HandlerTarget {
    Mouse(HitboxId),
    Dispatch {
        focus: Option<FocusId>,
        context: Option<KeyContext>,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct HandlerShape {
    kind: HandlerKind,
    target: Option<HandlerTarget>,
}

#[derive(Clone)]
pub(crate) struct SlottedHandler<T> {
    pub slot: Option<HandlerSlotId>,
    pub listener: T,
}

pub(crate) struct MouseHandler {
    pub slot: Option<HandlerSlotId>,
    pub listener: AnyMouseListener,
}

pub(crate) type HandlerTable = Vec<Option<MouseHandler>>;

impl RetainedElementTree {
    pub(crate) fn mouse_handler_slot(&mut self, event_type: TypeId) -> Option<HandlerSlotId> {
        self.handler_slot(
            HandlerKind::Mouse(event_type),
            self.handler_target.map(HandlerTarget::Mouse),
        )
    }

    fn handler_slot(
        &mut self,
        kind: HandlerKind,
        target: Option<HandlerTarget>,
    ) -> Option<HandlerSlotId> {
        if !self.active {
            return None;
        }
        let id = self.current?;
        let node = self.nodes.get_mut(id)?;
        if self.transactions > 0 {
            self.undo.push(Undo::Handlers(
                id,
                node.handler_shapes.clone(),
                node.handler_cursor,
            ));
        }
        let ordinal = node.handler_cursor;
        node.handler_cursor += 1;
        let target_known = target.is_some();
        let shape = HandlerShape { kind, target };
        let changed = node.handler_shapes.get(ordinal) != Some(&shape);
        if let Some(previous) = node.handler_shapes.get_mut(ordinal) {
            *previous = shape;
        } else {
            node.handler_shapes.push(shape);
        }
        let mut damage = Damage::HANDLERS;
        // A custom callback can capture arbitrary hitboxes. Without an explicit
        // target, equal event types alone do not prove compatible geometry.
        if changed || !target_known {
            damage |= Damage::PREPAINT;
        }
        self.damage_node(id, damage);
        self.stats.handlers_updated += 1;
        Some(HandlerSlotId {
            node: id,
            kind,
            ordinal,
        })
    }

    pub(crate) fn finish_handlers(&mut self) {
        let Some(id) = self.current.filter(|_| self.active) else {
            return;
        };
        let Some(node) = self.nodes.get_mut(id) else {
            return;
        };
        if node.handler_shapes.len() != node.handler_cursor {
            if self.transactions > 0 {
                self.undo.push(Undo::Handlers(
                    id,
                    node.handler_shapes.clone(),
                    node.handler_cursor,
                ));
            }
            node.handler_shapes.truncate(node.handler_cursor);
            self.damage_node(id, Damage::HANDLERS | Damage::PREPAINT);
        }
    }
}

impl Window {
    pub(crate) fn retained_dispatch_handler_slot(
        &mut self,
        kind: HandlerKind,
    ) -> Option<HandlerSlotId> {
        let dispatch_tree = &self.next_frame.dispatch_tree;
        let target = dispatch_tree
            .active_node_id()
            .filter(|id| Some(*id) == self.retained_tree.handler_dispatch_target)
            .map(|id| {
                let node = dispatch_tree.node(id);
                HandlerTarget::Dispatch {
                    focus: node.focus_id,
                    context: node.context.clone(),
                }
            });
        self.retained_tree.handler_slot(kind, target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement, ParentElement,
        Render, RequestFrameOptions, Styled, TestAppContext, div, px,
    };
    use std::{cell::Cell, rc::Rc};

    crate::actions!(retained_dispatch_test, [Invoke]);

    struct KeyboardChild {
        focus: FocusHandle,
        revision: usize,
        renders: Rc<Cell<usize>>,
        key: Rc<Cell<usize>>,
        action: Rc<Cell<usize>>,
        modifiers: Rc<Cell<usize>>,
    }

    impl Render for KeyboardChild {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let revision = self.revision;
            let key = self.key.clone();
            let action = self.action.clone();
            let modifiers = self.modifiers.clone();
            div()
                .id("keyboard-target")
                .track_focus(&self.focus)
                .key_context("RetainedKeyboard")
                .w(px(100.))
                .h(px(20.))
                .on_key_down(move |_, _, _| key.set(revision))
                .on_action(move |_: &Invoke, _, _| action.set(revision))
                .on_modifiers_changed(move |_, _, _| modifiers.set(revision))
        }
    }

    struct KeyboardRoot {
        child: Entity<KeyboardChild>,
        prefix: usize,
    }

    impl Render for KeyboardRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .children((0..self.prefix).map(|_| div().size(px(0.))))
                .child(self.child.clone().cached(Default::default()))
        }
    }

    #[gpui::test]
    fn keyboard_slots_refresh_callbacks_and_survive_cached_rebasing(cx: &mut TestAppContext) {
        let renders = Rc::new(Cell::new(0));
        let key = Rc::new(Cell::new(0));
        let action = Rc::new(Cell::new(0));
        let modifiers = Rc::new(Cell::new(0));
        let child = cx.new(|cx| KeyboardChild {
            focus: cx.focus_handle(),
            revision: 0,
            renders: renders.clone(),
            key: key.clone(),
            action: action.clone(),
            modifiers: modifiers.clone(),
        });
        let window = cx.add_window(|window, _| {
            window.retained_tree = RetainedElementTree::new(true);
            KeyboardRoot {
                child: child.clone(),
                prefix: 0,
            }
        });
        window
            .update(cx, |_, window, cx| {
                let focus = child.read(cx).focus.clone();
                window.focus(&focus, cx);
                window.activate_window();
            })
            .expect("window exists");
        let mut previous_slots = None;
        let mut previous_dispatch_node = None;
        for revision in 1..4 {
            child.update(cx, |child, cx| {
                child.revision = revision;
                cx.notify();
            });
            cx.test_window(window.into())
                .simulate_frame_request(RequestFrameOptions::default());
            for cached in [false, true] {
                if cached {
                    let renders_before = renders.get();
                    window
                        .update(cx, |view, _, cx| {
                            view.prefix = revision;
                            cx.notify();
                        })
                        .expect("window exists");
                    cx.test_window(window.into())
                        .simulate_frame_request(RequestFrameOptions::default());
                    assert_eq!(renders.get(), renders_before);
                }
                cx.update_window(window.into(), |_, window, cx| {
                    let focus = child.read(cx).focus.clone();
                    let node_id = window
                        .rendered_frame
                        .dispatch_tree
                        .focusable_node_id(focus.id)
                        .expect("focused node");
                    if cached {
                        assert_ne!(Some(node_id), previous_dispatch_node);
                    }
                    previous_dispatch_node = Some(node_id);
                    let node = window.rendered_frame.dispatch_tree.node(node_id);
                    let slots = (
                        node.key_listeners
                            .iter()
                            .map(|handler| handler.slot.expect("key slot"))
                            .collect::<Vec<_>>(),
                        node.action_listeners
                            .iter()
                            .map(|handler| handler.slot.expect("action slot"))
                            .collect::<Vec<_>>(),
                        node.modifiers_changed_listeners
                            .iter()
                            .map(|handler| handler.slot.expect("modifiers slot"))
                            .collect::<Vec<_>>(),
                    );
                    assert!(!slots.0.is_empty() && !slots.1.is_empty() && !slots.2.is_empty());
                    if let Some(previous) = &previous_slots {
                        assert_eq!(&slots, previous);
                    }
                    previous_slots = Some(slots);
                    if cached {
                        assert!(window.retained_tree.stats.handlers_replayed >= 3);
                    }
                    window.dispatch_event(
                        crate::PlatformInput::KeyDown(crate::KeyDownEvent {
                            keystroke: crate::Keystroke::parse("a").expect("keystroke"),
                            is_held: false,
                            prefer_character_input: false,
                        }),
                        cx,
                    );
                    focus.dispatch_action(&Invoke, window, cx);
                    window.dispatch_event(
                        crate::PlatformInput::ModifiersChanged(crate::ModifiersChangedEvent {
                            modifiers: crate::Modifiers::control(),
                            capslock: Default::default(),
                        }),
                        cx,
                    );
                    assert_eq!(key.get(), revision);
                    assert_eq!(action.get(), revision);
                    assert_eq!(modifiers.get(), revision);
                })
                .expect("window exists");
            }
        }
    }

    #[test]
    fn dispatch_handler_shape_distinguishes_kind_and_context() {
        let mut tree = RetainedElementTree::new(true);
        let event_type = TypeId::of::<crate::KeyDownEvent>();
        for (kind, context, prepaint) in [
            (HandlerKind::Key(event_type), None, true),
            (HandlerKind::Key(event_type), None, false),
            (HandlerKind::Action(event_type), None, true),
            (
                HandlerKind::Action(event_type),
                Some(KeyContext::default()),
                true,
            ),
            (
                HandlerKind::Action(event_type),
                Some(KeyContext::default()),
                false,
            ),
        ] {
            tree.begin_frame();
            let node = tree.begin_element(Some("dispatch".into()), TypeId::of::<()>());
            tree.enter(node);
            tree.handler_slot(
                kind,
                Some(HandlerTarget::Dispatch {
                    focus: None,
                    context,
                }),
            );
            tree.finish_handlers();
            let damage = tree
                .nodes
                .get(node.expect("node"))
                .expect("live node")
                .damage;
            assert!(damage.contains(Damage::HANDLERS));
            assert_eq!(damage.contains(Damage::PREPAINT), prepaint);
            tree.finish_frame();
        }
        tree.begin_frame();
        let node = tree.begin_element(Some("dispatch".into()), TypeId::of::<()>());
        tree.enter(node);
        tree.finish_handlers();
        let node = tree.nodes.get(node.expect("node")).expect("live node");
        assert!(node.handler_shapes.is_empty());
        assert!(node.damage.contains(Damage::HANDLERS | Damage::PREPAINT));
    }

    #[test]
    fn handler_slots_are_stable_and_transactional() {
        let mut tree = RetainedElementTree::new(true);
        tree.begin_frame();
        let node = tree.begin_element(Some("handlers".into()), TypeId::of::<()>());
        tree.enter(node);
        let first = tree.mouse_handler_slot(TypeId::of::<crate::MouseDownEvent>());
        tree.finish_handlers();
        tree.finish_frame();
        tree.begin_frame();
        let node = tree.begin_element(Some("handlers".into()), TypeId::of::<()>());
        tree.enter(node);
        let checkpoint = tree.checkpoint();
        assert_ne!(
            tree.mouse_handler_slot(TypeId::of::<crate::MouseUpEvent>()),
            first
        );
        tree.end_transaction(checkpoint, false);
        assert_eq!(
            tree.mouse_handler_slot(TypeId::of::<crate::MouseDownEvent>()),
            first
        );
        tree.finish_handlers();
    }
}
