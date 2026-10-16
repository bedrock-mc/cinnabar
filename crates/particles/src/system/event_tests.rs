//! System-level allowances span emitters and are renewed only for the next simulation step.

use serde_json::json;

use super::*;
use crate::event_tests::{document, limits};
use crate::world::EmptyWorld;

#[test]
fn undrained_sounds_keep_the_latest_entries_across_bounded_step_outputs() {
    let count = MAX_QUEUED_SOUNDS * 3 / 4;
    let sequence: Vec<_> = (0..count)
        .map(|index| {
            json!({
                "sound_effect": {"event_name": format!("sound{index}")}
            })
        })
        .collect();
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(&document(
        json!({
            "minecraft:particle_lifetime_events": {"creation_event": "fan"}
        }),
        json!({"fan": {"sequence": sequence}})
    )));
    let parent = SpawnRequest {
        effect: "test:budget".into(),
        ..SpawnRequest::default()
    };
    for _ in 0..2 {
        system.spawn(&parent).unwrap();
        system.step(0.01, &EmptyWorld);
    }
    let skipped = count * 2 - MAX_QUEUED_SOUNDS;
    let expected: Vec<_> = (skipped..count)
        .chain(0..count)
        .map(|index| format!("sound{index}"))
        .collect();
    let actual: Vec<_> = system
        .take_sounds()
        .into_iter()
        .map(|sound| sound.name.into_string())
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(system.event_overflows.sounds, skipped as u64);
    assert!(system.take_sounds().is_empty());
}

#[test]
fn nested_spawn_admission_and_overflow_counts_are_shared_across_emitters() {
    let sequence: Vec<_> = (0..6)
        .map(|_| {
            json!({
                "particle_effect": {"effect": "test:child"}
            })
        })
        .collect();
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(&document(
        json!({
            "minecraft:particle_lifetime_events": {"creation_event": "fan"}
        }),
        json!({"fan": {"sequence": sequence}})
    )));
    let mut child: serde_json::Value =
        serde_json::from_slice(&document(json!({}), json!({}))).unwrap();
    child["particle_effect"]["description"]["identifier"] = json!("test:child");
    assert!(system.register_effect(&serde_json::to_vec(&child).unwrap()));
    let parent = SpawnRequest {
        effect: "test:budget".into(),
        ..SpawnRequest::default()
    };
    system.spawn(&parent).unwrap();
    system.spawn(&parent).unwrap();
    system.step_with_output(
        0.01,
        &EmptyWorld,
        Outputs::with_limits(limits(100, 8, 2, 4)),
    );
    assert_eq!(
        system.emitter_count(),
        4,
        "two admitted children join the two live parents"
    );
    assert_eq!(system.event_overflows.spawns, 10);
    assert_eq!(system.event_overflows.work, 0);
    system.spawn(&parent).unwrap();
    system.step_with_output(
        0.01,
        &EmptyWorld,
        Outputs::with_limits(limits(100, 8, 2, 4)),
    );
    assert_eq!(
        system.emitter_count(),
        7,
        "a new step admits two more children"
    );
    assert_eq!(system.event_overflows.spawns, 14);
}

#[test]
fn exhausted_work_is_shared_with_later_emitters_and_resets_for_the_next_step() {
    let sequence: Vec<_> = (0..100)
        .map(|_| json!({"expression": "v.work = v.work + 1;"}))
        .collect();
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(&document(
        json!({
            "minecraft:particle_lifetime_events": {"creation_event": "fan"}
        }),
        json!({"fan": {"sequence": sequence}})
    )));
    let mut later: serde_json::Value = serde_json::from_slice(&document(
        json!({
            "minecraft:particle_lifetime_events": {"creation_event": "mark"}
        }),
        json!({"mark": {"expression": "v.mark = 1;"}}),
    ))
    .unwrap();
    later["particle_effect"]["description"]["identifier"] = json!("test:later");
    assert!(system.register_effect(&serde_json::to_vec(&later).unwrap()));
    system
        .spawn(&SpawnRequest {
            effect: "test:budget".into(),
            ..SpawnRequest::default()
        })
        .unwrap();
    let request = SpawnRequest {
        effect: "test:later".into(),
        ..SpawnRequest::default()
    };
    let first = system.spawn(&request).unwrap();
    system.step_with_output(0.01, &EmptyWorld, Outputs::with_limits(limits(16, 8, 4, 4)));
    let emitter = system
        .emitters
        .iter()
        .find(|emitter| emitter.id == first)
        .unwrap();
    let slot = emitter.def.interner.get("mark").unwrap() as usize;
    assert!(
        emitter.vars[slot].is_nan(),
        "later emitters cannot renew the step allowance"
    );
    assert!(system.event_overflows.work > 0);
    let next = system.spawn(&request).unwrap();
    system.step_with_output(0.01, &EmptyWorld, Outputs::with_limits(limits(16, 8, 4, 4)));
    let emitter = system
        .emitters
        .iter()
        .find(|emitter| emitter.id == next)
        .unwrap();
    assert_eq!(emitter.vars[slot], 1.0);
}
