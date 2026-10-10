//! Retains unchanged node output and merges fresh emissions in full-build draw order.

use std::ops::Range;

use super::{
    DrawCounts, TextEffects, UiDrawBatch, UiDrawList, UiError, UiNode, UiNodeId, UiTree, UiVertex,
    UiVisual,
    draw::{DrawSpace, emit_visual, is_empty},
    intersect, scale_rect,
};
use crate::{FormattingPalette, SafeArea, UiLimits, UiRect, UiScale};

/// What [`RetainedDraw::update`] changed in the list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawUpdate {
    /// Vertex ranges written again, in order. Unless `rebuilt`, everything outside them, and
    /// every index and batch, is as before.
    pub vertices: Vec<Range<usize>>,
    /// Indices or batches changed, so the whole list must be read again.
    pub rebuilt: bool,
    /// Nodes emitted again.
    pub emitted: usize,
}

/// One node in draw order: where it sits and what it emitted.
#[derive(Clone, Debug)]
struct Placed {
    /// Index of the node in the caller's node list.
    node: usize,
    parent: Option<usize>,
    /// One past the last draw position of this node's subtree.
    end: usize,
    bounds: UiRect,
    /// The clip handed down by the parent, before the node's own projection.
    inherited: UiRect,
    vertices: Range<usize>,
    indices: Range<usize>,
    /// This node's batches in [`RetainedDraw::runs`], before merging with its neighbours.
    runs: Range<usize>,
    counts: DrawCounts,
}

/// The inputs a list was built for, besides its nodes.
#[derive(Clone, Copy, PartialEq)]
struct Frame {
    viewport: UiRect,
    scale: UiScale,
    safe_area: SafeArea,
    palette: Option<FormattingPalette>,
}

/// A draw list with what each node contributed to it. It holds no nodes: each call passes the
/// node list the list was last drawn from.
pub struct RetainedDraw {
    frame: Frame,
    content: UiRect,
    /// Nodes in draw order.
    placed: Vec<Placed>,
    /// Each caller node's draw position.
    positions: Vec<usize>,
    /// Each node's own batches, index ranges relative to the node's first index.
    runs: Vec<UiDrawBatch>,
    counts: DrawCounts,
    list: UiDrawList,
}

/// One node's emission on its own, indices relative to its first vertex.
#[derive(Default)]
struct Scratch {
    vertices: Vec<UiVertex>,
    indices: Vec<u32>,
    batches: Vec<UiDrawBatch>,
}

