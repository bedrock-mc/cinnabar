//! Projects the committed crack progress onto the block's model surfaces.

use std::collections::HashMap;

use chunk_pipeline::ActiveBlockCrack;
use render::{CrackInstance, CrackShape, crack_shape_from_template};

/// Retains one column per runtime identity and transform without growing with world positions.
pub(super) struct CachedCrackShape {
    pub(super) column: Option<[i32; 2]>,
    pub(super) shape: CrackShape,
}

/// Caches model surfaces by runtime identity, transform and admitted column displacement.
pub(super) fn crack_shape(
    shapes: &mut HashMap<(u32, u32), CachedCrackShape>,
    assets: &assets::RuntimeAssets,
    mode: assets::NetworkIdMode,
    runtime_id: Option<u32>,
    block: [i32; 3],
) -> CrackShape {
    let Some(runtime_id) = runtime_id else {
        return CrackShape::Cube;
    };
    let visual = assets.resolve(mode, runtime_id);
    let transform = visual
        .model_template()
        .and_then(|template| assets.model_templates().get(template as usize))
        .map_or(visual.variant(), |template| {
            meshing::bamboo::transform_for_template(template.flags, visual.variant(), block)
        });
    let column = visual.model_template().and_then(|template| {
        has_component_offset(assets, template).then_some([block[0], block[2]])
    });
    let build = || CachedCrackShape {
        column,
        shape: visual
            .model_template()
            .and_then(|template| {
                crack_shape_from_template(assets, template, visual.variant(), block)
            })
            .unwrap_or_default(),
    };
    let cached = shapes.entry((runtime_id, transform)).or_insert_with(build);
    if cached.column != column {
        *cached = build();
    }
    cached.shape.clone()
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

/// The destroy stage (`0..=9`) for progress below completion.
pub(super) fn stage_for_progress(progress: f32) -> u8 {
    ((progress * 10.0) as u8).min(9)
}

/// Draws positive committed progress without maintaining another crack lifecycle.
pub(super) fn crack_instances(
    entries: &[ActiveBlockCrack],
    mut shape_of: impl FnMut(&ActiveBlockCrack) -> CrackShape,
) -> Vec<CrackInstance> {
    entries
        .iter()
        .filter(|entry| entry.progress > 0.0)
        .map(|entry| CrackInstance {
            block: entry.position,
            stage: stage_for_progress(entry.progress),
            shape: shape_of(entry),
        })
        .collect()
}

#[cfg(test)]
#[path = "crack_shape_tests.rs"]
mod shape_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds one committed entry at the requested visible progress.
    fn crack(progress: f32) -> ActiveBlockCrack {
        ActiveBlockCrack {
            position: [1, 2, 3],
            start_sequence: 7,
            server_value: 3_277,
            progress,
            layers: [None; world::MAX_STORAGE_COUNT],
        }
    }

    /// Zero and negative progress have no overlay until progression becomes positive.
    #[test]
    fn only_positive_progress_draws_cracks() {
        assert!(crack_instances(&[crack(0.0), crack(-0.5)], |_| CrackShape::Cube).is_empty());
        let instances = crack_instances(&[crack(0.05)], |_| CrackShape::Cube);
        assert_eq!(instances[0].block, [1, 2, 3]);
        assert_eq!(instances[0].stage, 0);
    }

    /// Each tenth of committed progress selects its corresponding atlas stage.
    #[test]
    fn committed_progress_selects_destroy_stage() {
        for (progress, stage) in [(0.05, 0), (0.5, 5), (0.95, 9)] {
            assert_eq!(
                crack_instances(&[crack(progress)], |_| CrackShape::Cube)[0].stage,
                stage
            );
        }
    }
}
