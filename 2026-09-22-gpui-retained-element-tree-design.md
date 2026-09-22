# GPUI Retained Element Tree 设计文档

**状态：** Draft for implementation
**目标读者：** GPUI 核心开发者
**目标：** 在保留 GPUI 当前 `Render` / `RenderOnce` 开发方式的前提下，引入持久化 Element Tree、增量布局、增量 prepaint、增量 paint 与 transform-only 更新，显著降低滚动和动画期间的 CPU 开销。

---

## 1. 背景

GPUI 当前采用 hybrid immediate + retained rendering model。

应用通过 `Render::render()` 或 `RenderOnce::render()` 在每个需要绘制的 frame 中生成新的 Element Tree。Element 随后依次经过：

```text
View::render
    ↓
Element::request_layout
    ↓
Taffy compute_layout
    ↓
Element::prepaint
    ↓
Element::paint
    ↓
Scene submission
```

该模型的优势是：

- API 简单，View 可以直接根据最新状态生成 UI；
- Element 生命周期短，不需要维护复杂的节点状态；
- 单帧吞吐非常高；
- 很适合编辑器、图表以及高度自定义的 GPU 绘制。

但持续滚动或动画时，每个 frame 都会产生以下 CPU 工作：

- 从 root view 开始重新执行相关 `render()`；
- 重新创建 Elements；
- 重新创建 Taffy nodes；
- 重新进入 layout、prepaint 和 paint；
- 重新生成 hitboxes、dispatch tree 和 Scene primitives。

即使一帧可以在 8ms 内完成，这些重复工作依然会造成较高的 CPU 占用和功耗。

GPUI 已有 `Entity::cached`，能够复用上一帧的 prepaint/paint ranges、Scene primitives、hitboxes、listeners、dispatch subtree 和文字布局。但它需要手动启用、要求外部提供确定布局，并且 bounds、clip、文字样式或 dirty 状态变化会导致整个缓存失效。它更接近粗粒度 subtree snapshot，还不能自动完成 Element 级 reconciliation。

本设计引入持久化的 Retained Element Tree，使 GPUI 能够在每帧生成新的 Element 描述后，与上一帧节点进行匹配，只更新真正发生变化的阶段。

---

## 2. 设计目标

### 2.1 核心目标

1. 保留当前 immediate-style `Render` API。
2. View 可以继续在状态变化后重新执行 `render()`。
3. 新一帧生成的 Elements 与持久节点进行 reconciliation。
4. 未变化节点不重复执行 layout、prepaint 和 paint。
5. 滚动等纯位置变化只更新 transform 和 clip。
6. 持久化 Taffy nodes，不再每帧清空整个 layout tree。
7. 现有 `Entity::cached` 迁移为 retained subtree isolation boundary。
8. 自定义 Element 在没有实现增量能力时保持完全正确。
9. 所有优化都可以安全回退到现有全量路径。

### 2.2 性能目标

在复杂窗口持续滚动 benchmark 中：

- View/Element 构建次数显著减少或其结果可被快速 reconcile；
- 未变化 Element 的 `request_layout` 调用减少 80% 以上；
- 未变化 Element 的 `prepaint` / `paint` 调用减少 90% 以上；
- median frame CPU time 至少降低 30%；
- p95 input latency 不增加超过 5%；
- retained tree 额外内存不超过当前 frame structures 的 25%；
- debug/inspector 模式允许退化为全量更新。

### 2.3 非目标

第一阶段不实现：

- React Concurrent Mode；
- 可中断 reconciliation；
- task priority scheduler；
- Slint/QML 风格 property dependency graph；
- 自动跳过所有 `View::render()`；
- GPU renderer 的通用 partial present；
- 多线程 layout 或 paint；
- 将 GPUI API 改造成声明式 DSL。

本设计解决的是 persistent identity、damage tracking 和阶段复用，不是完整复制 React Fiber。

---

## 3. 核心原则

### 3.1 View 继续 immediate，Element 执行结果 retained

第一版仍允许 dirty View 重新执行：

```rust
impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .child(self.toolbar.clone())
            .child(self.list.clone())
    }
}
```

不同之处是，返回的临时 Element 不再直接代表完整的新 frame tree，而是作为本帧的 Element description，与 `Window::retained_tree` 中的持久节点进行 reconcile。

### 3.2 默认保证正确，增量优化必须显式可证明

所有现有自定义 Element 默认返回：

```rust
Damage::LAYOUT | Damage::PREPAINT | Damage::PAINT
```

只有 GPUI 内建 Element 或显式实现 retained diff 的 Element 才能获得更细粒度复用。

### 3.3 不跨帧保留 arena-owned `AnyElement`

当前 Element 可能包含：

- `FnOnce`；
- frame arena 引用；
- 当帧 request-layout state；
- 当帧 prepaint state。

因此 Retained Tree 不直接保存当前 `AnyElement`。它只保存：

- 稳定身份；
- 可比较属性快照；
- persistent layout node；
- prepaint snapshot；
- paint snapshot；
- handler slots；
- children relationships。

### 3.4 阶段独立失效

颜色变化不能导致 layout；位置变化不能导致 repaint；handler callback 更新不能导致 hitbox 重建。

---

## 4. 总体架构