impl RetainedDraw {
    /// Keeps each node's output from the same layout and drawing as a full `UiTree` build.
    pub fn build(
        nodes: &[UiNode],
        viewport: UiRect,
        scale: UiScale,
        safe_area: SafeArea,
        effects: TextEffects<'_>,
    ) -> Result<Self, UiError> {
        let mut tree = UiTree::new(nodes.to_vec())?;
        tree.layout(viewport, scale, safe_area)?;
        let frame = Frame {
            viewport,
            scale,
            safe_area,
            palette: effects.palette.copied(),
        };
        // A list the full build rejects reports that build's own error.
        Self::retain(&tree, nodes, frame, effects)
            .map_err(|error| tree.build_draw_list_with(effects).err().unwrap_or(error))
    }
    /// Retains each laid-out node and its independent draw runs.
    fn retain(
        tree: &UiTree,
        nodes: &[UiNode],
        frame: Frame,
        effects: TextEffects<'_>,
    ) -> Result<Self, UiError> {
        let layout = tree.frame.as_ref().ok_or(UiError::InvalidSafeViewport)?;
        let index_of: std::collections::BTreeMap<UiNodeId, usize> = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id, index))
            .collect();
        let mut counts = DrawCounts::default();
        let mut placed: Vec<Placed> = Vec::with_capacity(nodes.len());
        let mut pending: Vec<(UiNodeId, UiRect, usize, Option<usize>)> = tree
            .roots
            .iter()
            .rev()
            .map(|id| (*id, layout.viewport, 0, None))
            .collect();
        // Draw positions whose subtrees are still being visited.
        let mut open: Vec<usize> = Vec::new();
        while let Some((id, inherited, clip_depth, parent)) = pending.pop() {
            while let Some(&last) = open.last() {
                if Some(last) == parent {
                    break;
                }
                placed[last].end = placed.len();
                open.pop();
            }
            let node = &tree.nodes[&id];
            let node_counts = DrawCounts::of(&node.visual)?;
            counts = counts.add(node_counts)?;
            let bounds = layout
                .bounds(id)
                .ok_or(UiError::MissingLayoutBounds { node: id })?;
            let position = placed.len();
            placed.push(Placed {
                node: index_of[&id],
                parent,
                end: position + 1,
                bounds,
                inherited,
                vertices: 0..0,
                indices: 0..0,
                runs: 0..0,
                counts: node_counts,
            });
            open.push(position);
            let clip = draw_clip(node, inherited)?;
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
            if let Some(children) = tree.children.get(&id) {
                pending.extend(
                    children
                        .iter()
                        .rev()
                        .map(|child| (*child, child_clip, child_depth, Some(position))),
                );
            }
        }
        while let Some(last) = open.pop() {
            placed[last].end = placed.len();
        }
        counts.budget()?;
        let mut positions = vec![0; nodes.len()];
        for (position, place) in placed.iter().enumerate() {
            positions[place.node] = position;
        }
        let mut retained = Self {
            frame,
            content: layout.viewport,
            placed,
            positions,
            runs: Vec::new(),
            counts,
            list: UiDrawList {
                revision: layout.revision,
                vertices: Vec::new(),
                indices: Vec::new(),
                batches: Vec::new(),
            },
        };
        let mut scratch = Scratch::default();
        for position in 0..retained.placed.len() {
            retained.emit(nodes, position, effects, &mut scratch)?;
            let place = &mut retained.placed[position];
            let vertex_start = retained.list.vertices.len();
            let index_start = retained.list.indices.len();
            let run_start = retained.runs.len();
            append(&mut retained.list, &mut retained.runs, scratch.emitted())?;
            place.vertices = vertex_start..retained.list.vertices.len();
            place.indices = index_start..retained.list.indices.len();
            place.runs = run_start..retained.runs.len();
        }
        check_limits(&retained.list)?;
        Ok(retained)
    }

    /// The current draw list.
    pub fn draw_list(&self) -> &UiDrawList {
        &self.list
    }

    /// Re-emits changed nodes when shape and frame inputs match, updating `last`.
    /// On `None`, or for animated `§k` text, the caller must discard this cache and build afresh.
    pub fn update(
        &mut self,
        last: &mut [UiNode],
        nodes: &[UiNode],
        viewport: UiRect,
        scale: UiScale,
        safe_area: SafeArea,
        effects: TextEffects<'_>,
    ) -> Option<DrawUpdate> {
        let frame = Frame {
            viewport,
            scale,
            safe_area,
            palette: effects.palette.copied(),
        };
        if frame != self.frame
            || nodes.len() != last.len()
            || nodes.len() != self.positions.len()
            || nodes
                .iter()
                .zip(last.iter())
                .any(|(new, old)| !same_shape(new, old))
        {
            return None;
        }
        // An unchanged frame allocates nothing.
        let Some(first) = nodes
            .iter()
            .zip(last.iter())
            .position(|(new, old)| new != old)
        else {
            return Some(DrawUpdate::default());
        };
        // Draw positions to emit again: a moved node carries its subtree along.
        let mut dirty = vec![false; self.placed.len()];
        let mut moved = false;
        for (index, (new, old)) in nodes.iter().zip(last.iter_mut()).enumerate().skip(first) {
            if new == old {
                continue;
            }
            if matches!(new.visual, UiVisual::Mesh(_)) && new.world_projection.is_some() {
                return None;
            }
            let position = self.positions[index];
            if new.bounds != old.bounds {
                moved = true;
                dirty[position..self.placed[position].end].fill(true);
            } else {
                dirty[position] = true;
            }
            old.clone_from(new);
        }
        if moved && self.relayout(nodes, &dirty).is_err() {
            return None;
        }
        let mut counts = self.counts;
        let mut emitted = Vec::new();
        for (position, _) in dirty.iter().enumerate().filter(|(_, dirty)| **dirty) {
            let node_counts = DrawCounts::of(&nodes[self.placed[position].node].visual).ok()?;
            counts = subtract(counts, self.placed[position].counts)
                .add(node_counts)
                .ok()?;
            self.placed[position].counts = node_counts;
            let mut scratch = Scratch::default();
            self.emit(nodes, position, effects, &mut scratch).ok()?;
            emitted.push((position, scratch));
        }
        counts.budget().ok()?;
        self.counts = counts;
        let mut update = DrawUpdate {
            emitted: emitted.len(),
            ..DrawUpdate::default()
        };
        if emitted
            .iter()
            .all(|(position, scratch)| self.same_footprint(*position, scratch))
        {
            for (position, scratch) in &emitted {
                let range = self.placed[*position].vertices.clone();
                if range.is_empty() {
                    continue;
                }
                self.list.vertices[range.clone()].copy_from_slice(&scratch.vertices);
                update.vertices.push(range);
            }
            return Some(update);
        }
        self.splice(emitted).ok()?;
        update.rebuilt = true;
        Some(update)
    }

    /// Places the dirty subtrees again from their parents' bounds and clips.
    fn relayout(&mut self, nodes: &[UiNode], dirty: &[bool]) -> Result<(), UiError> {
        let scale = self.frame.scale.get();
        for position in 0..self.placed.len() {
            if !dirty[position] {
                continue;
            }
            let place = &self.placed[position];
            let node = &nodes[place.node];
            let (origin, inherited) = match place.parent {
                Some(parent) => {
                    let parent_place = &self.placed[parent];
                    let parent_node = &nodes[parent_place.node];
                    let clip = draw_clip(parent_node, parent_place.inherited)?;
                    let child_clip = if parent_node.clip_children {
                        intersect(clip, parent_place.bounds)
                    } else {
                        clip
                    };
                    (parent_place.bounds.min(), child_clip)
                }
                None => (self.content.min(), self.content),
            };
            let bounds = if node.world_projection.is_some() {
                node.bounds
            } else {
                scale_rect(node.bounds, origin, scale)?
            };
            let place = &mut self.placed[position];
            place.bounds = bounds;
            place.inherited = inherited;
        }
        Ok(())
    }

    /// Emits the node at draw `position` on its own into `scratch`.
    fn emit(
        &self,
        nodes: &[UiNode],
        position: usize,
        effects: TextEffects<'_>,
        scratch: &mut Scratch,
    ) -> Result<(), UiError> {
        scratch.vertices.clear();
        scratch.indices.clear();
        scratch.batches.clear();
        let place = &self.placed[position];
        let node = &nodes[place.node];
        let clip = draw_clip(node, place.inherited)?;
        if is_empty(clip) {
            return Ok(());
        }
        emit_visual(
            &node.visual,
            place.bounds,
            DrawSpace {
                clip,
                projection: node.world_projection.as_ref(),
                node: node.id,
            },
            effects,
            &mut scratch.vertices,
            &mut scratch.indices,
            &mut scratch.batches,
        )
    }

    /// Whether a fresh emission fills exactly the vertices, indices and batches the node had.
    fn same_footprint(&self, position: usize, scratch: &Scratch) -> bool {
        let place = &self.placed[position];
        let base = place.vertices.start as u32;
        scratch.vertices.len() == place.vertices.len()
            && scratch.indices.len() == place.indices.len()
            && scratch
                .indices
                .iter()
                .zip(&self.list.indices[place.indices.clone()])
                .all(|(local, global)| local + base == *global)
            && scratch.batches[..] == self.runs[place.runs.clone()]
    }

    /// Rebuilds the list from unchanged nodes' output and the fresh emissions, in draw order.
    fn splice(&mut self, emitted: Vec<(usize, Scratch)>) -> Result<(), UiError> {
        let mut fresh = emitted.into_iter().peekable();
        let revision = self.list.revision;
        let old_list = std::mem::replace(
            &mut self.list,
            UiDrawList {
                revision,
                vertices: Vec::new(),
                indices: Vec::new(),
                batches: Vec::new(),
            },
        );
        self.list.vertices.reserve(old_list.vertices.len());
        self.list.indices.reserve(old_list.indices.len());
        self.list.batches.reserve(old_list.batches.len());
        let old_runs = std::mem::take(&mut self.runs);
        self.runs.reserve(old_runs.len());
        for position in 0..self.placed.len() {
            let vertex_start = self.list.vertices.len();
            let index_start = self.list.indices.len();
            let run_start = self.runs.len();
            let place = &self.placed[position];
            let next = fresh.next_if(|(next, _)| *next == position);
            let emitted = match &next {
                Some((_, scratch)) => scratch.emitted(),
                None => Emitted {
                    vertices: &old_list.vertices[place.vertices.clone()],
                    indices: &old_list.indices[place.indices.clone()],
                    base: place.vertices.start as u32,
                    batches: &old_runs[place.runs.clone()],
                },
            };
            append(&mut self.list, &mut self.runs, emitted)?;
            let place = &mut self.placed[position];
            place.vertices = vertex_start..self.list.vertices.len();
            place.indices = index_start..self.list.indices.len();
            place.runs = run_start..self.runs.len();
        }
        check_limits(&self.list)
    }
}

