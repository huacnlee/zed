use crate::{
    AnyElement, AnyEntity, AnyWeakEntity, App, Bounds, ContentMask, Context, Element, ElementId,
    Entity, EntityId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, PaintIndex,
    Pixels, PrepaintStateIndex, Render, RenderOnce, Style, StyleRefinement, TextStyle, WeakEntity,
};
use crate::{Empty, Window};
use anyhow::Result;
use collections::FxHashSet;
use refineable::Refineable;
use std::mem;
use std::{any::TypeId, fmt, ops::Range};

/// A dynamically-typed view handle that can be downcast to a specific `Entity<V>`.
///
/// This is the type-erased counterpart to [`ViewElement`]: it holds an entity plus
/// a function pointer to its render, and is itself a [`View`], so embedding it as an
/// element goes through the same [`ViewElement`] machinery as any other view.
#[derive(Clone, Debug)]
pub struct AnyView {
    entity: AnyEntity,
    render: fn(&AnyView, &mut Window, &mut App) -> AnyElement,
}

impl<V: Render> From<Entity<V>> for AnyView {
    fn from(value: Entity<V>) -> Self {
        AnyView {
            entity: value.into_any(),
            render: any_view::render::<V>,
        }
    }
}

impl AnyView {
    /// Embed this view as a cached [`ViewElement`] laid out at `style`.
    ///
    /// The rendered subtree is recycled from the previous frame unless
    /// [Context::notify] was called on the backing entity since it was rendered
    /// (or [Window::refresh] is called, which ignores caching).
    pub fn cached(self, style: StyleRefinement) -> ViewElement<AnyView> {
        ViewElement::new(self).cached(style)
    }

    /// Convert this to a weak handle.
    pub fn downgrade(&self) -> AnyWeakView {
        AnyWeakView {
            entity: self.entity.downgrade(),
            render: self.render,
        }
    }

    /// Convert this to a [Entity] of a specific type.
    /// If this handle does not contain a view of the specified type, returns itself in an `Err` variant.
    pub fn downcast<T: 'static>(self) -> Result<Entity<T>, Self> {
        match self.entity.downcast() {
            Ok(entity) => Ok(entity),
            Err(entity) => Err(Self {
                entity,
                render: self.render,
            }),
        }
    }

    /// Gets the [TypeId] of the underlying view.
    pub fn entity_type(&self) -> TypeId {
        self.entity.entity_type
    }

    /// The [`EntityId`] of this view.
    pub fn entity_id(&self) -> EntityId {
        self.entity.entity_id()
    }
}

impl PartialEq for AnyView {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl Eq for AnyView {}

/// `AnyView` is the type-erased [`View`]: its `render` is a function pointer rather
/// than a concrete type, but it participates in the reactive graph exactly like any
/// other view via [`ViewElement`].
impl View for AnyView {
    fn entity_id(&self) -> Option<EntityId> {
        Some(self.entity.entity_id())
    }

    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        (self.render)(&self, window, cx)
    }
}

impl<V: 'static + Render> IntoElement for Entity<V> {
    type Element = ViewElement<Entity<V>>;

    fn into_element(self) -> Self::Element {
        ViewElement::new(self)
    }
}

impl IntoElement for AnyView {
    type Element = ViewElement<AnyView>;

    fn into_element(self) -> Self::Element {
        ViewElement::new(self)
    }
}

/// A weak, dynamically-typed view handle.
pub struct AnyWeakView {
    entity: AnyWeakEntity,
    render: fn(&AnyView, &mut Window, &mut App) -> AnyElement,
}

impl AnyWeakView {
    /// Upgrade to a strong `AnyView` handle, if the view is still alive.
    pub fn upgrade(&self) -> Option<AnyView> {
        let entity = self.entity.upgrade()?;
        Some(AnyView {
            entity,
            render: self.render,
        })
    }
}

impl<V: 'static + Render> From<WeakEntity<V>> for AnyWeakView {
    fn from(view: WeakEntity<V>) -> Self {
        AnyWeakView {
            entity: view.into(),
            render: any_view::render::<V>,
        }
    }
}

impl PartialEq for AnyWeakView {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl std::fmt::Debug for AnyWeakView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnyWeakView")
            .field("entity_id", &self.entity.entity_id)
            .finish_non_exhaustive()
    }
}

mod any_view {
    use crate::{AnyElement, AnyView, App, IntoElement, Render, Window};