```text
                         ┌─────────────────────┐
                         │     View State      │
                         └──────────┬──────────┘
                                    │ cx.notify()
                                    ▼
                         ┌─────────────────────┐
                         │    View::render     │
                         │ transient Elements  │
                         └──────────┬──────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────┐
│                    Reconciler                           │
│ identity match → property diff → damage propagation     │
└──────────┬──────────────────┬──────────────────┬─────────┘
           │                  │                  │
           ▼                  ▼                  ▼
  Persistent Taffy     Prepaint Snapshot    Paint Snapshot
      Layout Tree       / Dispatch Tree       / Scene Layer
           │                  │                  │
           └──────────────────┴──────────────────┘
                              │
                              ▼
                       Composed Frame
```

每个 Window 拥有一棵：

```rust
pub(crate) struct RetainedElementTree {
    nodes: SlotMap<RetainedNodeId, RetainedNode>,
    root: Option<RetainedNodeId>,
    frame_generation: u64,
    reconcile_stack: Vec<ReconcileFrame>,
    pending_removals: Vec<RetainedNodeId>,
}
```

Tree 与 Window 生命周期一致。窗口关闭时整体释放。

---

## 5. 节点身份模型

### 5.1 RetainedNodeId

```rust
slotmap::new_key_type! {
    pub(crate) struct RetainedNodeId;
}
```

`RetainedNodeId` 是 Window 内部稳定句柄，不对公共 API 暴露。

### 5.2 ReconcileKey

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ReconcileKey {
    Explicit(ElementId),
    View(EntityId),
    Positional {
        element_type: TypeId,
        slot: u32,
    },
}
```

匹配优先级：

1. `ElementId`；
2. Entity-backed View 的 `EntityId`；
3. 同一 parent 下的 `TypeId + sibling slot`。

显式 key 在同一个 parent 下必须唯一。发现重复 key 时：

- debug build panic，并报告完整 element path；
- release build 将第二个重复节点退化为 positional identity，并记录一次 warning。

### 5.3 匹配规则

新节点只能与满足以下条件的旧节点复用：

```text
same parent
AND same ReconcileKey
AND same Element TypeId
```

key 相同但类型变化时，旧 subtree 被删除并创建新 subtree。

无 key children 发生插入或 reorder 时，其后续 positional siblings 允许重新创建。需要稳定 reorder 的列表必须提供 `ElementId`。

---

## 6. RetainedNode 数据结构

```rust
pub(crate) struct RetainedNode {
    // Identity
    id: RetainedNodeId,
    key: ReconcileKey,
    element_type: TypeId,
    parent: Option<RetainedNodeId>,
    children: SmallVec<[RetainedNodeId; 4]>,

    // Ownership
    owner_view: Option<EntityId>,
    last_seen_generation: u64,

    // Diff state
    properties: Box<dyn Any>,
    property_fingerprint: u64,
    damage: Damage,

    // Layout
    layout_id: LayoutId,
    layout_constraints: Option<LayoutConstraints>,
    local_bounds: Bounds<Pixels>,
    absolute_transform: ElementTransform,

    // Environment inherited from ancestors
    environment: InheritedEnvironment,

    // Reusable frame output
    prepaint_snapshot: Option<PrepaintSnapshot>,
    paint_snapshot: Option<PaintSnapshot>,

    // Updated without rebuilding geometry
    handlers: HandlerTable,