impl Scratch {
    /// Borrows one node's output with local indices.
    fn emitted(&self) -> Emitted<'_> {
        Emitted {
            vertices: &self.vertices,
            indices: &self.indices,
            base: 0,
            batches: &self.batches,
        }
    }
}

/// Whether two nodes differ at most in bounds and visual.
fn same_shape(new: &UiNode, old: &UiNode) -> bool {
    new.id == old.id
        && new.parent == old.parent
        && new.focusable == old.focusable
        && new.navigation_order == old.navigation_order
        && new.clip_children == old.clip_children
        && new.world_projection == old.world_projection
}

/// The clip a node draws under: its projection's viewport, else what its parent hands down.
fn draw_clip(node: &UiNode, inherited: UiRect) -> Result<UiRect, UiError> {
    match node.world_projection {
        Some(projection) => projection
            .viewport_clip()
            .map_err(|_| UiError::InvalidWorldProjection { node: node.id }),
        None => Ok(inherited),
    }
}

/// One node's output: its vertices, its indices counted from `base`, and its own batches with
/// index ranges relative to its first index.
struct Emitted<'a> {
    vertices: &'a [UiVertex],
    indices: &'a [u32],
    base: u32,
    batches: &'a [UiDrawBatch],
}

/// Appends one node's output at the end of `list`, merging its first batch into the list's
/// last exactly as emitting in place would.
fn append(
    list: &mut UiDrawList,
    runs: &mut Vec<UiDrawBatch>,
    emitted: Emitted<'_>,
) -> Result<(), UiError> {
    let start = u32::try_from(list.vertices.len()).map_err(|_| UiError::DrawIndexOverflow)?;
    let index_base = u32::try_from(list.indices.len()).map_err(|_| UiError::DrawIndexOverflow)?;
    list.vertices.extend_from_slice(emitted.vertices);
    list.indices.extend(
        emitted
            .indices
            .iter()
            .map(|index| index - emitted.base + start),
    );
    for batch in emitted.batches {
        runs.push(batch.clone());
        let range = batch.index_range.start + index_base..batch.index_range.end + index_base;
        if let Some(last) = list.batches.last_mut()
            && batch.isolated_depth_scope.is_none()
            && last.isolated_depth_scope.is_none()
            && last.texture_page == batch.texture_page
            && last.clip == batch.clip
            && last.blend == batch.blend
            && last.depth_test == batch.depth_test
            && last.depth_write == batch.depth_write
            && last.world_projection == batch.world_projection
            && last.index_range.end == range.start
        {
            last.index_range.end = range.end;
            continue;
        }
        list.batches.push(UiDrawBatch {
            index_range: range,
            ..batch.clone()
        });
    }
    Ok(())
}

