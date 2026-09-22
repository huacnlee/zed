use super::{Damage, Isolation, RetainedElementTree, Undo};
use crate::{App, Element, Pixels, TextStyle, Window};
use std::{
    any::{Any, TypeId},
    rc::Rc,
};

pub(crate) trait RetainableElement: Element {
    type RetainedProperties: 'static;

    fn retained_properties(&self) -> Self::RetainedProperties;
    fn diff(
        previous: &Self::RetainedProperties,
        current: &Self::RetainedProperties,
        environment: &EnvironmentDiff,
    ) -> Damage;
    fn isolation(&self) -> Isolation {
        Isolation::empty()
    }

    fn property_heap_bytes(_properties: &Self::RetainedProperties) -> usize {
        0
    }

    fn update_retained_properties(
        &self,
        previous: &mut Self::RetainedProperties,
        environment: &EnvironmentDiff,
    ) -> Damage {
        let current = self.retained_properties();
        let damage = Self::diff(previous, &current, environment);
        *previous = current;
        damage
    }
}

#[derive(Clone, PartialEq)]
struct PropertyEnvironment {
    rem_size: Pixels,
    scale_factor: f32,
    text_style: TextStyle,
    font_generation: usize,
    opacity: f32,
    active: bool,
}

impl PropertyEnvironment {
    fn capture(window: &Window, cx: &App) -> Self {
        Self {
            rem_size: window.rem_size(),
            scale_factor: window.scale_factor(),
            text_style: window.text_style(),
            font_generation: cx.text_system().font_generation(),
            opacity: window.element_opacity(),
            active: window.is_window_active(),
        }
    }
}

pub(crate) struct EnvironmentDiff {
    pub metrics: bool,
    pub text_layout: bool,
    pub text_paint: bool,
    pub composite: bool,
}

impl EnvironmentDiff {
    fn between(previous: &PropertyEnvironment, current: &PropertyEnvironment) -> Self {
        let previous_text = &previous.text_style;
        let current_text = &current.text_style;
        Self {
            metrics: previous.rem_size != current.rem_size
                || previous.scale_factor != current.scale_factor,
            text_layout: previous_text.font_family != current_text.font_family
                || previous_text.font_features != current_text.font_features
                || previous_text.font_fallbacks != current_text.font_fallbacks
                || previous_text.font_weight != current_text.font_weight
                || previous_text.font_style != current_text.font_style
                || previous_text.font_size != current_text.font_size
                || previous_text.line_height != current_text.line_height
                || previous_text.white_space != current_text.white_space
                || previous_text.text_overflow != current_text.text_overflow
                || previous_text.line_clamp != current_text.line_clamp
                || previous.font_generation != current.font_generation,
            text_paint: previous_text.color != current_text.color
                || previous_text.background_color != current_text.background_color
                || previous_text.underline != current_text.underline
                || previous_text.strikethrough != current_text.strikethrough
                || previous_text.text_align != current_text.text_align,
            composite: previous.opacity != current.opacity || previous.active != current.active,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        !self.metrics && !self.text_layout && !self.text_paint && !self.composite
    }
}

#[derive(Clone, Copy)]
struct RetainedElementVTable {
    type_id: TypeId,
    properties: fn(&dyn Any) -> Option<Rc<dyn Any>>,
    diff: fn(&dyn Any, &dyn Any, &EnvironmentDiff) -> Damage,
    update: fn(&dyn Any, &mut Rc<dyn Any>, &EnvironmentDiff) -> Damage,
    isolation: fn(&dyn Any) -> Isolation,
    property_size: usize,
    property_heap_bytes: fn(&dyn Any) -> usize,
}

impl RetainedElementVTable {
    fn of<E: RetainableElement>() -> Self {
        Self {
            type_id: TypeId::of::<E>(),
            properties: |element| {
                element
                    .downcast_ref::<E>()
                    .map(|element| Rc::new(element.retained_properties()) as Rc<dyn Any>)
            },
            diff: |previous, current, environment| match (
                previous.downcast_ref::<E::RetainedProperties>(),
                current.downcast_ref::<E::RetainedProperties>(),
            ) {
                (Some(previous), Some(current)) => E::diff(previous, current, environment),
                _ => Damage::FULL,
            },
            update: |element, previous, environment| {
                let Some(element) = element.downcast_ref::<E>() else {
                    return Damage::FULL;
                };
                if let Some(previous) = Rc::get_mut(previous)
                    .and_then(|previous| previous.downcast_mut::<E::RetainedProperties>())
                {
                    return element.update_retained_properties(previous, environment);
                }
                let current = element.retained_properties();
                let damage = previous
                    .downcast_ref::<E::RetainedProperties>()
                    .map_or(Damage::FULL, |previous| {
                        E::diff(previous, &current, environment)
                    });
                *previous = Rc::new(current);
                damage
            },
            isolation: |element| {
                element
                    .downcast_ref::<E>()
                    .map_or(Isolation::empty(), E::isolation)
            },
            property_size: size_of::<E::RetainedProperties>(),
            property_heap_bytes: |properties| {
                properties
                    .downcast_ref::<E::RetainedProperties>()
                    .map_or(0, E::property_heap_bytes)
            },
        }
    }
}

#[derive(Clone)]
pub(super) struct PropertySnapshot {
    vtable: RetainedElementVTable,
    value: Rc<dyn Any>,
    environment: PropertyEnvironment,
    pub(super) generation: u64,
}

impl PropertySnapshot {
    pub(super) fn owned_bytes(&self) -> usize {
        size_of::<Self>()
            + self.vtable.property_size
            + (self.vtable.property_heap_bytes)(self.value.as_ref())
    }
}

impl RetainedElementTree {
    fn reconcile_property_value<E: RetainableElement>(
        &mut self,
        value: E::RetainedProperties,
        isolation: Isolation,
        environment: PropertyEnvironment,
    ) {
        let Some(id) = self.current.filter(|_| self.active) else {
            return;
        };
        let vtable = RetainedElementVTable::of::<E>();
        let Some(node) = self.nodes.get_mut(id) else {
            return;
        };
        let damage = node
            .properties
            .as_ref()
            .filter(|previous| previous.vtable.type_id == vtable.type_id)
            .and_then(|previous| {
                previous
                    .value
                    .downcast_ref::<E::RetainedProperties>()
                    .map(|properties| {
                        E::diff(
                            properties,
                            &value,
                            &EnvironmentDiff::between(&previous.environment, &environment),
                        )
                    })
            })
            .unwrap_or(Damage::FULL);
        if self.transactions == 0
            && let Some(previous) = node
                .properties
                .as_mut()
                .filter(|previous| previous.vtable.type_id == vtable.type_id)
        {
            if let Some(properties) = Rc::get_mut(&mut previous.value)
                .and_then(|properties| properties.downcast_mut::<E::RetainedProperties>())
            {
                *properties = value;
            } else {
                previous.value = Rc::new(value);
            }
            previous.environment = environment;
            previous.generation = self.generation;
        } else {
            let previous = node.properties.replace(Box::new(PropertySnapshot {
                vtable,
                value: Rc::new(value),
                environment,
                generation: self.generation,
            }));
            if self.transactions > 0 {
                self.undo.push(Undo::Properties(id, previous));
            }
        }
        self.isolate_current(isolation);
        self.damage_node(id, damage);
    }