    // Debug/profiling
    source_location: Option<&'static Location<'static>>,
    stats: NodeStats,
}
```

### 6.1 InheritedEnvironment

```rust
#[derive(Clone, PartialEq)]
pub(crate) struct InheritedEnvironment {
    text_style: TextStyle,
    rem_size: Pixels,
    scale_factor: f32,
    opacity: f32,
    content_mask: ContentMask<Pixels>,
    window_active: bool,
}
```

不是所有环境字段都具有相同影响。diff 时转换成对应 damage：

| 变化 | Damage |
|---|---|
| `rem_size` | `LAYOUT | PREPAINT | PAINT` |
| `scale_factor` | `LAYOUT | PREPAINT | PAINT` |
| 字体、字号、line-height | `LAYOUT | PREPAINT | PAINT` |
| 文字颜色 | `PAINT` |
| opacity | `COMPOSITE` |
| absolute clip | `CLIP` |
| window active 状态 | 由 Element dependency 决定，默认 `PAINT` |

---

## 7. Damage 模型

```rust
bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct Damage: u16 {
        const NONE        = 0;
        const BUILD       = 1 << 0;
        const CHILDREN    = 1 << 1;
        const LAYOUT      = 1 << 2;
        const PREPAINT    = 1 << 3;
        const HANDLERS    = 1 << 4;
        const PAINT       = 1 << 5;
        const TRANSFORM   = 1 << 6;
        const CLIP        = 1 << 7;
        const COMPOSITE   = 1 << 8;

        const FULL = Self::BUILD.bits()
            | Self::CHILDREN.bits()
            | Self::LAYOUT.bits()
            | Self::PREPAINT.bits()
            | Self::HANDLERS.bits()
            | Self::PAINT.bits();
    }
}
```

### 7.1 Damage 含义

- `BUILD`：需要重新执行 owner View/Component 的构建逻辑。
- `CHILDREN`：children identity/order 发生变化。
- `LAYOUT`：style、intrinsic measurement、children 或 constraints 变化。
- `PREPAINT`：hitbox、focus、tooltip、dispatch metadata 等需要重新生成。
- `HANDLERS`：仅 callback 内容需要替换，geometry 不变。
- `PAINT`：Scene primitives 需要重新生成。
- `TRANSFORM`：local content 不变，只更新 translation/scale/rotation。
- `CLIP`：只更新 ancestor clip composition。
- `COMPOSITE`：只更新 layer opacity/blend/compositor state。

### 7.2 Damage 传播

向上传播：

```text
child LAYOUT
→ parent LAYOUT
→ 直到 layout isolation boundary
```

```text
child CHILDREN
→ parent LAYOUT（如果布局依赖 children）
```

向下传播：

```text
parent inherited text layout style changed
→ descendants LAYOUT | PREPAINT | PAINT
```

```text
parent TRANSFORM
→ 不标记 children damage
→ composition 时组合 transform
```

```text
parent CLIP
→ 不标记 children paint
→ composition 时重新求交集
```

### 7.3 隔离边界

```rust
bitflags::bitflags! {
    pub(crate) struct Isolation: u8 {
        const LAYOUT = 1 << 0;
        const PAINT = 1 << 1;
        const TRANSFORM = 1 << 2;
    }
}
```

固定尺寸、内容不会影响父布局的 subtree 可以成为 `LAYOUT` isolation boundary。现有 `Entity::cached(style)` 天然是强隔离边界。

---

## 8. Element retained 接口

### 8.1 Reconciliation 的实际接入点

当前 `Element` trait 没有统一的 children enumeration API。`Div`、List 和自定义 Element 通常在各自的 `request_layout()` 中递归调用 child 的 `request_layout()`。因此第一版不能假设 GPUI 能在 layout 之前完整遍历临时 Element Tree。

Reconciliation 必须嵌入现有 `AnyElement::request_layout()` traversal：

```rust
impl AnyElement {
    fn request_layout(&mut self, window: &mut Window, cx: &mut App) -> LayoutId {
        let token = window.retained_tree.begin_element(
            self.reconcile_key(),
            self.type_id(),
            self.retained_properties(),
        );

        let layout_id = if token.can_reuse_layout() {
            token.layout_id()
        } else {
            self.drawable.request_layout(window, cx)
        };

        window.retained_tree.end_element(token, layout_id);
        layout_id
    }
}
```

父 Element 调用 children 的 `request_layout()` 时，Reconciler 通过 stack 知道 parent 和 sibling slot：

```rust
struct ReconcileFrame {
    node: RetainedNodeId,
    next_child_slot: u32,
    matched_children: FxHashSet<RetainedNodeId>,
}
```

这带来两个兼容级别：

1. Legacy parent：仍执行自身 `request_layout()` 并遍历 children；每个 child 在进入 `AnyElement::request_layout()` 时完成 identity reconciliation。行为完全兼容，但无法提前跳过整个 parent traversal。
2. Retainable parent：提供 children keys/properties fingerprint。fingerprint 未变化且 layout clean 时，可以直接返回 persistent `LayoutId`，完全跳过 children traversal。

GPUI 内建容器逐步升级到第二级；第三方自定义 Element 无需立刻修改。

### 8.2 兼容现有 Element trait

不直接破坏现有 `Element` trait。新增内部扩展 trait：

```rust
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
}
```

通过 erased vtable 保存：

```rust
pub(crate) struct RetainedElementVTable {
    type_id: TypeId,
    properties: fn(&dyn Any) -> Box<dyn Any>,
    diff: fn(&dyn Any, &dyn Any, &EnvironmentDiff) -> Damage,
    isolation: fn(&dyn Any) -> Isolation,
}
```

### 8.3 Parent retained contract

内建 Parent Elements 额外实现：

```rust
pub(crate) trait RetainableParentElement: RetainableElement + ParentElement {
    fn child_descriptors(&self) -> SmallVec<[ChildDescriptor; 4]>;
}

pub(crate) struct ChildDescriptor {
    key: ReconcileKey,
    element_type: TypeId,
    property_fingerprint: u64,
}
```

`child_descriptors()` 只返回匹配和快速 diff 所需的轻量数据，不移动或执行 child。只有 descriptors、parent layout properties 和 constraints 均未变化时，才允许跳过 children 的 layout traversal。

### 8.4 默认实现

没有实现 `RetainableElement` 的 Element 使用 conservative adapter：

```rust
Damage::LAYOUT
    | Damage::PREPAINT
    | Damage::HANDLERS
    | Damage::PAINT
```

它仍参与 identity reconciliation，但不会跳过执行阶段。

### 8.5 内建 Elements 的 properties

以 `Div` 为例：

```rust
struct DivRetainedProperties {
    style: Style,
    interactivity: InteractivityProperties,
    child_keys: SmallVec<[ReconcileKey; 4]>,
}
```

diff 规则：

```rust
if layout_style_changed {
    damage |= Damage::LAYOUT | Damage::PREPAINT | Damage::PAINT;
}

if visual_style_changed {
    damage |= Damage::PAINT;
}

if hitbox_style_changed {
    damage |= Damage::PREPAINT;
}

if handlers_changed {
    damage |= Damage::HANDLERS;
}

