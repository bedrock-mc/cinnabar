//! Turns the server's cracking speeds into destroy stages against the client clock.

use std::collections::{HashMap, HashSet};

use chunk_pipeline::ActiveBlockCrack;
use render::{CrackInstance, CrackShape, crack_shape_from_template};

/// Caches model surfaces by runtime identity and transform, independent of column height.
pub(super) fn crack_shape(
    shapes: &mut HashMap<(u32, u32), CrackShape>,
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
    shapes
        .entry((runtime_id, transform))
        .or_insert_with(|| {
            visual
                .model_template()
                .and_then(|template| {
                    crack_shape_from_template(assets, template, visual.variant(), block)
                })
                .unwrap_or_default()
        })
        .clone()
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
                // Vanilla drops a crack once its progress completes.
                let progress = track.progress(now_seconds);
                (progress < 1.0).then(|| CrackInstance {
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
        assert_eq!(
            clock.instances(&entries, 0.0, |_| CrackShape::Cube)[0].stage,
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
        assert_eq!(
            clock
                .instances(&restarted, 100.0, |_| CrackShape::Cube)
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
        assert_eq!(
            clock.instances(&restarted, 0.6, |_| CrackShape::Cube)[0].stage,
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
}