    fn reconcile_properties<E: RetainableElement>(
        &mut self,
        element: &E,
        environment: PropertyEnvironment,
    ) {
        let Some(id) = self.current.filter(|_| self.active) else {
            return;
        };
        let vtable = RetainedElementVTable::of::<E>();
        let Some(node) = self.nodes.get_mut(id) else {
            return;
        };
        let damage = if self.transactions == 0
            && let Some(previous) = node
                .properties
                .as_mut()
                .filter(|previous| previous.vtable.type_id == vtable.type_id)
        {
            let damage = (vtable.update)(
                element,
                &mut previous.value,
                &EnvironmentDiff::between(&previous.environment, &environment),
            );
            previous.environment = environment;
            previous.generation = self.generation;
            damage
        } else {
            let Some(value) = (vtable.properties)(element) else {
                self.damage_current(Damage::FULL);
                return;
            };
            let damage = node
                .properties
                .as_ref()
                .filter(|previous| previous.vtable.type_id == vtable.type_id)
                .map_or(Damage::FULL, |previous| {
                    (vtable.diff)(
                        previous.value.as_ref(),
                        value.as_ref(),
                        &EnvironmentDiff::between(&previous.environment, &environment),
                    )
                });
            let previous = node.properties.replace(Box::new(PropertySnapshot {
                vtable,
                value,
                environment,
                generation: self.generation,
            }));
            if self.transactions > 0 {
                self.undo.push(Undo::Properties(id, previous));
            }
            damage
        };
        self.isolate_current((vtable.isolation)(element));
        self.damage_node(id, damage);
    }
}

impl Window {
    pub(crate) fn retains_element_properties(&self) -> bool {
        self.retained_tree.active && self.retained_tree.current.is_some()
    }

    pub(crate) fn reconcile_retained_property_value<E: RetainableElement>(
        &mut self,
        value: E::RetainedProperties,
        isolation: Isolation,
        cx: &App,
    ) {
        if self.retained_tree.active {
            let environment = PropertyEnvironment::capture(self, cx);
            self.retained_tree
                .reconcile_property_value::<E>(value, isolation, environment);
        }
    }