if child_keys_changed {
    damage |= Damage::CHILDREN;
}
```

Style 必须按影响拆分 fingerprint：

```rust
struct StyleFingerprint {
    layout: u64,
    prepaint: u64,
    paint: u64,
    composite: u64,
}
```

不能只对完整 `Style` 做一个 hash，否则颜色变化仍会触发 layout。

---

## 9. Reconciliation 算法

### 9.1 输入

每个 dirty View 的 `render()` 产生临时 root `AnyElement`。Reconciler 从 root 的 `AnyElement::request_layout()` 开始，随现有 layout traversal 逐节点进入：

```rust
fn reconcile_view(
    owner_view: EntityId,
    element: AnyElement,
    previous_root: Option<RetainedNodeId>,
    window: &mut Window,
    cx: &mut App,
) -> RetainedNodeId;
```

该函数不会要求提前取出整棵临时 tree。它安装 reconciliation context，然后调用 root `request_layout()`；每次 child 进入 `AnyElement::request_layout()` 时，通过 stack 完成匹配。

### 9.2 子节点匹配

```rust
fn reconcile_known_children(
    parent: RetainedNodeId,
    new_children: &[ChildDescriptor],
    tree: &mut RetainedElementTree,
) {
    let old_children = tree[parent].children.clone();
    let mut keyed = FxHashMap::default();

    for child in &old_children {
        if tree[*child].key.is_explicit() {
            keyed.insert(tree[*child].key.clone(), *child);
        }
    }

    let mut next_children = SmallVec::new();

    for (slot, child) in new_children.iter().enumerate() {
        let key = child.key.clone();

        let matched = match &key {
            ReconcileKey::Explicit(_) | ReconcileKey::View(_) => {
                keyed.remove(&key)
            }
            ReconcileKey::Positional { element_type, slot } => {
                old_children
                    .get(*slot as usize)
                    .copied()
                    .filter(|node| tree[*node].element_type == *element_type)
            }
        };

        let node = reconcile_descriptor(parent, matched, child, tree);
        next_children.push(node);
    }

    for old_child in old_children {
        if !next_children.contains(&old_child) {
            tree.schedule_remove(old_child);
        }
    }

    tree[parent].children = next_children;
}
```

该快速路径只用于 `RetainableParentElement`。Legacy parent 不调用 `reconcile_known_children()`；它执行原来的 `request_layout()`，children 在实际递归进入时逐个 reconcile。这样不会要求第三方 Element 暴露内部 children。

### 9.3 节点更新

```rust
fn reconcile_descriptor(
    parent: RetainedNodeId,
    previous: Option<RetainedNodeId>,
    current: &ChildDescriptor,
    tree: &mut RetainedElementTree,
) -> RetainedNodeId {
    let node = match previous {
        Some(node) if tree[node].element_type == current.type_id() => node,
        Some(node) => {
            tree.schedule_remove(node);
            tree.insert_descriptor(parent, current)
        }
        None => tree.insert_descriptor(parent, current),
    };

    if tree[node].property_fingerprint != current.property_fingerprint {
        tree[node].damage |= Damage::LAYOUT | Damage::PREPAINT | Damage::PAINT;
    }
    tree[node].property_fingerprint = current.property_fingerprint;
    tree[node].last_seen_generation = tree.frame_generation;

    node
}
```

### 9.4 删除时机

旧节点不能在 reconcile 中立即递归释放，因为当前 frame 的 snapshot replay 可能仍然引用上一帧数据。

使用两阶段回收：

1. reconcile 时加入 `pending_removals`；
2. 新 frame 完成并替换 `rendered_frame` 后释放 retained node；
3. renderer/GPU resource 按现有 frame fence 生命周期释放。

---

## 10. View invalidation

### 10.1 cx.notify()

第一版保持现有语义：

```text
cx.notify(entity)
→ 对应 View node BUILD dirty
→ 标记 ancestor View 路径需要进入 reconciliation
→ 下一帧重新执行该 View::render()
```

但 ancestor View 不一定重新生成所有 descendants。遇到未 dirty 的 Entity-backed View 时，可以直接引用其 retained root。

### 10.2 Entity dependency

沿用当前 `detect_accessed_entities`：

- 每个 View node 记录 render 时访问过的 Entities；
- 任意被访问 Entity notify 时，owner View `BUILD` dirty；
- clean child View 不因 dirty parent 自动失效；
- 全局 refresh 才使整棵树 `FULL` dirty。

### 10.3 第二阶段优化

未来可以把依赖记录到具体 retained node，而不是 View：

```text
Entity change
→ only nodes that accessed it become dirty
```

这不是第一版要求。

---

## 11. Persistent Taffy Tree

### 11.1 生命周期

删除每帧结束时的：

```rust
self.layout_engine.as_mut().unwrap().clear();
```

每个 `RetainedNode` 持有稳定 `LayoutId`。

```rust
struct RetainedLayoutState {
    layout_id: LayoutId,
    style_fingerprint: u64,
    children_fingerprint: u64,
    last_constraints: Option<Size<AvailableSpace>>,
}
```

### 11.2 更新规则

```rust
if style_changed {
    taffy.set_style(layout_id, new_style);
}

if children_changed {
    taffy.set_children(layout_id, child_layout_ids);
}