    pub(crate) fn render<V: 'static + Render>(
        view: &AnyView,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let view = view.clone().downcast::<V>().unwrap();
        // Record the view's Render type name so the accessibility debug dump can
        // attribute nodes to the view that produced them.
        #[cfg(debug_assertions)]
        window
            .a11y
            .view_type_names
            .insert(view.entity_id(), std::any::type_name::<V>());
        view.update(cx, |view, cx| view.render(window, cx).into_any_element())
    }
}

/// A renderable that participates in GPUI's reactive graph — the unifying model
/// behind [`Render`] and [`RenderOnce`].
///
/// When `entity_id()` returns `Some`, that id becomes the view's identity: it gets
/// a unique element-id space (so internal `use_state` / `.id(..)` never collide
/// across siblings) and `cx.notify()` on that entity re-renders only this view's
/// subtree. `None` behaves like a stateless component.
///
/// You rarely implement `View` directly. `Entity<T: Render>` and any `T: RenderOnce`
/// get a blanket impl below; implement it by hand only when a component needs both
/// parent-supplied props *and* a backing entity for identity.
pub trait View: 'static + Sized {
    /// This view's identity, if it has one. A view typically holds the backing
    /// entity as a field and returns its [`EntityId`] here.
    ///
    /// The id becomes this view's [`ElementId`], so two views keyed on the same
    /// entity must not be rendered at the same position in the element tree
    /// (e.g. as siblings under the same parent): their internal element state
    /// (`use_state`, scroll offsets, etc.) would silently collide. Nesting is
    /// fine — the id is scoped by the parent path.
    fn entity_id(&self) -> Option<EntityId>;

    /// Render this view into an element tree, consuming `self`.
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement;
}

/// A stateless component (`RenderOnce`) is a `View` with no identity.
impl<T: RenderOnce> View for T {
    fn entity_id(&self) -> Option<EntityId> {
        None
    }

    #[inline]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        RenderOnce::render(self, window, cx)
    }
}

/// An entity that renders itself (`Render`) is a `View` keyed on its own id.
impl<T: Render> View for Entity<T> {
    fn entity_id(&self) -> Option<EntityId> {
        Some(Entity::entity_id(self))
    }

    #[inline]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.update(cx, |this, cx| {
            Render::render(this, window, cx).into_any_element()
        })
    }
}

impl<T: Render> Entity<T> {
    /// Embed this entity as a cached [`ViewElement`] laid out at `style`.
    ///
    /// The rendered subtree is reused until the entity is notified (or the
    /// cached bounds / text style change). Caching requires a definite size:
    /// a cached view is laid out from `style` and is *not* measured from its
    /// contents. Use [`ViewElement::new`] (or `.child(entity)`) for the
    /// uncached case.
    #[track_caller]
    pub fn cached(self, style: StyleRefinement) -> ViewElement<Entity<T>> {
        ViewElement::new(self).cached(style)
    }
}

/// The element type for [`View`] implementations. Wraps a `View` and hooks it
/// into layout, prepaint, and paint. Constructed via [`ViewElement::new`].
#[doc(hidden)]
pub struct ViewElement<V: View> {
    view: Option<V>,
    entity_id: Option<EntityId>,
    cached_style: Option<StyleRefinement>,
    #[cfg(debug_assertions)]
    source: &'static core::panic::Location<'static>,
}

impl<V: View> ViewElement<V> {
    /// Wrap a [`View`] as an element.
    #[track_caller]
    pub fn new(view: V) -> Self {
        let entity_id = view.entity_id();
        ViewElement {
            entity_id,
            cached_style: None,
            view: Some(view),
            #[cfg(debug_assertions)]
            source: core::panic::Location::caller(),
        }
    }

    /// Enable caching of this view's rendered subtree, laid out at `style`.
    /// The composer supplies the layout style because caching skips rendering
    /// the contents to measure them.
    ///
    /// Crate-private on purpose: caching is only sound for entity-backed views,
    /// where [`Context::notify`] is the contract that busts the cache. A stateless
    /// view has no such contract, so a frozen subtree could never be invalidated.
    /// Reach this through [`Entity::cached`] or [`AnyView::cached`], which are
    /// entity-backed by construction.
    pub(crate) fn cached(mut self, style: StyleRefinement) -> Self {
        self.cached_style = Some(style);
        self
    }
}

