//! Replays already painted controls without re-entering the engine's texture or animation locks.

mod cached;
pub use cached::CachedFragment;

use std::{collections::BTreeMap, sync::Arc};

use ui::{
    UiBlendMode, UiError, UiLimits, UiMesh, UiMeshBatch, UiMeshVertex, UiNode, UiNodeId,
    UiTransform, UiTree, UiVisual,
};

use super::{Canvas, rect, screen::PaintError};

/// Measures already painted artwork in its actual source viewport and translates it to
/// a zero origin for [`Canvas::paint_fragment`]. Returns its size in logical pixels, or
/// `None` for empty/unsupported filter artwork. Layout and all node-local transforms stay
/// intact; only root positions and absolute clips move. Retained draw budgets also apply.
pub fn normalize_fragment(
    nodes: &mut [UiNode],
    viewport: [f32; 4],
) -> Result<Option<[f32; 2]>, PaintError> {
    if nodes.iter().any(filtered) {
        return Ok(None);
    }
    let retained = ui::RetainedDraw::build(
        nodes,
        rect(viewport[0], viewport[1], viewport[2], viewport[3])?,
        ui::UiScale::default(),
        ui::SafeArea::ZERO,
        ui::TextEffects::default(),
    )
    .map_err(PaintError::Tree)?;
    let Some(bounds) = retained.draw_list().painted_bounds() else {
        return Ok(None);
    };
    let delta = [-bounds.min().x(), -bounds.min().y()];
    let translated = nodes
        .iter()
        .map(|node| {
            let mut moved = node.clone();
            if node.parent().is_none() {
                moved = moved.with_bounds(
                    node.bounds()
                        .translated(delta)
                        .map_err(PaintError::Geometry)?,
                );
            }
            if let Some(clip) = node.viewport_clip() {
                moved =
                    moved.with_viewport_clip(clip.translated(delta).map_err(PaintError::Geometry)?);
            }
            Ok(moved)
        })
        .collect::<Result<Vec<_>, PaintError>>()?;
    nodes.clone_from_slice(&translated);
    Ok(Some([bounds.width(), bounds.height()]))
}

impl Canvas<'_> {
    /// Contains a prepainted fragment inside `dest` ([left, top, right, bottom], logical px).
    /// Source nodes use a zero-origin `source_size` and retain their tree's paint order. Artwork and
    /// node-local transforms survive; input and world projection do not. Each visual gets a
    /// fresh identity and a clip intersected with the destination and this canvas's clip.
    /// Empty/non-finite extents draw nothing. Filtered/backdrop fragments draw nothing as a
    /// whole: nested filter composition is not supported. Existing node budgets also bound
    /// replay. O(nodes log nodes + faded mesh vertices), with mesh copies shared per replay.
    pub fn paint_fragment(
        &mut self,
        nodes: &[UiNode],
        source_size: [f32; 2],
        dest: [f32; 4],
        opacity: f32,
    ) -> Result<(), PaintError> {
        let Some(clip) = self.clip() else {
            return Ok(());
        };
        // A pending canvas map moves the destination as well as its artwork. Apply the
        // fixed viewport clip only after that map, so entering content is not cut early.
        let source_clip = if self.residual.is_some() { dest } else { clip };
        let (mut painted, next) = replay(
            nodes,
            source_size,
            dest,
            source_clip,
            opacity,
            *self.next,
            self.nodes.len(),
        )?;
        if let Some(transform) = self.residual {
            move_clips(&mut painted, transform, clip)?;
        }
        self.nodes.extend(painted);
        *self.next = next;
        Ok(())
    }
}