if measure_function_changed {
    taffy.set_node_context(layout_id, new_measure_context);
}
```

### 11.3 Compute roots

不对每个 dirty node 单独 compute。收集最小 dirty roots：

```rust
fn collect_layout_roots(tree: &RetainedElementTree) -> Vec<RetainedNodeId> {
    tree.nodes_with(Damage::LAYOUT)
        .filter(|node| {
            tree[node].parent.is_none_or(|parent| {
                !tree[parent].damage.contains(Damage::LAYOUT)
                    || tree[parent].isolation.contains(Isolation::LAYOUT)
            })
        })
        .collect()
}
```

通常窗口 root 仍可能成为 compute root，但 Taffy 内部未 dirty nodes 会复用 layout cache；关键是避免每帧重建所有 Taffy nodes。

### 11.4 Measured nodes

文本、图片和自定义 measured Elements 的 measure closure 不能引用 frame arena。

改为持久 measure object：

```rust
trait RetainedMeasure: 'static {
    fn measure(
        &mut self,
        known_dimensions: Size<Option<Pixels>>,
        available_space: Size<AvailableSpace>,
        window: &mut Window,
        cx: &mut App,
    ) -> Size<Pixels>;
}
```

内建文字节点保存 `SharedString`、font key、runs 和 wrapping parameters。任一 measurement input 改变时标记 `LAYOUT`。

自定义 Element 如果只能提供 frame-local measure closure，默认每次重建该 layout node。

### 11.5 LayoutId 安全

`LayoutId` 必须增加 generation validation，避免节点释放后旧 ID 指向新节点：

```rust
struct LayoutId {
    node: taffy::NodeId,
    generation: u32,
}
```

如果底层 SlotMap/Generational ID 已提供同等保证，可以直接封装而不重复实现。

---

## 12. Prepaint Snapshot

### 12.1 内容

```rust
pub(crate) struct PrepaintSnapshot {
    local_origin: Point<Pixels>,
    hitboxes: Range<HitboxIndex>,
    tooltip_requests: Range<TooltipIndex>,
    deferred_draws: Range<DeferredDrawIndex>,
    dispatch_subtree: DispatchSubtreeSnapshot,
    accessed_element_states: Range<ElementStateIndex>,
    text_layouts: Range<LineLayoutIndex>,
    tab_stops: Range<TabStopIndex>,
}
```

它是现有 `PrepaintStateIndex` / `reuse_prepaint` 的节点化版本。

### 12.2 Replay

```rust
fn replay_prepaint(
    snapshot: &PrepaintSnapshot,
    transform: ElementTransform,
    clip: ContentMask<Pixels>,
    previous: &Frame,
    next: &mut Frame,
);
```

Replay 必须：

- 复制或引用 hitboxes，并应用 transform；
- refresh dispatch node ids；
- 更新 focus path；
- 保持 tooltip/deferred draw parent relationship；
- 复用 text layouts；
- 使用当前 clip 重新决定可交互范围。

`Range` 始终指向上一帧的 `rendered_frame`，而不是任意历史 Frame。每次 replay 或 rebuild 后，必须记录写入 `next_frame` 的新 range，并用它替换节点 snapshot range：

```text
snapshot range in rendered_frame
→ replay into next_frame
→ capture new range in next_frame
→ frame swap
→ new range now points into rendered_frame
```

因此 RetainedNode 可以跨任意帧存在，但它的 frame-local ranges 每帧都会 rebase。任何 generation 不匹配都必须退化为 rebuild。

### 12.3 Handler slots

事件 callback 经常捕获本帧新数据，不能因为 geometry 未变化就一直复用旧 closure。

引入稳定 handler slot：

```rust
pub(crate) struct HandlerSlotId {
    node: RetainedNodeId,
    kind: HandlerKind,
    ordinal: u16,
}

