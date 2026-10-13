//! Projects committed crack progress onto model surfaces.

use chunk_pipeline::ActiveBlockCrack;
use render_model::{CrackInstance, CrackShape};

/// The destroy stage (`0..=9`) for progress below completion.
pub(super) fn stage_for_progress(progress: f32) -> u8 {
    ((progress * 10.0) as u8).min(9)
}

/// The block layers stored at a crack position.
pub(super) type CrackLayers = [Option<u32>; world::MAX_STORAGE_COUNT];

/// Each crack plus the other block of its two-block structure, as vanilla cracks both halves of
/// a door or bed together. `second_part` gives the other block's position and layers; a position
/// that already has its own crack keeps it.
pub(super) fn with_second_parts(
    entries: &[ActiveBlockCrack],
    mut second_part: impl FnMut(&ActiveBlockCrack) -> Option<([i32; 3], CrackLayers)>,
) -> Vec<ActiveBlockCrack> {
    let mut expanded = entries.to_vec();
    for entry in entries {
        let Some((position, layers)) = second_part(entry) else {
            continue;
        };
        if expanded.iter().all(|other| other.position != position) {
            expanded.push(ActiveBlockCrack {
                position,
                layers,
                ..entry.clone()
            });
        }
    }
    expanded
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

    /// One layer-zero block id, as stored for a crack position.
    fn layers(id: u32) -> CrackLayers {
        std::array::from_fn(|layer| (layer == 0).then_some(id))
    }

    #[test]
    fn the_other_half_of_a_door_cracks_with_the_hit_half() {
        let lower = ActiveBlockCrack {
            layers: layers(7),
            ..crack(0.4)
        };
        let upper = [lower.position[0], lower.position[1] + 1, lower.position[2]];
        let expanded = with_second_parts(std::slice::from_ref(&lower), |entry| {
            (entry.layers[0] == Some(7)).then_some((upper, layers(8)))
        });
        assert_eq!(expanded.len(), 2);
        assert_eq!(expanded[1].position, upper);
        assert_eq!(
            expanded[1].progress, lower.progress,
            "both halves crack together"
        );
        assert_eq!(expanded[1].layers, layers(8));
        let both = with_second_parts(&[lower.clone(), expanded[1].clone()], |entry| {
            Some((
                if entry.position == upper {
                    lower.position
                } else {
                    upper
                },
                entry.layers,
            ))
        });
        assert_eq!(both.len(), 2, "a half with its own crack is not doubled");
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
