use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    mem::size_of,
    ops::Range,
    sync::Arc,
};

use crate::{BedrockColor, TextLayout, UiAction, UiLimits, UiPoint, UiRect, UiScale};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct UiNodeId(u32);

impl UiNodeId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, Default)]
pub enum UiVisual {
    #[default]
    None,
    Solid {
        texture_page: u16,
        color: [u8; 4],
    },
    Text {
        layout: Arc<TextLayout>,
        color: [u8; 4],
    },
}

#[derive(Clone, Debug)]
pub struct UiNode {
    id: UiNodeId,
    parent: Option<UiNodeId>,
    bounds: UiRect,
    focusable: bool,
    navigation_order: Option<u32>,
    clip_children: bool,
    visual: UiVisual,
}

impl UiNode {
    pub fn new(id: UiNodeId, parent: Option<UiNodeId>, bounds: UiRect) -> Self {
        Self {
            id,
            parent,
            bounds,
            focusable: false,
            navigation_order: None,
            clip_children: false,
            visual: UiVisual::None,
        }
    }

    pub fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    pub fn with_navigation_order(mut self, navigation_order: u32) -> Self {
        self.navigation_order = Some(navigation_order);
        self
    }

    pub fn with_clip_children(mut self, clip_children: bool) -> Self {
        self.clip_children = clip_children;
        self
    }

    pub fn with_visual(mut self, visual: UiVisual) -> Self {
        self.visual = visual;
        self
    }

    pub const fn id(&self) -> UiNodeId {
        self.id
    }
}

#[derive(Clone, Debug, Default)]
pub struct FocusState {
    focused: Option<UiNodeId>,
    pointer_capture: Option<UiNodeId>,
}

impl FocusState {
    pub const fn focused(&self) -> Option<UiNodeId> {
        self.focused
    }

    pub const fn pointer_capture(&self) -> Option<UiNodeId> {
        self.pointer_capture
    }

    pub fn set_focused(&mut self, focused: Option<UiNodeId>) -> Option<UiNodeId> {
        if self.focused == focused {
            return None;
        }
        self.focused = focused;
        self.pointer_capture.take()
    }

    pub fn capture_pointer(&mut self, node: UiNodeId) -> Result<(), UiError> {
        if self.focused != Some(node) {
            return Err(UiError::PointerCaptureRequiresFocus { node });
        }
        self.pointer_capture = Some(node);
        Ok(())
    }

