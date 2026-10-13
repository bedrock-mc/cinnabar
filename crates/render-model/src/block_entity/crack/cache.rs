//! Cached model surfaces shared by block overlays.

use super::CrackShape;
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

/// Keeps shapes only while their admitted asset identity is current.
#[derive(Default)]
pub struct CrackShapeCache {
    assets: Option<Weak<assets::RuntimeAssets>>,
    shapes: HashMap<(u32, u32, Option<u32>), CachedCrackShape>,
}

impl CrackShapeCache {
    /// Clears a session's surfaces and owner while retaining the cache's buckets.
    pub fn clear(&mut self) {
        self.shapes.clear();
        self.assets = None;
    }

    /// Reports whether any surfaces are retained for the current asset identity.
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    /// Drops surfaces when resource reload publishes a different asset generation.
    fn bind_assets(&mut self, assets: &Arc<assets::RuntimeAssets>) {
        if self
            .assets
            .as_ref()
            .is_some_and(|current| current.as_ptr() == Arc::as_ptr(assets))
        {
            return;
        }
        self.shapes.clear();
        self.assets = Some(Arc::downgrade(assets));
    }
}

/// Retains one column per runtime identity and pose without growing with world positions.
struct CachedCrackShape {
    column: Option<[i32; 2]>,
    shape: CrackShape,
}

impl CrackShapeCache {
    /// Caches world-resolved surfaces by runtime identity, pose and column displacement.
    pub fn shape(
        &mut self,
        assets: &Arc<assets::RuntimeAssets>,
        mode: assets::NetworkIdMode,
        runtime_id: Option<u32>,
        block: [i32; 3],
        resolved_template: Option<u32>,
    ) -> CrackShape {
        self.bind_assets(assets);
        let Some(runtime_id) = runtime_id else {
            return CrackShape::Cube;
        };
        let visual = assets.resolve(mode, runtime_id);
        let transform = resolved_template
            .and_then(|template| assets.model_templates().get(template as usize))
            .map_or(visual.variant(), |template| {
                assets::bamboo::transform_for_template(template.flags, visual.variant(), block)
            });
        let column = resolved_template.and_then(|template| {
            has_component_offset(assets, template).then_some([block[0], block[2]])
        });
        let build = || CachedCrackShape {
            column,
            shape: super::resolved_shape(
                assets,
                resolved_template,
                visual.model_template(),
                visual.variant(),
                block,
            )
            .unwrap_or_default(),
        };
        let cached = self
            .shapes
            .entry((runtime_id, transform, resolved_template))
            .or_insert_with(build);
        if cached.column != column {
            *cached = build();
        }
        cached.shape.clone()
    }
}

/// Checks every part because a compound surface may admit displacement after its first part.
fn has_component_offset(assets: &assets::RuntimeAssets, mut template: u32) -> bool {
    while let Some(part) = assets.model_templates().get(template as usize) {
        if assets.model_random_offset(template).is_some() {
            return true;
        }
        if part.flags & assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT == 0 {
            break;
        }
        template += 1;
    }
    false
}

#[cfg(test)]
mod tests;