    pub(crate) fn reconcile_retained_properties<E: RetainableElement>(
        &mut self,
        element: &E,
        cx: &App,
    ) {
        if self.retained_tree.active {
            let environment = PropertyEnvironment::capture(self, cx);
            self.retained_tree
                .reconcile_properties(element, environment);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Canvas, SharedString, TextAlign, canvas, px};

    fn revisioned_canvas(revision: u64) -> Canvas<()> {
        canvas(|_, _, _| (), |_, (), _, _| {}).retained("properties", revision)
    }

    fn begin_canvas(tree: &mut RetainedElementTree) {
        tree.begin_frame();
        let node = tree.begin_element(Some("properties".into()), TypeId::of::<Canvas<()>>());
        tree.enter(node);
    }

    fn allocations(tree: &RetainedElementTree) -> (*const PropertySnapshot, *const ()) {
        let snapshot = tree
            .nodes
            .values()
            .next()
            .expect("node")
            .properties
            .as_ref()
            .expect("properties");
        (
            std::ptr::from_ref(snapshot.as_ref()),
            Rc::as_ptr(&snapshot.value).cast::<()>(),
        )
    }

    #[test]
    fn text_environment_classifies_decoration_alignment_and_font_changes() {
        let environment = PropertyEnvironment {
            rem_size: px(16.),
            scale_factor: 1.,
            text_style: TextStyle::default(),
            font_generation: 0,
            opacity: 1.,
            active: true,
        };
        let paint_changes: [fn(&mut PropertyEnvironment); 7] = [
            |environment| environment.text_style.color = crate::red(),
            |environment| environment.text_style.background_color = Some(crate::red()),
            |environment| environment.text_style.underline = Some(Default::default()),
            |environment| environment.text_style.strikethrough = Some(Default::default()),
            |environment| environment.text_style.text_align = TextAlign::Center,
            |environment| environment.opacity = 0.5,
            |environment| environment.active = false,
        ];
        let layout_changes: [fn(&mut PropertyEnvironment); 8] = [
            |environment| environment.rem_size = px(24.),
            |environment| environment.scale_factor = 2.,
            |environment| environment.font_generation += 1,
            |environment| environment.text_style.font_family = "Other Font".into(),
            |environment| environment.text_style.font_size = px(24.).into(),
            |environment| environment.text_style.line_height = px(32.).into(),
            |environment| environment.text_style.line_clamp = Some(2),
            |environment| environment.text_style.white_space = crate::WhiteSpace::Nowrap,
        ];
        for (changes, expected) in [
            (paint_changes.as_slice(), Damage::PAINT),
            (
                layout_changes.as_slice(),
                Damage::LAYOUT | Damage::PREPAINT | Damage::PAINT,
            ),
        ] {
            for change in changes {
                let mut current = environment.clone();
                change(&mut current);
                let difference = EnvironmentDiff::between(&environment, &current);
                assert!(!difference.is_empty());
                assert_eq!(
                    <&'static str as RetainableElement>::diff(&"text", &"text", &difference),
                    expected
                );
                assert_eq!(
                    SharedString::diff(&"text".into(), &"text".into(), &difference),
                    expected
                );
            }
        }
    }

    #[test]
    fn plain_text_properties_separate_layout_and_paint_damage() {
        let environment = PropertyEnvironment {
            rem_size: px(16.),
            scale_factor: 1.,
            text_style: TextStyle::default(),
            font_generation: 0,
            opacity: 1.,
            active: true,
        };
        let mut tree = RetainedElementTree::new(true);
        let text = SharedString::from("retained text");
        for (current, environment, expected) in [
            (text.clone(), environment.clone(), Damage::FULL),
            (text.clone(), environment.clone(), Damage::empty()),
            (
                text.clone(),
                {
                    let mut environment = environment.clone();
                    environment.text_style.color = crate::red();
                    environment
                },
                Damage::PAINT,
            ),
            (
                text,
                {
                    let mut environment = environment.clone();
                    environment.text_style.font_size = px(24.).into();
                    environment
                },
                Damage::LAYOUT | Damage::PREPAINT | Damage::PAINT,
            ),
            (
                SharedString::from("changed text"),
                environment,
                Damage::LAYOUT | Damage::PREPAINT | Damage::PAINT,
            ),
        ] {
            tree.begin_frame();
            let node = tree.begin_element(None, TypeId::of::<SharedString>());
            tree.enter(node);
            tree.reconcile_properties(&current, environment);
            assert_eq!(tree.current_damage(), expected);
            let snapshot = tree
                .nodes
                .get(node.expect("text node"))
                .expect("retained text node")
                .properties
                .as_ref()
                .expect("text properties");
            assert_eq!(
                snapshot.owned_bytes(),
                size_of::<PropertySnapshot>() + size_of::<SharedString>() + current.len()
            );
            tree.finish_frame();
        }
    }

    #[test]
    fn resolved_property_values_preserve_transaction_and_shared_snapshots() {
        let environment = PropertyEnvironment {
            rem_size: px(16.),
            scale_factor: 1.,
            text_style: TextStyle::default(),
            font_generation: 0,
            opacity: 1.,
            active: true,
        };
        let mut tree = RetainedElementTree::new(true);
        begin_canvas(&mut tree);
        tree.reconcile_property_value::<Canvas<()>>(
            Some(1),
            Isolation::empty(),
            environment.clone(),
        );
        tree.finish_frame();
        let initial_allocations = allocations(&tree);
        begin_canvas(&mut tree);
        tree.reconcile_property_value::<Canvas<()>>(
            Some(1),
            Isolation::empty(),
            environment.clone(),
        );
        assert!(tree.current_damage().is_empty());
        assert_eq!(allocations(&tree), initial_allocations);
        let checkpoint = tree.checkpoint();
        tree.reconcile_property_value::<Canvas<()>>(
            Some(2),
            Isolation::empty(),
            environment.clone(),
        );
        assert_eq!(tree.current_damage(), Damage::PREPAINT | Damage::PAINT);
        tree.end_transaction(checkpoint, false);
        tree.reconcile_property_value::<Canvas<()>>(
            Some(1),
            Isolation::empty(),
            environment.clone(),
        );
        assert!(tree.current_damage().is_empty());
        assert_eq!(allocations(&tree), initial_allocations);
        let shared = tree
            .nodes
            .values()
            .next()
            .expect("node")
            .properties
            .as_ref()
            .expect("properties")
            .value
            .clone();
        tree.reconcile_property_value::<Canvas<()>>(Some(3), Isolation::empty(), environment);
        assert_eq!(shared.downcast_ref::<Option<u64>>(), Some(&Some(1)));
        assert_ne!(allocations(&tree).1, Rc::as_ptr(&shared).cast::<()>());
        assert_eq!(tree.current_damage(), Damage::PREPAINT | Damage::PAINT);
    }

    #[test]
    fn typed_properties_diff_roll_back_and_rebuild_after_eviction() {
        let environment = PropertyEnvironment {
            rem_size: px(16.),
            scale_factor: 1.,
            text_style: TextStyle::default(),
            font_generation: 0,
            opacity: 1.,
            active: true,
        };
        let mut tree = RetainedElementTree::new(true);
        begin_canvas(&mut tree);
        tree.reconcile_properties(&revisioned_canvas(1), environment.clone());
        tree.finish_frame();
        let initial_allocations = allocations(&tree);

        begin_canvas(&mut tree);
        tree.reconcile_properties(&revisioned_canvas(1), environment.clone());
        assert_eq!(allocations(&tree), initial_allocations);
        assert!(tree.current_damage().is_empty());
        let checkpoint = tree.checkpoint();
        tree.reconcile_properties(&revisioned_canvas(2), environment.clone());
        assert_eq!(tree.current_damage(), Damage::PREPAINT | Damage::PAINT);
        tree.end_transaction(checkpoint, false);
        tree.reconcile_properties(&revisioned_canvas(1), environment.clone());
        assert!(tree.current_damage().is_empty());
        tree.reconcile_properties(&revisioned_canvas(2), environment.clone());
        assert_eq!(tree.current_damage(), Damage::PREPAINT | Damage::PAINT);
        tree.finish_frame();
        assert_eq!(allocations(&tree), initial_allocations);

        begin_canvas(&mut tree);
        let mut changed_environment = environment;
        changed_environment.scale_factor *= 2.;
        tree.reconcile_properties(&revisioned_canvas(2), changed_environment.clone());
        assert_eq!(tree.current_damage(), Damage::PREPAINT | Damage::PAINT);
        let shared = tree
            .nodes
            .values()
            .next()
            .expect("node")
            .properties
            .as_ref()
            .expect("properties")
            .value
            .clone();
        tree.reconcile_properties(&revisioned_canvas(3), changed_environment.clone());
        assert_eq!(shared.downcast_ref::<Option<u64>>(), Some(&Some(2)));
        let current = &tree
            .nodes
            .values()
            .next()
            .expect("node")
            .properties
            .as_ref()
            .expect("properties")
            .value;
        assert_eq!(current.downcast_ref::<Option<u64>>(), Some(&Some(3)));
        assert!(!Rc::ptr_eq(&shared, current));
        drop(shared);
        tree.budget.max_snapshot_bytes = 0;
        tree.finish_frame();
        assert_eq!(tree.stats.snapshot_bytes, 0);
        assert!(tree.stats.property_bytes > 0);
        assert!(tree.nodes.values().all(|node| node.properties.is_some()));

        begin_canvas(&mut tree);
        tree.reconcile_properties(&revisioned_canvas(3), changed_environment);
        assert!(tree.current_damage().is_empty());
    }
}