    pub fn release_pointer(&mut self) -> Option<UiNodeId> {
        self.pointer_capture.take()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusTransition {
    pub focused: Option<UiNodeId>,
    pub released_capture: Option<UiNodeId>,
}

#[derive(Clone, Debug)]
pub struct UiFrame {
    revision: u64,
    viewport: UiRect,
    bounds: BTreeMap<UiNodeId, UiRect>,
    focus_order: Box<[UiNodeId]>,
}

impl UiFrame {
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn focus_order(&self) -> &[UiNodeId] {
        &self.focus_order
    }

    pub fn bounds(&self, node: UiNodeId) -> Option<UiRect> {
        self.bounds.get(&node).copied()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiVertex {
    pub position: [f32; 2],
    pub uv: [u16; 2],
    pub color: [u8; 4],
    pub style_flags: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiDrawBatch {
    pub texture_page: u16,
    pub clip: UiRect,
    pub index_range: Range<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiDrawList {
    pub revision: u64,
    pub vertices: Vec<UiVertex>,
    pub indices: Vec<u32>,
    pub batches: Vec<UiDrawBatch>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum UiError {
    NodeLimitExceeded { actual: usize, limit: usize },
    FocusableLimitExceeded { actual: usize, limit: usize },
    DuplicateNodeId { id: UiNodeId },
    MissingParent { node: UiNodeId, parent: UiNodeId },
    ParentCycle { node: UiNodeId },
    InvalidSafeViewport,
    LayoutRevisionOverflow,
    MissingLayoutBounds { node: UiNodeId },
    StaleFrame { expected: u64, actual: u64 },
    PointerCaptureRequiresFocus { node: UiNodeId },
    ClipDepthExceeded { actual: usize, limit: usize },
    VertexLimitExceeded { actual: usize, limit: usize },
    IndexLimitExceeded { actual: usize, limit: usize },
    DrawBatchLimitExceeded { actual: usize, limit: usize },
    DrawByteLimitExceeded { actual: usize, limit: usize },
    DrawIndexOverflow,
    DrawAllocationFailed,
}

impl fmt::Display for UiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "retained UI error: {self:?}")
    }
}

impl std::error::Error for UiError {}

pub struct UiTree {
    nodes: BTreeMap<UiNodeId, UiNode>,
    children: BTreeMap<UiNodeId, Vec<UiNodeId>>,
    roots: Box<[UiNodeId]>,
    focus: FocusState,
    revision: u64,
    frame: Option<UiFrame>,
}

impl UiTree {
    pub fn new(nodes: Vec<UiNode>) -> Result<Self, UiError> {
        if nodes.len() > UiLimits::MAX_NODES {
            return Err(UiError::NodeLimitExceeded {
                actual: nodes.len(),
                limit: UiLimits::MAX_NODES,
            });
        }
        let mut by_id = BTreeMap::new();
        for node in nodes {
            let id = node.id;
            if by_id.insert(id, node).is_some() {
                return Err(UiError::DuplicateNodeId { id });
            }
        }
        for node in by_id.values() {
            if let Some(parent) = node.parent
                && !by_id.contains_key(&parent)
            {
                return Err(UiError::MissingParent {
                    node: node.id,
                    parent,
                });
            }
        }
        reject_parent_cycles(&by_id)?;

        let focusable = by_id.values().filter(|node| node.focusable).count();
        if focusable > UiLimits::MAX_FOCUSABLE {
            return Err(UiError::FocusableLimitExceeded {
                actual: focusable,
                limit: UiLimits::MAX_FOCUSABLE,
            });
        }
        let mut children = BTreeMap::<UiNodeId, Vec<UiNodeId>>::new();
        let mut roots = Vec::new();
        for node in by_id.values() {
            if let Some(parent) = node.parent {
                children.entry(parent).or_default().push(node.id);
            } else {
                roots.push(node.id);
            }
        }

        Ok(Self {
            nodes: by_id,
            children,
            roots: roots.into_boxed_slice(),
            focus: FocusState::default(),
            revision: 0,
            frame: None,
        })
    }

    pub fn focus(&self) -> &FocusState {
        &self.focus
    }

    pub fn focus_mut(&mut self) -> &mut FocusState {
        &mut self.focus
    }

    pub fn layout(
        &mut self,
        viewport: UiRect,
        scale: UiScale,
        safe_area: crate::SafeArea,
    ) -> Result<UiFrame, UiError> {
        let content_min = UiPoint::new(
            viewport.min().x() + safe_area.left(),
            viewport.min().y() + safe_area.top(),
        )
        .map_err(|_| UiError::InvalidSafeViewport)?;
        let content_max = UiPoint::new(
            viewport.max().x() - safe_area.right(),
            viewport.max().y() - safe_area.bottom(),
        )
        .map_err(|_| UiError::InvalidSafeViewport)?;
        let content =
            UiRect::new(content_min, content_max).map_err(|_| UiError::InvalidSafeViewport)?;
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(UiError::LayoutRevisionOverflow)?;

        let mut bounds = BTreeMap::new();
        let mut pending = self.roots.iter().rev().copied().collect::<Vec<_>>();
        while let Some(id) = pending.pop() {
            let node = &self.nodes[&id];
            let origin = node
                .parent
                .and_then(|parent| bounds.get(&parent).copied())
                .map_or(content.min(), UiRect::min);
            let scaled = scale_rect(node.bounds, origin, scale.get())?;
            bounds.insert(id, scaled);
            if let Some(children) = self.children.get(&id) {
                pending.extend(children.iter().rev().copied());
            }
        }

        let mut focus_order = self
            .nodes
            .values()
            .filter(|node| node.focusable)
            .map(|node| {
                let node_bounds = bounds[&node.id];
                (
                    node.navigation_order.is_none(),
                    node.navigation_order.unwrap_or(u32::MAX),
                    FloatOrder::new(node_bounds.min().y()),
                    FloatOrder::new(node_bounds.min().x()),
                    node.id,
                )
            })
            .collect::<Vec<_>>();
        focus_order.sort_unstable();
        let frame = UiFrame {
            revision: next_revision,
            viewport: content,
            bounds,
            focus_order: focus_order.into_iter().map(|(_, _, _, _, id)| id).collect(),
        };
        self.revision = next_revision;
        self.frame = Some(frame.clone());
        Ok(frame)
    }

    pub fn handle_action(
        &mut self,
        frame: &UiFrame,
        action: UiAction,
    ) -> Result<FocusTransition, UiError> {
        if frame.revision != self.revision {
            return Err(UiError::StaleFrame {
                expected: self.revision,
                actual: frame.revision,
            });
        }
        let previous_capture = self.focus.pointer_capture();
        match action {
            UiAction::TabNext => self.move_focus(frame, 1),
            UiAction::TabPrevious => self.move_focus(frame, -1),
            UiAction::Navigate([horizontal, vertical]) if horizontal != 0 || vertical != 0 => {
                let step = if vertical < 0 || (vertical == 0 && horizontal < 0) {
                    -1
                } else {
                    1
                };
                self.move_focus(frame, step);
            }
            UiAction::PointerPrimary { position, phase } => match phase {
                crate::PointerPhase::Pressed => {
                    if let Some(node) = frame
                        .focus_order
                        .iter()
                        .rev()
                        .copied()
                        .find(|node| frame.bounds[node].contains(position))
                    {
                        self.focus.set_focused(Some(node));
                        self.focus.capture_pointer(node)?;
                    }
                }
                crate::PointerPhase::Released => {
                    self.focus.release_pointer();
                }
                crate::PointerPhase::Held => {}
            },
            _ => {}
        }
        let released_capture =
            previous_capture.filter(|capture| self.focus.pointer_capture() != Some(*capture));
        Ok(FocusTransition {
            focused: self.focus.focused(),
            released_capture,
        })
    }

    pub fn build_draw_list(&self) -> Result<UiDrawList, UiError> {
        let synthetic;
        let frame = if let Some(frame) = &self.frame {
            frame
        } else {
            synthetic = self.synthetic_frame()?;
            &synthetic
        };
        let (quad_count, vertex_count, index_count) = self.draw_counts()?;
        let batch_capacity = quad_count.min(UiLimits::MAX_DRAW_BATCHES);
        let reserved_bytes = vertex_count
            .checked_mul(size_of::<UiVertex>())
            .and_then(|bytes| {
                index_count
                    .checked_mul(size_of::<u32>())
                    .and_then(|index_bytes| bytes.checked_add(index_bytes))
            })
            .and_then(|bytes| {
                batch_capacity
                    .checked_mul(size_of::<UiDrawBatch>())
                    .and_then(|batch_bytes| bytes.checked_add(batch_bytes))
            })
            .ok_or(UiError::DrawIndexOverflow)?;
        if reserved_bytes > UiLimits::MAX_DRAW_LIST_BYTES {
            return Err(UiError::DrawByteLimitExceeded {
                actual: reserved_bytes,
                limit: UiLimits::MAX_DRAW_LIST_BYTES,
            });
        }
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut batches = Vec::new();
        vertices
            .try_reserve_exact(vertex_count)
            .map_err(|_| UiError::DrawAllocationFailed)?;
        indices
            .try_reserve_exact(index_count)
            .map_err(|_| UiError::DrawAllocationFailed)?;
        batches
            .try_reserve_exact(batch_capacity)
            .map_err(|_| UiError::DrawAllocationFailed)?;

        let mut pending = self
            .roots
            .iter()
            .rev()
            .map(|id| (*id, frame.viewport, 0usize))
            .collect::<Vec<_>>();
        while let Some((id, clip, clip_depth)) = pending.pop() {
            let node = &self.nodes[&id];
            let bounds = frame
                .bounds(id)
                .ok_or(UiError::MissingLayoutBounds { node: id })?;
            if !is_empty(clip) {
                emit_visual(
                    &node.visual,
                    bounds,
                    clip,
                    &mut vertices,
                    &mut indices,
                    &mut batches,
                )?;
            }

            let (child_clip, child_depth) = if node.clip_children {
                let actual = clip_depth
                    .checked_add(1)
                    .ok_or(UiError::DrawIndexOverflow)?;
                if actual > UiLimits::MAX_CLIP_DEPTH {
                    return Err(UiError::ClipDepthExceeded {
                        actual,
                        limit: UiLimits::MAX_CLIP_DEPTH,
                    });
                }
                (intersect(clip, bounds), actual)
            } else {
                (clip, clip_depth)
            };
            if let Some(children) = self.children.get(&id) {
                pending.extend(
                    children
                        .iter()
                        .rev()
                        .map(|child| (*child, child_clip, child_depth)),
                );
            }
        }
        Ok(UiDrawList {
            revision: frame.revision,
            vertices,
            indices,
            batches,
        })
    }

    fn move_focus(&mut self, frame: &UiFrame, step: isize) {
        if frame.focus_order.is_empty() {
            self.focus.set_focused(None);
            return;
        }
        let current = self
            .focus
            .focused()
            .and_then(|focused| frame.focus_order.iter().position(|node| *node == focused));
        let next = match (current, step.is_negative()) {
            (Some(index), false) => (index + 1) % frame.focus_order.len(),
            (Some(0), true) | (None, true) => frame.focus_order.len() - 1,
            (Some(index), true) => index - 1,
            (None, false) => 0,
        };
        self.focus.set_focused(Some(frame.focus_order[next]));
    }

    fn synthetic_frame(&self) -> Result<UiFrame, UiError> {
        let viewport = UiRect::new(
            UiPoint::new(-f32::MAX / 4.0, -f32::MAX / 4.0)
                .map_err(|_| UiError::InvalidSafeViewport)?,
            UiPoint::new(f32::MAX / 4.0, f32::MAX / 4.0)
                .map_err(|_| UiError::InvalidSafeViewport)?,
        )
        .map_err(|_| UiError::InvalidSafeViewport)?;
        Ok(UiFrame {
            revision: self.revision,
            viewport,
            bounds: self
                .nodes
                .iter()
                .map(|(id, node)| (*id, node.bounds))
                .collect(),
            focus_order: Box::new([]),
        })
    }

    fn draw_counts(&self) -> Result<(usize, usize, usize), UiError> {
        let quads = self.nodes.values().try_fold(0usize, |total, node| {
            let count = match &node.visual {
                UiVisual::None => 0,
                UiVisual::Solid { .. } => 1,
                UiVisual::Text { layout, .. } => layout.glyphs().len(),
            };
            total.checked_add(count).ok_or(UiError::DrawIndexOverflow)
        })?;
        let vertices = quads.checked_mul(4).ok_or(UiError::DrawIndexOverflow)?;
        if vertices > UiLimits::MAX_UI_VERTICES {
            return Err(UiError::VertexLimitExceeded {
                actual: vertices,
                limit: UiLimits::MAX_UI_VERTICES,
            });
        }
        let indices = quads.checked_mul(6).ok_or(UiError::DrawIndexOverflow)?;
        if indices > UiLimits::MAX_UI_INDICES {
            return Err(UiError::IndexLimitExceeded {
                actual: indices,
                limit: UiLimits::MAX_UI_INDICES,
            });
        }
        Ok((quads, vertices, indices))
    }
}

fn reject_parent_cycles(nodes: &BTreeMap<UiNodeId, UiNode>) -> Result<(), UiError> {
    let mut complete = BTreeSet::new();
    for start in nodes.keys().copied() {
        if complete.contains(&start) {
            continue;
        }
        let mut path = BTreeSet::new();
        let mut visited = Vec::new();
        let mut cursor = Some(start);
        while let Some(id) = cursor {
            if complete.contains(&id) {
                break;
            }
            if !path.insert(id) {
                return Err(UiError::ParentCycle { node: id });
            }
            visited.push(id);
            cursor = nodes[&id].parent;
        }
        complete.extend(visited);
    }
    Ok(())
}

fn scale_rect(rect: UiRect, origin: UiPoint, scale: f32) -> Result<UiRect, UiError> {
    UiRect::new(
        UiPoint::new(
            origin.x() + rect.min().x() * scale,
            origin.y() + rect.min().y() * scale,
        )
        .map_err(|_| UiError::InvalidSafeViewport)?,
        UiPoint::new(
            origin.x() + rect.max().x() * scale,
            origin.y() + rect.max().y() * scale,
        )
        .map_err(|_| UiError::InvalidSafeViewport)?,
    )
    .map_err(|_| UiError::InvalidSafeViewport)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FloatOrder(u32);

impl FloatOrder {
    fn new(value: f32) -> Self {
        Self(value.to_bits())
    }
}

impl From<f32> for FloatOrder {
    fn from(value: f32) -> Self {
        Self::new(value)
    }
}

impl Ord for FloatOrder {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        f32::from_bits(self.0).total_cmp(&f32::from_bits(other.0))
    }
}

impl PartialOrd for FloatOrder {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn intersect(left: UiRect, right: UiRect) -> UiRect {
    UiRect::new(
        UiPoint::new(
            left.min().x().max(right.min().x()),
            left.min().y().max(right.min().y()),
        )
        .expect("finite rectangles have a finite intersection minimum"),
        UiPoint::new(
            left.max().x().min(right.max().x()),
            left.max().y().min(right.max().y()),
        )
        .expect("finite rectangles have a finite intersection maximum"),
    )
    .unwrap_or_else(|_| {
        let point = UiPoint::new(0.0, 0.0).expect("zero is finite");
        UiRect::new(point, point).expect("equal points form a rectangle")
    })
}

fn is_empty(rect: UiRect) -> bool {
    rect.width() == 0.0 || rect.height() == 0.0
}

fn emit_visual(
    visual: &UiVisual,
    bounds: UiRect,
    clip: UiRect,
    vertices: &mut Vec<UiVertex>,
    indices: &mut Vec<u32>,
    batches: &mut Vec<UiDrawBatch>,
) -> Result<(), UiError> {
    match visual {
        UiVisual::None => Ok(()),
        UiVisual::Solid {
            texture_page,
            color,
        } => {
            if is_empty(bounds) {
                return Ok(());
            }
            emit_quad(
                bounds,
                [[0, 0], [1, 0], [1, 1], [0, 1]],
                *texture_page,
                *color,
                0,
                clip,
                vertices,
                indices,
                batches,
            )
        }
        UiVisual::Text { layout, color } => {
            for glyph in layout.glyphs() {
                let glyph_bounds = UiRect::new(
                    UiPoint::new(
                        bounds.min().x() + glyph.bounds_64[0] as f32 / 64.0,
                        bounds.min().y() + glyph.bounds_64[1] as f32 / 64.0,
                    )
                    .map_err(|_| UiError::DrawIndexOverflow)?,
                    UiPoint::new(
                        bounds.min().x() + glyph.bounds_64[2] as f32 / 64.0,
                        bounds.min().y() + glyph.bounds_64[3] as f32 / 64.0,
                    )
                    .map_err(|_| UiError::DrawIndexOverflow)?,
                )
                .map_err(|_| UiError::DrawIndexOverflow)?;
                if is_empty(glyph_bounds) {
                    continue;
                }
                let glyph_color = style_color(glyph.style.color, *color);
                let style_flags = u8::from(glyph.style.obfuscated)
                    | (u8::from(glyph.style.bold) << 1)
                    | (u8::from(glyph.style.italic) << 2);
                emit_quad(
                    glyph_bounds,
                    [
                        [glyph.uv[0], glyph.uv[1]],
                        [glyph.uv[2], glyph.uv[1]],
                        [glyph.uv[2], glyph.uv[3]],
                        [glyph.uv[0], glyph.uv[3]],
                    ],
                    glyph.page,
                    glyph_color,
                    style_flags,
                    clip,
                    vertices,
                    indices,
                    batches,
                )?;
            }
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_quad(
    bounds: UiRect,
    uv: [[u16; 2]; 4],
    texture_page: u16,
    color: [u8; 4],
    style_flags: u8,
    clip: UiRect,
    vertices: &mut Vec<UiVertex>,
    indices: &mut Vec<u32>,
    batches: &mut Vec<UiDrawBatch>,
) -> Result<(), UiError> {
    let next_vertices = vertices
        .len()
        .checked_add(4)
        .ok_or(UiError::DrawIndexOverflow)?;
    if next_vertices > UiLimits::MAX_UI_VERTICES {
        return Err(UiError::VertexLimitExceeded {
            actual: next_vertices,
            limit: UiLimits::MAX_UI_VERTICES,
        });
    }
    let next_indices = indices
        .len()
        .checked_add(6)
        .ok_or(UiError::DrawIndexOverflow)?;
    if next_indices > UiLimits::MAX_UI_INDICES {
        return Err(UiError::IndexLimitExceeded {
            actual: next_indices,
            limit: UiLimits::MAX_UI_INDICES,
        });
    }
    let base = u32::try_from(vertices.len()).map_err(|_| UiError::DrawIndexOverflow)?;
    let positions = [
        [bounds.min().x(), bounds.min().y()],
        [bounds.max().x(), bounds.min().y()],
        [bounds.max().x(), bounds.max().y()],
        [bounds.min().x(), bounds.max().y()],
    ];
    vertices.extend(
        positions
            .into_iter()
            .zip(uv)
            .map(|(position, uv)| UiVertex {
                position,
                uv,
                color,
                style_flags,
            }),
    );
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    let start = u32::try_from(indices.len() - 6).map_err(|_| UiError::DrawIndexOverflow)?;
    let end = u32::try_from(indices.len()).map_err(|_| UiError::DrawIndexOverflow)?;
    if let Some(batch) = batches.last_mut()
        && batch.texture_page == texture_page
        && batch.clip == clip
        && batch.index_range.end == start
    {
        batch.index_range.end = end;
        return Ok(());
    }
    let actual = batches
        .len()
        .checked_add(1)
        .ok_or(UiError::DrawIndexOverflow)?;
    if actual > UiLimits::MAX_DRAW_BATCHES {
        return Err(UiError::DrawBatchLimitExceeded {
            actual,
            limit: UiLimits::MAX_DRAW_BATCHES,
        });
    }
    batches.push(UiDrawBatch {
        texture_page,
        clip,
        index_range: start..end,
    });
    Ok(())
}

fn style_color(style: BedrockColor, base: [u8; 4]) -> [u8; 4] {
    let rgb = match style {
        BedrockColor::White => return base,
        BedrockColor::Black => [0, 0, 0],
        BedrockColor::DarkBlue => [0, 0, 170],
        BedrockColor::DarkGreen => [0, 170, 0],
        BedrockColor::DarkAqua => [0, 170, 170],
        BedrockColor::DarkRed => [170, 0, 0],
        BedrockColor::DarkPurple => [170, 0, 170],
        BedrockColor::Gold => [255, 170, 0],
        BedrockColor::Gray => [170, 170, 170],
        BedrockColor::DarkGray => [85, 85, 85],
        BedrockColor::Blue => [85, 85, 255],
        BedrockColor::Green => [85, 255, 85],
        BedrockColor::Aqua => [85, 255, 255],
        BedrockColor::Red => [255, 85, 85],
        BedrockColor::LightPurple => [255, 85, 255],
        BedrockColor::Yellow => [255, 255, 85],
        BedrockColor::MinecoinGold => [221, 214, 5],
        BedrockColor::MaterialQuartz => [227, 212, 209],
        BedrockColor::MaterialIron => [206, 202, 202],
        BedrockColor::MaterialNetherite => [68, 58, 59],
        BedrockColor::MaterialRedstone => [151, 22, 7],
        BedrockColor::MaterialCopper => [180, 104, 77],
        BedrockColor::MaterialGold => [222, 177, 45],
        BedrockColor::MaterialEmerald => [17, 160, 54],
        BedrockColor::MaterialDiamond => [44, 186, 168],
        BedrockColor::MaterialLapis => [35, 98, 180],
        BedrockColor::MaterialAmethyst => [154, 92, 198],
        BedrockColor::MaterialResin => [237, 105, 52],
    };
    [rgb[0], rgb[1], rgb[2], base[3]]
}