pub(crate) struct HandlerTable {
    callbacks: SmallVec<[(HandlerSlotId, AnyHandler); 4]>,
}
```

reconcile 时可以替换 callback，而不重新创建 hitbox：

```text
same handler shape + new closure
→ HANDLERS dirty
→ update HandlerTable
→ keep PREPAINT snapshot
```

如果 handler 数量、类型或 target hitbox 改变，则 `PREPAINT` dirty。

---

## 13. Paint Snapshot 与 Scene Layer

### 13.1 目标

现有缓存通过 Scene range replay primitive。Retained Tree 将其提升为每个节点或 isolation subtree 的 `PaintSnapshot`。

```rust
pub(crate) struct PaintSnapshot {
    layer: SceneLayerId,
    primitive_range: Range<PaintIndex>,
    local_bounds: Bounds<Pixels>,
    local_clip: ContentMask<Pixels>,
    text_layouts: Range<LineLayoutIndex>,
    resource_generation: RendererGeneration,
}
```

### 13.2 Scene local coordinates

可复用 subtree 的 primitives 使用 subtree-local coordinates 记录：

```text
absolute_position = ancestor_transform × node_transform × local_position
```

Scene layer：

```rust
pub(crate) struct SceneLayer {
    parent: Option<SceneLayerId>,
    transform: ElementTransform,
    clip: ContentMask<Pixels>,
    opacity: f32,
    primitives: Range<PaintIndex>,
}
```

### 13.3 Replay 条件

满足以下条件时可复用 paint snapshot：

- 没有 `PAINT` damage；
- renderer resource generation 相同；
- glyph/image atlas references 仍有效；
- local visual environment 未变化；
- custom paint Element 声明 snapshot safe。

### 13.4 Device recovery

GPU device recovery 或 atlas generation 整体失效时：

```text
window renderer_generation += 1
→ all PaintSnapshot generation mismatch
→ PAINT dirty
```

不需要删除 Retained Tree 或 persistent layout。

---

## 14. Transform、滚动与 Clip

### 14.1 滚动必须是 transform-only 更新

Scroll container 的 content layout 保持在 local coordinates：

```text
content transform = translate(-scroll_x, -scroll_y)
viewport clip = scroll viewport bounds
```

滚动 offset 改变：

```text
scroll node TRANSFORM | CLIP dirty
children layout unchanged
children prepaint unchanged
children paint unchanged
```

### 14.2 Hit testing

hitbox 不需要重新生成，但查询鼠标位置时需要使用 inverse transform：

```rust
let local_pointer = world_transform.inverse().transform_point(pointer);
```

对于只有 translation 的常见滚动路径，可以提供无矩阵分配的 fast path。

### 14.3 Clip composition

snapshot 保存 local clip，composition 时计算：

```rust
world_clip = parent_world_clip.intersect(
    transform.apply(local_clip)
);
```

clip 改变不使 children `PAINT` dirty。

### 14.4 Virtualized List

Virtualized List 仍负责决定哪些 rows 存在：

- 离开 overscan range 的 rows 从 active children 移除；
- 新进入 rows 创建 retained nodes；
- 仍然可见且 key 相同的 rows 只更新 transform；
- row data revision 改变时只更新对应 row；
- row reorder 通过 item key 移动 retained node。

非虚拟列表仍然会创建所有 retained nodes，降低的是重复计算，不是初始构建和内存开销。

---

## 15. Animation

动画属性必须声明影响阶段：

```rust
pub enum AnimationImpact {
    Layout,
    Paint,
    Transform,
    Composite,
}
```

示例：

| 动画 | Damage |
|---|---|
| width / height / padding | `LAYOUT` |
| background color | `PAINT` |
| translation / rotation / scale | `TRANSFORM` |
| opacity | `COMPOSITE` |

GPUI animation API 应尽量在 retained node 上更新值，而不是每帧 `cx.notify()` 整个 View。

第一版可以继续 notify View，但 reconciliation 会将变化压缩到具体 Element damage。第二版提供：

```rust
window.animate_node(node_id, property, value);
```

这允许 transform/composite animation 完全跳过 `View::render()`。

---

## 16. Entity::cached 迁移

现有 `Entity::cached(style)` 保留公共 API，但内部改为：

```text
Retained View Node
+ layout isolation
+ paint isolation
+ skip View::render when owner Entity is clean
```

其语义：

- Entity clean：直接复用整个 retained subtree；
- Entity dirty：执行 View render 并 reconcile subtree；
- bounds origin 变化：只更新 transform；
- size 变化：layout dirty；
- inherited text layout style 变化：layout/paint dirty；
- clip 变化：只更新 clip composition。

长期来看，普通 Entity-backed View 也可以自动获得大部分能力；`.cached(style)` 继续作为强 isolation 和明确跳过 render 的性能承诺。

---

## 17. 自定义 Element 兼容策略

### 17.1 Level 0：Legacy

无需修改现有代码：

```text
identity retained
layout/prepaint/paint every dirty frame
```

### 17.2 Level 1：Stable properties

实现 `RetainableElement`，可以 diff layout/paint 属性。

### 17.3 Level 2：Snapshot-safe paint

声明 paint output 可以 replay：

```rust
fn paint_retention(&self) -> PaintRetention {
    PaintRetention::SnapshotSafe
}
```

### 17.4 Level 3：Transformable

声明 subtree 可以在 local coordinate layer 中平移、缩放或旋转。

### 17.5 Canvas

`canvas(prepaint, paint)` 使用 `FnOnce`，无法自动判断闭包是否等价。默认：

```text
PREPAINT | PAINT dirty whenever owner View rebuilds
```

新增显式 API：

```rust
canvas(...)
    .retained(id, revision)
```

revision 未变化时允许 snapshot replay。

---

## 18. Accessibility、Focus 与 Input

### 18.1 Accessibility

每个 retained node 保存 accessibility snapshot：

```rust
struct AccessibilitySnapshot {
    node_id: accesskit::NodeId,
    properties_hash: u64,
    children: SmallVec<[accesskit::NodeId; 4]>,
}
```

只有以下变化才发送 node update：

- a11y properties 变化；
- bounds/transform 变化；
- children relationship 变化；
- node 新增或删除。

### 18.2 Focus

Focus identity 绑定稳定 retained node / ElementId，而不是 frame dispatch index。

replay dispatch subtree 时刷新 frame-local dispatch node id，但保留 stable focus handle mapping。

被删除的 focused node：

1. 尝试父级 focus fallback；
2. 否则清除 focus；
3. 触发现有 focus-lost 流程。

### 18.3 Input handler / IME

focused text input 的 handler slot 每帧允许替换 callback/object，但只有 focus target 或 input configuration 改变时才通知 platform。

---

## 19. Frame 生命周期

```text
1. collect entity notifications
2. mark View BUILD damage
3. begin retained generation
4. render dirty Views
5. reconcile transient Elements
6. propagate damage
7. update persistent Taffy nodes
8. compute dirty layout roots
9. regenerate or replay prepaint snapshots
10. update handler slots
11. regenerate or replay paint snapshots
12. compose transforms, clips and layers
13. build next Frame facade
14. present
15. swap rendered/next frame
16. release removed retained nodes and expired snapshots
```

`Frame` 仍然可以保留为 platform 提交和事件查询的扁平视图，但它由 Retained Tree compose 得到，不再是唯一真实状态。

---

## 20. Window 结构变化

```rust
pub struct Window {
    // Existing fields...

    retained_tree: RetainedElementTree,
    layout_engine: TaffyLayoutEngine,
    rendered_frame: Frame,
    next_frame: Frame,

