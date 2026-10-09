//! Turns the server's cracking speeds into destroy stages against the client clock.

use std::collections::{HashMap, HashSet};

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

/// Server progress units for a fully broken block.
const PROGRESS_UNITS: f32 = 65_535.0;
const TICKS_PER_SECOND: f64 = 20.0;
const STAGES: f32 = 10.0;

#[derive(Debug)]
struct Track {
    start_sequence: u64,
    rate_per_tick: u16,
    /// Progress accumulated before the current rate took effect, `0.0..=1.0`.
    base_progress: f32,
    rate_since_seconds: f64,
}

impl Track {
    fn progress(&self, now_seconds: f64) -> f32 {
        let ticks = (now_seconds - self.rate_since_seconds).max(0.0) * TICKS_PER_SECOND;
        (self.base_progress + ticks as f32 * f32::from(self.rate_per_tick) / PROGRESS_UNITS)
            .clamp(0.0, 1.0)
    }
}

/// Client-side progress per cracked block; the server value is a speed, not a clock.
#[derive(Debug, Default)]
pub(super) struct CrackClock {
    tracks: HashMap<[i32; 3], Track>,
}

/// The destroy stage (`0..=9`) for `progress` in `0.0..=1.0`.
pub(super) fn stage_for_progress(progress: f32) -> u8 {
    ((progress * STAGES).floor().max(0.0) as u8).min(9)
}

impl CrackClock {
    pub(super) fn instances(
        &mut self,
        entries: &[ActiveBlockCrack],
        now_seconds: f64,
        mut shape_of: impl FnMut(&ActiveBlockCrack) -> CrackShape,
    ) -> Vec<CrackInstance> {
        let live = entries
            .iter()
            .map(|entry| entry.position)
            .collect::<HashSet<_>>();
        self.tracks.retain(|position, _| live.contains(position));
        entries
            .iter()
            .filter_map(|entry| {
                let track = self.tracks.entry(entry.position).or_insert(Track {
                    start_sequence: entry.start_sequence,
                    rate_per_tick: entry.server_value,
                    base_progress: 0.0,
                    rate_since_seconds: now_seconds,
                });
                if track.start_sequence != entry.start_sequence {
                    *track = Track {
                        start_sequence: entry.start_sequence,
                        rate_per_tick: entry.server_value,
                        base_progress: 0.0,
                        rate_since_seconds: now_seconds,
                    };
                } else if track.rate_per_tick != entry.server_value {
                    track.base_progress = track.progress(now_seconds);
                    track.rate_per_tick = entry.server_value;
                    track.rate_since_seconds = now_seconds;
                }
                // Cracks become visible after progress begins and disappear at completion.
                let progress = track.progress(now_seconds);
                (progress > 0.0 && progress < 1.0).then(|| CrackInstance {
                    block: entry.position,
                    stage: stage_for_progress(progress),
                    shape: shape_of(entry),
                })
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "crack_shape_tests.rs"]
mod shape_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn crack(position: [i32; 3], sequence: u64, rate: u16) -> ActiveBlockCrack {
        ActiveBlockCrack {
            position,
            start_sequence: sequence,
            server_value: rate,
            layers: [None; world::MAX_STORAGE_COUNT],
        }
    }

    #[test]
    fn stages_advance_with_the_client_clock() {
        let mut clock = CrackClock::default();
        // 1/20 of the block per tick: half done after ten ticks (half a second).
        let entries = [crack([1, 2, 3], 7, 3_277)];
        assert!(
            clock
                .instances(&entries, 0.0, |_| CrackShape::Cube)
                .is_empty()
        );
        assert_eq!(
            clock.instances(&entries, 0.01, |_| CrackShape::Cube)[0].stage,
            0
        );
        assert_eq!(
            clock.instances(&entries, 0.5, |_| CrackShape::Cube)[0].stage,
            5
        );
        assert_eq!(
            clock.instances(&entries, 0.95, |_| CrackShape::Cube)[0].stage,
            9
        );
    }

    /// A completed crack stops rendering instead of holding stage 9 indefinitely.
    #[test]
    fn completed_cracks_stop_rendering_until_restarted() {
        let mut clock = CrackClock::default();
        let entries = [crack([1, 2, 3], 7, 3_277)];
        clock.instances(&entries, 0.0, |_| CrackShape::Cube);
        assert!(
            clock
                .instances(&entries, 100.0, |_| CrackShape::Cube)
                .is_empty()
        );
        let restarted = [crack([1, 2, 3], 8, 3_277)];
        assert!(
            clock
                .instances(&restarted, 100.0, |_| CrackShape::Cube)
                .is_empty()
        );
        assert_eq!(
            clock
                .instances(&restarted, 100.01, |_| CrackShape::Cube)
                .len(),
            1
        );
    }

    #[test]
    fn rate_changes_keep_progress_and_new_starts_reset_it() {
        let mut clock = CrackClock::default();
        let slow = [crack([0; 3], 1, 3_277)];
        clock.instances(&slow, 0.0, |_| CrackShape::Cube);
        assert_eq!(
            clock.instances(&slow, 0.5, |_| CrackShape::Cube)[0].stage,
            5
        );
        let faster = [crack([0; 3], 1, 6_554)];
        assert_eq!(
            clock.instances(&faster, 0.5, |_| CrackShape::Cube)[0].stage,
            5
        );
        let restarted = [crack([0; 3], 2, 3_277)];
        assert!(
            clock
                .instances(&restarted, 0.6, |_| CrackShape::Cube)
                .is_empty()
        );
        assert_eq!(
            clock.instances(&restarted, 0.61, |_| CrackShape::Cube)[0].stage,
            0
        );
    }

    #[test]
    fn stopped_cracks_are_forgotten() {
        let mut clock = CrackClock::default();
        clock.instances(&[crack([5; 3], 1, 100)], 0.0, |_| CrackShape::Cube);
        clock.instances(&[], 1.0, |_| CrackShape::Cube);
        assert!(clock.tracks.is_empty());
    }

    /// A zero-speed start has no visible progress; updates preserve a paused stage.
    #[test]
    fn stationary_cracks_wait_for_progress_and_pause_without_resetting() {
        let mut clock = CrackClock::default();
        let stationary = [crack([1, 2, 3], 7, 0)];
        assert!(
            clock
                .instances(&stationary, 0.0, |_| CrackShape::Cube)
                .is_empty()
        );
        assert!(
            clock
                .instances(&stationary, 5.0, |_| CrackShape::Cube)
                .is_empty()
        );
        let moving = [crack([1, 2, 3], 7, 3_277)];
        assert!(
            clock
                .instances(&moving, 5.0, |_| CrackShape::Cube)
                .is_empty()
        );
        assert_eq!(
            clock.instances(&moving, 5.5, |_| CrackShape::Cube)[0].stage,
            5
        );
        assert_eq!(
            clock.instances(&stationary, 5.5, |_| CrackShape::Cube)[0].stage,
            5
        );
        assert_eq!(
            clock.instances(&stationary, 10.0, |_| CrackShape::Cube)[0].stage,
            5
        );
        assert_eq!(
            clock.instances(&moving, 10.0, |_| CrackShape::Cube)[0].stage,
            5
        );
        assert_eq!(
            clock.instances(&moving, 10.4, |_| CrackShape::Cube)[0].stage,
            9
        );
        assert!(
            clock
                .instances(&moving, 10.5, |_| CrackShape::Cube)
                .is_empty()
        );
    }
}
