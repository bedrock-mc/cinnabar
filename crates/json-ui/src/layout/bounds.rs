//! Paint overflow used by both indexed placements and per-node scroll culling.

use super::{LayoutEnv, Rect, ResolvedControl, measure};
use crate::transform::Affine;
use serde_json::Value;

/// Child paint in its parent's layout coordinates, retaining authored input and child bounds.
pub(super) fn subtree(
    control: &ResolvedControl,
    rect: Rect,
    env: &LayoutEnv,
    parent: Option<Affine>,
) -> Rect {
    let own = measure::style(control)
        .transform
        .filter(|transform| !transform.is_identity())
        .map(|transform| transform.about(rect));
    let transform = match (parent, own) {
        (Some(parent), Some(own)) => Some(parent.then(&own)),
        (parent, own) => parent.or(own),
    };
    let paint = if control.control_type.as_deref() == Some("custom")
        && matches!(
            control.properties.get("renderer").and_then(Value::as_str),
            Some(crate::rounded::RENDERER | crate::vector_path::RENDERER)
        ) {
        match crate::emit::custom_draw(control) {
            Some(crate::Draw::Custom { renderer, data })
                if renderer == crate::rounded::RENDERER =>
            {
                crate::rounded::Shadow::read(&data).map_or(rect, |shadow| {
                    shadow.subtree_bounds(rect, env.text.physical_pixel_size(), transform)
                })
            }
            Some(crate::Draw::Custom { data, .. }) => crate::vector_path::bounds(&data)
                .map_or(rect, |bounds| {
                    bounds.subtree_at(rect, env.text.physical_pixel_size())
                }),
            _ => rect,
        }
    } else {
        rect
    };
    rect.union(own.map_or(paint, |own| own.bounds(paint)))
}

/// Returns the screen clip in a parent's layout coordinates; a collapsed parent cannot paint.
pub(super) fn local_clip(parent: Option<Affine>, clip: Rect) -> Option<Rect> {
    match parent {
        Some(parent) => parent.inverse().map(|inverse| inverse.bounds(clip)),
        None => Some(clip),
    }
}