impl<V: View> IntoElement for ViewElement<V> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[derive(Default)]
struct ViewElementState {
    layout: Option<ViewElementLayoutState>,
    prepaint_range: Range<PrepaintStateIndex>,
    paint_range: Range<PaintIndex>,
    cache_key: Option<ViewElementCacheKey>,
    accessed_entities: FxHashSet<EntityId>,
}

struct ViewElementLayoutState {
    layout_id: LayoutId,
    layout_generation: u64,
    rem_size: Pixels,
    scale_factor: f32,
    text_style: TextStyle,
    image_cache_id: Option<EntityId>,
}

struct ViewElementCacheKey {
    bounds: Bounds<Pixels>,
    content_mask: ContentMask<Pixels>,
    text_style: TextStyle,
    opacity: f32,
    image_cache_id: Option<EntityId>,
}

impl<V: View> Element for ViewElement<V> {
    type RequestLayoutState = Option<AnyElement>;
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        self.entity_id.map(ElementId::View)
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        #[cfg(debug_assertions)]
        return Some(self.source);

        #[cfg(not(debug_assertions))]
        return None;
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        if let Some(entity_id) = self.entity_id {
            // Stateful path: create a reactive boundary.
            window.with_rendered_view(entity_id, |window| {
                let caching_disabled = window.is_inspector_picking(cx);
                match self.cached_style.as_ref() {
                    Some(style) if !caching_disabled => {
                        let mut root_style = Style::default();
                        root_style.refine(style);
                        let layout_id = window.request_layout(root_style, None, cx);
                        (layout_id, None)
                    }
                    _ => {
                        if let Some(global_id) = global_id {
                            let rem_size = window.rem_size();
                            let scale_factor = window.scale_factor();
                            let text_style = window.text_style();
                            let image_cache_id = window.image_cache_id();
                            let layout_generation = window.layout_generation();
                            return window.with_element_state::<ViewElementState, _>(
                                global_id,
                                |element_state, window| {
                                    let mut element_state = element_state.unwrap_or_default();
                                    if let Some(layout_state) = element_state.layout.as_ref()
                                        && layout_state.layout_generation == layout_generation
                                        && layout_state.rem_size == rem_size
                                        && layout_state.scale_factor == scale_factor
                                        && layout_state.text_style == text_style
                                        && layout_state.image_cache_id == image_cache_id
                                        && !window.dirty_views.contains(&entity_id)
                                        && !window.refreshing
                                        && !caching_disabled
                                    {
                                        window.reuse_layout(layout_state.layout_id);
                                        let layout_id = layout_state.layout_id;
                                        return ((layout_id, None), element_state);
                                    }

                                    let ((layout_id, element), accessed_entities) = cx
                                        .detect_accessed_entities(|cx| {
                                            let mut element = self
                                                .view
                                                .take()
                                                .unwrap()
                                                .render(window, cx)
                                                .into_any_element();
                                            let layout_id = element.request_layout(window, cx);
                                            (layout_id, element)
                                        });
                                    element_state.layout = Some(ViewElementLayoutState {
                                        layout_id,
                                        layout_generation,
                                        rem_size,
                                        scale_factor,
                                        text_style,
                                        image_cache_id,
                                    });
                                    element_state.accessed_entities = accessed_entities;
                                    ((layout_id, Some(element)), element_state)
                                },
                            );
                        }

                        let mut element = self
                            .view
                            .take()
                            .unwrap()
                            .render(window, cx)
                            .into_any_element();
                        let layout_id = element.request_layout(window, cx);
                        (layout_id, Some(element))
                    }
                }
            })
        } else {
            // Stateless path: isolate subtree via type name (no entity identity).
            window.with_id(
                ElementId::Name(std::any::type_name::<V>().into()),
                |window| {
                    let mut element = self
                        .view
                        .take()
                        .unwrap()
                        .render(window, cx)
                        .into_any_element();
                    let layout_id = element.request_layout(window, cx);
                    (layout_id, Some(element))
                },
            )
        }
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        if let Some(entity_id) = self.entity_id {
            // Stateful path.
            window.set_view_id(entity_id);
            window.with_rendered_view(entity_id, |window| {
                if let Some(mut element) = element.take() {
                    let prepaint_start = window.prepaint_index();
                    element.prepaint(window, cx);
                    let prepaint_end = window.prepaint_index();
                    let content_mask = window.content_mask();
                    let text_style = window.text_style();
                    let opacity = window.element_opacity();
                    let image_cache_id = window.image_cache_id();
                    window.with_element_state::<ViewElementState, _>(
                        global_id.unwrap(),
                        |element_state, _| {
                            let mut element_state = element_state.unwrap_or_default();
                            element_state.prepaint_range = prepaint_start..prepaint_end;
                            element_state.paint_range =
                                PaintIndex::default()..PaintIndex::default();
                            element_state.cache_key = Some(ViewElementCacheKey {
                                bounds,
                                content_mask,
                                text_style,
                                opacity,
                                image_cache_id,
                            });
                            ((), element_state)
                        },
                    );
                    return Some(element);
                }

                window.with_element_state::<ViewElementState, _>(
                    global_id.unwrap(),
                    |mut element_state, window| {
                        let content_mask = window.content_mask();
                        let text_style = window.text_style();
                        let opacity = window.element_opacity();
                        let image_cache_id = window.image_cache_id();

                        if element_state.as_ref().is_some_and(|element_state| {
                            element_state.cache_key.as_ref().is_some_and(|cache_key| {
                                cache_key.bounds == bounds
                                    && cache_key.content_mask == content_mask
                                    && cache_key.text_style == text_style
                                    && cache_key.opacity == opacity
                                    && cache_key.image_cache_id == image_cache_id
                            }) && !window.dirty_views.contains(&entity_id)
                                && !window.refreshing
                        }) {
                            if let Some(mut element_state) = element_state.take() {
                                let prepaint_start = window.prepaint_index();
                                window.reuse_prepaint(element_state.prepaint_range.clone());
                                cx.entities
                                    .extend_accessed(&element_state.accessed_entities);
                                let prepaint_end = window.prepaint_index();
                                element_state.prepaint_range = prepaint_start..prepaint_end;

                                return (None, element_state);
                            }
                        }

                        let refreshing = mem::replace(&mut window.refreshing, true);
                        let prepaint_start = window.prepaint_index();
                        let (mut element, accessed_entities) = cx.detect_accessed_entities(|cx| {
                            let mut element = self
                                .view
                                .take()
                                .unwrap()
                                .render(window, cx)
                                .into_any_element();
                            element.layout_as_root(bounds.size.into(), window, cx);
                            element.prepaint_at(bounds.origin, window, cx);
                            element
                        });

                        let prepaint_end = window.prepaint_index();
                        window.refreshing = refreshing;

                        let mut element_state = element_state.unwrap_or_default();
                        element_state.accessed_entities = accessed_entities;
                        element_state.prepaint_range = prepaint_start..prepaint_end;
                        element_state.paint_range = PaintIndex::default()..PaintIndex::default();
                        element_state.cache_key = Some(ViewElementCacheKey {
                            bounds,
                            content_mask,
                            text_style,
                            opacity,
                            image_cache_id,
                        });

                        (Some(element), element_state)
                    },
                )
            })
        } else {
            // Stateless path: just prepaint the element.
            window.with_id(
                ElementId::Name(std::any::type_name::<V>().into()),
                |window| {
                    element.as_mut().unwrap().prepaint(window, cx);
                },
            );
            Some(element.take().unwrap())
        }
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        element: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(entity_id) = self.entity_id {
            // Stateful path.
            window.with_rendered_view(entity_id, |window| {
                window.with_element_state::<ViewElementState, _>(
                    global_id.unwrap(),
                    |element_state, window| {
                        let mut element_state = element_state.unwrap_or_default();

                        let paint_start = window.paint_index();

                        if let Some(element) = element {
                            let refreshing = mem::replace(&mut window.refreshing, true);
                            element.paint(window, cx);
                            window.refreshing = refreshing;
                        } else {
                            window.reuse_paint(element_state.paint_range.clone());
                        }

                        let paint_end = window.paint_index();
                        element_state.paint_range = paint_start..paint_end;

                        ((), element_state)
                    },
                )
            });
        } else {
            // Stateless path: just paint the element.
            window.with_id(
                ElementId::Name(std::any::type_name::<V>().into()),
                |window| {
                    element.as_mut().unwrap().paint(window, cx);
                },
            );
        }
    }
}

