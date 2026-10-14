//! Bell swings, started once per observed ring cue and forgotten once settled.

use bevy::platform::collections::HashMap;

/// The `BlockEventPacket` type a bell ring arrives as.
pub(super) const BELL_RING_EVENT_TYPE: i32 = 1;
/// Seconds after a ring when the swing has visibly settled and its state can go.
const BELL_RING_RETAIN_SECONDS: f64 = 10.0;

/// Starts each newly observed ring once and reports its elapsed animation time.
pub(super) fn bell_elapsed(
    rings: &mut HashMap<[i32; 3], (u64, f64)>,
    position: [i32; 3],
    sequence: Option<u64>,
    now: f64,
) -> f32 {
    if let Some(sequence) = sequence {
        let entry = rings.entry(position).or_insert((sequence, now));
        if entry.0 != sequence {
            *entry = (sequence, now);
        }
    }
    rings
        .get(&position)
        .map_or(f32::INFINITY, |(_, start)| (now - start) as f32)
}

/// Keeps a latched cue's sequence so an expired swing cannot restart; evicted cues age out.
pub(super) fn prune_bell_rings(
    rings: &mut HashMap<[i32; 3], (u64, f64)>,
    now_seconds: f64,
    mut cue: impl FnMut(&[i32; 3]) -> Option<u64>,
) {
    rings.retain(|position, (sequence, start)| {
        cue(position) == Some(*sequence) || now_seconds - *start < BELL_RING_RETAIN_SECONDS
    });
}