/// Moves replay's private scissors while keeping each leaf's pre-transform absolute origin.
/// The enclosing canvas subsequently composes its map into those leaves' paint transforms.
fn move_clips(
    nodes: &mut [UiNode],
    transform: json_ui::transform::Affine,
    viewport: [f32; 4],
) -> Result<(), PaintError> {
    for pair in nodes.as_chunks_mut::<2>().0 {
        let previous = pair[0].bounds();
        let moved = transform.bounds(json_ui::Rect::new(
            f64::from(previous.min().x()),
            f64::from(previous.min().y()),
            f64::from(previous.max().x() - previous.min().x()),
            f64::from(previous.max().y() - previous.min().y()),
        ));
        let clipped = intersect(
            [
                moved.x as f32,
                moved.y as f32,
                (moved.x + moved.w) as f32,
                (moved.y + moved.h) as f32,
            ],
            viewport,
        );
        let group = rect(clipped[0], clipped[1], clipped[2], clipped[3])?;
        let delta = [
            previous.min().x() - clipped[0],
            previous.min().y() - clipped[1],
        ];
        let bounds = edges(pair[1].bounds());
        pair[1] = pair[1].clone().with_bounds(rect(
            bounds[0] + delta[0],
            bounds[1] + delta[1],
            bounds[2] + delta[0],
            bounds[3] + delta[1],
        )?);
        pair[0] = pair[0].clone().with_bounds(group);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Placement {
    origin: [f32; 2],
    children_clip: [f32; 4],
}

/// Intersects absolute rectangles without making inverted rectangles.
fn intersect(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let left = a[0].max(b[0]);
    let top = a[1].max(b[1]);
    [left, top, a[2].min(b[2]).max(left), a[3].min(b[3]).max(top)]
}

/// Reads a model rectangle as absolute edge coordinates.
fn edges(bounds: ui::UiRect) -> [f32; 4] {
    [
        bounds.min().x(),
        bounds.min().y(),
        bounds.max().x(),
        bounds.max().y(),
    ]
}

/// Rejects filter packets before replay can change the caller's output or filter scope.
fn filtered(node: &UiNode) -> bool {
    node.filter_group().is_some()
        || matches!(node.visual(), UiVisual::Mesh(mesh) if mesh.batches().iter().any(|batch|
            matches!(batch.blend, UiBlendMode::Filter | UiBlendMode::Backdrop)))
}

/// Builds a complete replay before committing it, so malformed trees never leave partial paint.
#[allow(clippy::too_many_arguments)]
fn replay(
    nodes: &[UiNode],
    source_size: [f32; 2],
    dest: [f32; 4],
    outer_clip: [f32; 4],
    opacity: f32,
    mut next: u32,
    existing: usize,
) -> Result<(Vec<UiNode>, u32), PaintError> {
    if nodes.len() > UiLimits::MAX_NODES {
        return Err(PaintError::Tree(UiError::NodeLimitExceeded {
            actual: nodes.len(),
            limit: UiLimits::MAX_NODES,
        }));
    }
    if source_size
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
        || dest
            .iter()
            .chain(outer_clip.iter())
            .any(|value| !value.is_finite())
        || dest[2] <= dest[0]
        || dest[3] <= dest[1]
        || !opacity.is_finite()
        || opacity <= 0.0
        || nodes.iter().any(filtered)
    {
        return Ok((Vec::new(), next));
    }
    let clip = intersect(dest, outer_clip);
    if clip[2] <= clip[0] || clip[3] <= clip[1] {
        return Ok((Vec::new(), next));
    }
    let scale = ((dest[2] - dest[0]) / source_size[0]).min((dest[3] - dest[1]) / source_size[1]);
    if !scale.is_finite() || scale <= 0.0 {
        return Ok((Vec::new(), next));
    }
    let offset = [
        dest[0] + ((dest[2] - dest[0]) - source_size[0] * scale) * 0.5,
        dest[1] + ((dest[3] - dest[1]) - source_size[1] * scale) * 0.5,
    ];
    let map_rect = |source: [f32; 4]| {
        [
            offset[0] + source[0] * scale,
            offset[1] + source[1] * scale,
            offset[0] + source[2] * scale,
            offset[1] + source[3] * scale,
        ]
    };
    let initial = Placement {
        origin: [0.0; 2],
        children_clip: [0.0, 0.0, source_size[0], source_size[1]],
    };
    let mut placements = BTreeMap::<UiNodeId, Placement>::new();
    // The tree owns retained paint order and structural validation. Skeletons deliberately
    // omit the input and world-projection metadata that this replay does not carry forward.
    let tree = UiTree::new(
        nodes
            .iter()
            .map(|node| UiNode::new(node.id(), node.parent(), node.bounds()))
            .collect(),
    )
    .map_err(PaintError::Tree)?;
    let originals: BTreeMap<_, _> = nodes.iter().map(|node| (node.id(), node)).collect();
    let mut meshes = BTreeMap::new();
    let mut output = Vec::new();
    for ordered in tree.paint_order() {
        let node = originals[&ordered.id()];
        let parent = match node.parent() {
            Some(parent) => {
                *placements
                    .get(&parent)
                    .ok_or(PaintError::Tree(UiError::MissingParent {
                        node: node.id(),
                        parent,
                    }))?
            }
            None => initial,
        };
        let bounds = edges(node.bounds());
        let origin = [parent.origin[0] + bounds[0], parent.origin[1] + bounds[1]];
        let own = [
            origin[0],
            origin[1],
            parent.origin[0] + bounds[2],
            parent.origin[1] + bounds[3],
        ];
        let inherited = node.viewport_clip().map_or(parent.children_clip, edges);
        let placement = Placement {
            origin,
            children_clip: if node.clips_children() {
                intersect(inherited, own)
            } else {
                inherited
            },
        };
        if placements.insert(node.id(), placement).is_some() {
            return Err(PaintError::Tree(UiError::DuplicateNodeId { id: node.id() }));
        }
        if matches!(node.visual(), UiVisual::None) {
            continue;
        }
        let clipped = intersect(clip, map_rect(inherited));
        if clipped[2] <= clipped[0] || clipped[3] <= clipped[1] {
            continue;
        }
        let count = existing.saturating_add(output.len()).saturating_add(2);
        if count > UiLimits::MAX_NODES {
            return Err(PaintError::Tree(UiError::NodeLimitExceeded {
                actual: count,
                limit: UiLimits::MAX_NODES,
            }));
        }
        let after = next
            .checked_add(2)
            .ok_or(PaintError::Tree(UiError::DrawIndexOverflow))?;
        let group = UiNodeId::new(next);
        output.push(
            UiNode::new(
                group,
                None,
                rect(clipped[0], clipped[1], clipped[2], clipped[3])?,
            )
            .with_clip_children(true),
        );
        let at = [offset[0] + origin[0] * scale, offset[1] + origin[1] * scale];
        let previous = node.transform().copied().unwrap_or(UiTransform {
            linear: [1.0, 0.0, 0.0, 1.0],
            translation: [0.0; 2],
        });
        output.push(
            UiNode::new(
                UiNodeId::new(next + 1),
                Some(group),
                rect(
                    at[0] - clipped[0],
                    at[1] - clipped[1],
                    at[0] - clipped[0] + bounds[2] - bounds[0],
                    at[1] - clipped[1] + bounds[3] - bounds[1],
                )?,
            )
            .with_visual(faded(node.visual(), opacity.min(1.0), &mut meshes))
            .with_transform(UiTransform {
                linear: previous.linear.map(|value| value * scale),
                translation: previous.translation.map(|value| value * scale),
            }),
        );
        next = after;
    }
    Ok((output, next))
}

/// Fades every visual kind while keeping immutable text and unchanged mesh artwork shared.
fn faded(visual: &UiVisual, opacity: f32, meshes: &mut BTreeMap<usize, Arc<UiMesh>>) -> UiVisual {
    let mut visual = visual.clone();
    if opacity == 1.0 {
        return visual;
    }
    let alpha = |color: &mut [u8; 4]| {
        color[3] = (f32::from(color[3]) * opacity).round() as u8;
    };
    match &mut visual {
        UiVisual::None => {}
        UiVisual::Mesh(mesh) => {
            *mesh = Arc::clone(
                meshes
                    .entry(Arc::as_ptr(mesh) as usize)
                    .or_insert_with(|| Arc::new((**mesh).clone().with_opacity(opacity))),
            );
        }
        UiVisual::Solid { color, .. }
        | UiVisual::Sprite { color, .. }
        | UiVisual::RotatedSprite { color, .. }
        | UiVisual::GlintSprite { color, .. }
        | UiVisual::StyledSprite { color, .. }
        | UiVisual::RepeatedSprite { color, .. }
        | UiVisual::Text { color, .. }
        | UiVisual::RotatedText { color, .. } => alpha(color),
        UiVisual::Gradient { colors, .. } => colors.iter_mut().for_each(alpha),
        UiVisual::InvertedSprite { texture_page, uv } => {
            let [left, top, right, bottom] = uv.map(f32::from);
            let mut color = [255; 4];
            alpha(&mut color);
            let vertices: Vec<_> = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
                .map(|position| UiMeshVertex {
                    position,
                    clip_z: 0.0,
                    clip_w: 1.0,
                    uv: [
                        left + position[0] * (right - left),
                        top + position[1] * (bottom - top),
                    ],
                    color,
                    model_light: 1.0,
                    overlay_color: [0.0; 4],
                    style_flags: 0,
                    alpha_test: false,
                })
                .into();
            visual = UiMesh::new(
                vertices.into(),
                Arc::from([0, 1, 2, 0, 2, 3]),
                Arc::from([UiMeshBatch {
                    texture_page: *texture_page,
                    index_range: 0..6,
                    blend: UiBlendMode::Invert,
                    depth_test: false,
                    depth_write: false,
                    alpha_cutoff: None,
                }]),
            )
            .map_or(UiVisual::None, |mesh| UiVisual::Mesh(Arc::new(mesh)));
        }
    }
    visual
}

#[cfg(test)]
mod tests;