/// A view that renders nothing
pub struct EmptyView;

impl Render for EmptyView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppContext as _, TestAppContext, div, prelude::*, px, size};
    use std::{cell::Cell, rc::Rc};

    struct CountedView {
        render_count: Rc<Cell<usize>>,
    }

    impl Render for CountedView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.render_count.set(self.render_count.get() + 1);
            div().size(px(20.))
        }
    }

    struct ChangingView {
        wide: bool,
    }

    impl Render for ChangingView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .h(px(20.))
                .w(if self.wide { px(40.) } else { px(20.) })
        }
    }

    struct SiblingRoot {
        counted: Entity<CountedView>,
        changing: Entity<ChangingView>,
    }

    impl Render for SiblingRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .child(self.counted.clone())
                .child(self.changing.clone())
        }
    }

    #[gpui::test]
    fn clean_sibling_reuses_its_rendered_layout(cx: &mut TestAppContext) {
        let render_count = Rc::new(Cell::new(0));
        let window = cx.open_window(size(px(200.), px(100.)), {
            let render_count = render_count.clone();
            |_, cx| SiblingRoot {
                counted: cx.new(|_| CountedView { render_count }),
                changing: cx.new(|_| ChangingView { wide: false }),
            }
        });
        cx.run_until_parked();
        let initial_render_count = render_count.get();

        window
            .update(cx, |root, _, cx| {
                root.changing.update(cx, |changing, cx| {
                    changing.wide = true;
                    cx.notify();
                });
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(render_count.get(), initial_render_count);

        window
            .update(cx, |root, _, cx| {
                root.changing.update(cx, |changing, cx| {
                    changing.wide = false;
                    cx.notify();
                });
            })
            .unwrap();
        cx.run_until_parked();

        assert_eq!(render_count.get(), initial_render_count);
    }

    struct OpacityRoot {
        child: Entity<CountedView>,
        opacity: f32,
    }

    impl Render for OpacityRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().opacity(self.opacity).child(self.child.clone())
        }
    }

    #[gpui::test]
    fn inherited_opacity_invalidates_cached_paint(cx: &mut TestAppContext) {
        let render_count = Rc::new(Cell::new(0));
        let window = cx.open_window(size(px(200.), px(100.)), {
            let render_count = render_count.clone();
            |_, cx| OpacityRoot {
                child: cx.new(|_| CountedView { render_count }),
                opacity: 1.,
            }
        });
        cx.run_until_parked();
        let initial_render_count = render_count.get();

        window
            .update(cx, |root, _, cx| {
                root.opacity = 0.5;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();

        assert_eq!(render_count.get(), initial_render_count + 1);
    }

    struct MovableChild;

    impl Render for MovableChild {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size(px(20.))
                .debug_selector(|| "MOVABLE_CHILD".into())
        }
    }

    struct MovingRoot {
        child: Entity<MovableChild>,
        child_on_right: bool,
    }

    impl Render for MovingRoot {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .when(!self.child_on_right, |element| {
                    element.child(self.child.clone())
                })
                .child(div().w(px(100.)).h(px(20.)))
                .when(self.child_on_right, |element| {
                    element.child(self.child.clone())
                })
        }
    }

    #[gpui::test]
    fn reused_layout_can_move_to_a_new_parent_position(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(200.), px(100.)), |_, cx| MovingRoot {
            child: cx.new(|_| MovableChild),
            child_on_right: false,
        });
        cx.run_until_parked();

        let initial_x = window
            .update(cx, |_, window, _| {
                window.rendered_frame.debug_bounds["MOVABLE_CHILD"].origin.x
            })
            .unwrap();
        window
            .update(cx, |root, _, cx| {
                root.child_on_right = true;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        let moved_x = window
            .update(cx, |_, window, _| {
                window.rendered_frame.debug_bounds["MOVABLE_CHILD"].origin.x
            })
            .unwrap();

        assert_eq!(moved_x - initial_x, px(100.));
    }

    #[gpui::test]
    fn discarded_layout_subtrees_are_reclaimed(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(200.), px(100.)), |_, cx| MovingRoot {
            child: cx.new(|_| MovableChild),
            child_on_right: false,
        });
        cx.run_until_parked();
        window
            .update(cx, |root, _, cx| {
                root.child_on_right = true;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        let initial_node_count = window
            .update(cx, |_, window, _| window.layout_node_count())
            .unwrap();

        for child_on_right in [false, true].into_iter().cycle().take(20) {
            window
                .update(cx, |root, _, cx| {
                    root.child_on_right = child_on_right;
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
        }

        window
            .update(cx, |_, window, _| {
                assert_eq!(window.layout_node_count(), initial_node_count);
            })
            .unwrap();
    }
}