/// Rejects a list a full build could not have produced, so the caller builds it afresh.
fn check_limits(list: &UiDrawList) -> Result<(), UiError> {
    if list.vertices.len() > UiLimits::MAX_UI_VERTICES {
        return Err(UiError::VertexLimitExceeded {
            actual: list.vertices.len(),
            limit: UiLimits::MAX_UI_VERTICES,
        });
    }
    if list.indices.len() > UiLimits::MAX_UI_INDICES {
        return Err(UiError::IndexLimitExceeded {
            actual: list.indices.len(),
            limit: UiLimits::MAX_UI_INDICES,
        });
    }
    if list.batches.len() > UiLimits::MAX_DRAW_BATCHES {
        return Err(UiError::DrawBatchLimitExceeded {
            actual: list.batches.len(),
            limit: UiLimits::MAX_DRAW_BATCHES,
        });
    }
    Ok(())
}

/// `total` without one node's share; shares were added, so this cannot underflow.
fn subtract(total: DrawCounts, share: DrawCounts) -> DrawCounts {
    DrawCounts {
        quads: total.quads - share.quads,
        mesh_vertices: total.mesh_vertices - share.mesh_vertices,
        mesh_indices: total.mesh_indices - share.mesh_indices,
        mesh_batches: total.mesh_batches - share.mesh_batches,
    }
}