    renderer_generation: RendererGeneration,
    damage_tracker: DamageTracker,
}
```

建议新增文件：

```text
crates/gpui/src/retained/
├── mod.rs
├── tree.rs
├── node.rs
├── reconcile.rs
├── damage.rs
├── environment.rs
├── prepaint_snapshot.rs
├── paint_snapshot.rs
├── handlers.rs
└── debug.rs
```

不要继续把 retained 逻辑全部加入 `window.rs` 和 `element.rs`。

---

## 21. 调试与可观测性

### 21.1 Node counters

每帧记录：

```rust
struct RetainedFrameStats {
    nodes_total: usize,
    nodes_created: usize,
    nodes_removed: usize,
    nodes_reconciled: usize,
    layout_recomputed: usize,
    prepaint_rebuilt: usize,
    prepaint_replayed: usize,
    paint_rebuilt: usize,
    paint_replayed: usize,
    transform_only: usize,
    cache_bytes: usize,
}
```

### 21.2 Inspector

Inspector 增加：

- stable RetainedNodeId；
- key/type/owner View；
- current damage；
- 上一次 cache miss reason；
- layout/prepaint/paint generation；
- snapshot memory；
- source location。

### 21.3 Miss reason

```rust
enum RetentionMissReason {
    NewNode,
    TypeChanged,
    KeyChanged,
    PropertiesChanged,
    ChildrenChanged,
    LayoutConstraintsChanged,
    EnvironmentChanged,
    RendererGenerationChanged,
    LegacyElement,
    InspectorForced,
    ExplicitRefresh,
}
```

没有 miss reason 的 retained system 很难在真实复杂应用中优化。

---

## 22. 内存管理

### 22.1 Snapshot budget

每个 Window 设置预算：

```rust
struct RetainedBudget {
    max_snapshot_bytes: usize,
    max_unused_generations: u64,
}
```

默认建议：

- desktop：窗口可见 Scene 大小的 2–3 倍；
- embedded/mobile：由 backend 提供更小预算；
- debug inspector 显示当前预算使用。

### 22.2 回收策略

优先回收：

1. 已从 tree 删除的 snapshots；
2. 超过两帧未使用的 offscreen nodes；
3. 最大 paint snapshots；
4. 可重新生成的 prepaint snapshots；
5. persistent layout nodes 最后回收。

Virtualized List 已移除的 row nodes 默认立即进入回收，不保留无限 item cache。List 本身可以配置小型 row reuse pool。

---

## 23. 错误处理与安全回退

以下情况直接使相关 subtree `FULL` dirty：

- retained properties downcast 失败；
- duplicate explicit key；
- stale LayoutId；
- snapshot range/generation 不匹配；
- non-invertible transform；
- renderer resource generation 不匹配；
- custom Element 未声明 retention contract；
- inspector 需要执行真实 Element 方法。

Debug build 应断言并输出 element path；release build 应记录错误并重建 subtree，不允许显示旧内容。

提供运行时开关：

```text
GPUI_RETAINED_TREE=0
GPUI_RETAINED_LAYOUT=0
GPUI_RETAINED_PREPAINT=0
GPUI_RETAINED_PAINT=0
GPUI_RETAINED_TRANSFORM=0
```

用于快速定位回归和 A/B benchmark。

---

## 24. 测试设计

### 24.1 Identity

- keyed child 保持 identity；
- keyed reorder 不创建新节点；
- positional insert 导致预期范围重建；
- key 相同但 type 变化时替换；
- duplicate key 正确报错；
- View 在不同 parent 移动时明确选择重新挂载。

### 24.2 Layout

- style 不变不调用 Taffy setter；
- color 改变不触发布局；
- child size 改变传播到正确 ancestor；
- layout isolation 截断传播；
- window resize 更新 constraints；
- rem/scale factor 变化使相关文字重新布局；
- measured text/image 正确更新。

### 24.3 Prepaint

- hitbox translation；
- nested transforms；
- clip 后 hit test；
- focus path replay；
- tooltip parent mapping；
- deferred draw nesting；
- tab order；
- handler callback 更新但 geometry 不重建。

### 24.4 Paint

- Scene primitive replay 与全量 paint 像素一致；
- text glyph placement；
- shadows、borders、rounded clips；
- nested opacity；
- image atlas generation change；
- GPU device recovery；
- custom Canvas 安全回退。

### 24.5 Scrolling

- Virtualized List stable rows 只 transform；
- 新进入 viewport rows 创建；
- 离开 viewport rows 删除；
- overscan 正确；
- 普通 ScrollView children 不重新 paint；
- viewport clip 变化不泄漏内容；
- 120/144Hz 连续滚动无 input latency 回归。

### 24.6 Differential rendering

建立双路径测试：

```text
same UI state sequence
├── retained enabled
└── retained disabled/full rebuild
```

每帧比较：

- Scene primitives；
- hitboxes；
- dispatch behavior；
- focus；
- accessibility tree；
- screenshots/pixel output。

这是整个项目最重要的正确性机制。

---

## 25. Benchmark

至少包含：

1. 1000 个简单 Elements，单个颜色变化；
2. 深层嵌套布局，单个 leaf 变化；
3. 复杂窗口中 Virtualized List 滚动；
4. 普通 ScrollView 滚动；
5. transform animation；
6. layout animation；
7. 文本密集型 editor viewport；
8. 多 Panel，仅一个 Panel 更新；
9. 所有节点每帧变化的 worst case；
10. retained tree memory stress。

每个 benchmark 输出：

```text
median/p95 frame CPU
render/reconcile time
Taffy update/compute time
prepaint rebuild/replay time
paint rebuild/replay time
nodes created/removed
snapshot bytes
input deadline misses
```

必须包含 worst case，确保全量变化时 retained reconciliation 不会让 GPUI 比旧路径慢太多。目标是 worst-case CPU regression 不超过 10%。

---

## 26. 实施阶段

### Phase 0：Baseline 与 feature flags

- 添加 phase counters 和 benchmark；
- 增加 retained feature flags；
- 建立 retained/full-rebuild differential harness。

### Phase 1：Retained identity tree

- 建立 `RetainedElementTree`；
- 实现 key/type/slot reconciliation；
- 暂时仍然全量 layout/prepaint/paint；
- 验证增删、reorder、生命周期。

### Phase 2：Persistent Taffy

- RetainedNode 持有稳定 LayoutId；
- 不再每帧 clear Taffy；
- style/children 变化才调用 setter；
- 保持全量 prepaint/paint。

### Phase 3：Paint retention

- 节点化当前 Scene range replay；
- 引入 local-coordinate Scene layers；
- 未变化节点 replay paint snapshot。

### Phase 4：Prepaint retention

- 节点化 hitboxes、dispatch subtree、tooltips、tab stops；
- 实现 handler slots；
- 未变化节点 replay prepaint snapshot。

### Phase 5：Transform/clip composition

- Scroll offset 变为 transform；
- hit test 使用 inverse transform；
- clip 在 composition 阶段求交；
- 验证 virtualized/non-virtualized scrolling。

### Phase 6：Built-in fine-grained diff

依次支持：

1. `Div` / style；
2. text；
3. image/SVG；
4. list/uniform list；
5. interactive elements；
6. animation；
7. canvas opt-in。

### Phase 7：Entity::cached migration

- 使用 Retained Tree 实现 existing cache API；
- 删除重复 snapshot path；
- 保留行为兼容。

### Phase 8：默认启用

- Zed dogfood；
- GPUI Kit / Longbridge Pro dogfood；
- macOS/Windows/Linux differential tests；
- 先对 built-in Elements 默认启用；
- custom Elements 保持 conservative fallback。

---

## 27. PR 拆分建议

1. Benchmark、stats 和 feature flags。
2. `RetainedElementTree` skeleton 与 identity tests。
3. Reconciliation 与 node lifecycle。
4. Persistent Taffy nodes。
5. Layout damage propagation。
6. Scene layer 与 PaintSnapshot。
7. PrepaintSnapshot。
8. Handler slots。
9. Transform-aware hit testing。
10. Scroll/clip composition。
11. Div retained properties/diff。
12. Text retained properties/diff。
13. List retained integration。
14. `Entity::cached` migration。
15. Inspector、metrics 和默认启用。

每个 PR 必须能够独立测试，并且 feature flag 关闭时维持旧路径。

---

## 28. 关键设计决策

### 决策 1：不直接保留 AnyElement

原因：Element 可能包含 frame-local state 和 `FnOnce`。保留 properties 与 snapshots 更安全。

### 决策 2：第一版仍执行 dirty View::render

原因：无需同时引入 property-level dependency system；reconciliation 已经能消除大部分 layout/prepaint/paint 开销。

### 决策 3：自定义 Element 默认全量更新

原因：框架无法自动判断任意命令式 prepaint/paint 是否可复用。

### 决策 4：滚动使用 transform，不修改所有 child bounds

原因：这是降低持续滚动 CPU 的关键路径，也是 retained architecture 的主要收益。

### 决策 5：Frame 继续存在

原因：平台渲染、hit testing 和现有 API 仍需要扁平 frame representation。Retained Tree 是长期状态，Frame 是一次 composition 的结果。

### 决策 6：不实现 React scheduler

原因：GPUI 当前问题是重复计算，不是大型 DOM reconciliation 阻塞。并发调度会显著扩大复杂度，但不能直接解决滚动 CPU。

---

## 29. 成功标准

项目达到以下条件后，可以认为 Retained Element Tree 第一版完成：

1. GPUI API 使用方式基本不变；
2. 现有应用无需修改即可获得 built-in Elements 的 retained 优化；
3. complex virtualized list 滚动时，窗口其他静态 subtree 不执行 layout/prepaint/paint；
4. list row 仅位置变化时只更新 transform；
5. 单个颜色变化不会触发 layout；
6. 未变化文字不重新 shaping；
7. differential rendering suite 连续多帧一致；
8. Zed 和 Longbridge Pro 真实场景 CPU 明显下降；
9. worst-case 全量动态界面的回归不超过 10%；
10. 可以通过 feature flags 快速回退旧执行路径。

---

## 30. 最终执行模型

完成后，GPUI 的语义可以概括为：

> View remains immediate; Elements become retained.

开发者仍然以当前方式写：

```rust
fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement
```

但 GPUI 不再把每次返回值视为完全独立的新 UI，而是把它作为对持久 Element Tree 的最新描述：

```text
Render describes intent.
Reconciliation preserves identity.
Damage tracking limits work.
Persistent Taffy preserves layout.
Snapshots preserve prepaint and paint.
Transforms handle scrolling and animation.
```

这使 GPUI 能够同时保留 immediate-style API 的开发效率，以及 retained-mode 在 CPU、功耗和持续渲染方面的优势。
