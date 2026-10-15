//! Original authored fixtures cover event admission, ordering and the emitter's RNG stream.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::{
    EmptyWorld,
    atlas::ParticleAtlas,
    def::parse_effect,
    emitter::{Emitter, Outputs, SpawnRequest},
    event_budget::{EventLimits, EventOverflows},
};

/// Builds a minimal original effect while letting each test own its event and component inputs.
pub(crate) fn document(components: Value, events: Value) -> Vec<u8> {
    let mut defaults = json!({
        "minecraft:emitter_rate_instant": {"num_particles": 1},
        "minecraft:emitter_lifetime_once": {"active_time": 1},
        "minecraft:emitter_shape_point": {"direction": [1, 0, 0]},
        "minecraft:particle_lifetime_expression": {"max_lifetime": 1},
        "minecraft:particle_appearance_billboard": {"size": [0.1, 0.1]}
    });
    defaults
        .as_object_mut()
        .unwrap()
        .extend(components.as_object().unwrap().clone());
    serde_json::to_vec(&json!({"particle_effect": {
        "description": {"identifier": "test:budget", "basic_render_parameters": {
            "material": "particles_alpha", "texture": "test:fixture"}},
        "components": defaults, "events": events
    }}))
    .unwrap()
}

/// Starts an emitter without advancing its clock or consuming any creation event allowance.
fn emitter(components: Value, events: Value) -> Emitter {
    let def = Arc::new(parse_effect(&document(components, events)).unwrap());
    Emitter::new(
        1,
        def,
        ParticleAtlas::default().fallback(),
        &SpawnRequest::default(),
        7,
    )
}

/// Makes small policy allowances explicit rather than requiring a production-sized fanout.
pub(crate) fn limits(work: usize, deferred: usize, spawns: usize, sounds: usize) -> EventLimits {
    EventLimits {
        work,
        deferred,
        spawns,
        sounds,
    }
}

/// Observes the sound order exposed to the audio owner.
fn names(output: &Outputs) -> Vec<&str> {
    output.sounds.iter().map(|sound| &*sound.name).collect()
}

#[test]
fn creation_fanout_stops_before_unbounded_temporary_outputs_or_expressions() {
    let leaves: Vec<_> = (0..1000)
        .map(|index| {
            json!({
                "particle_effect": {"effect": "test:child"},
                "sound_effect": {"event_name": format!("sound{index}")},
                "expression": "v.calls = v.calls + 1;"
            })
        })
        .collect();
    let mut emitter = emitter(
        json!({
            "minecraft:emitter_rate_instant": {"num_particles": 4},
            "minecraft:particle_lifetime_events": {"creation_event": "fan"}
        }),
        json!({"fan": {"sequence": leaves}}),
    );
    let mut output = Outputs::with_limits(limits(80, 4, 2, 3));
    emitter.advance(0.01, &mut output, 100);
    assert_eq!(
        emitter.particles.len(),
        4,
        "event refusal does not cancel ordinary spawning"
    );
    assert_eq!(output.spawns.len(), 2);
    assert_eq!(output.sounds.len(), 3);
    assert!(output.budget.overflow.work > 0);
    assert!(output.budget.overflow.spawns > 0);
    assert!(output.budget.overflow.sounds > 0);
    let calls = emitter.vars[emitter.def.interner.get("calls").unwrap() as usize];
    assert!(calls > 0.0 && calls < 1000.0);
}

#[test]
fn sound_retention_keeps_the_latest_admitted_entries_in_order() {
    let sequence: Vec<_> = (0..10)
        .map(|index| {
            json!({
                "sound_effect": {"event_name": format!("sound{index}")}
            })
        })
        .collect();
    let mut emitter = emitter(json!({}), json!({"fan": {"sequence": sequence}}));
    let mut output = Outputs::with_limits(limits(100, 4, 4, 3));
    assert!(emitter.run_event("fan", [1.0; 3], [0.0; 3], &mut output));
    assert_eq!(names(&output), ["sound7", "sound8", "sound9"]);
    assert_eq!(output.budget.overflow.sounds, 7);
    assert_eq!(output.budget.overflow.work, 0);
}

#[test]
fn ordinary_sequence_preserves_expression_random_choice_and_spawn_order() {
    let mut emitter = emitter(
        json!({}),
        json!({"ordered": {"sequence": [
            {"sound_effect": {"event_name": "first"}},
            {"expression": "v.draw = math.random(10, 20);"},
            {"randomize": [
                {"weight": 1, "sound_effect": {"event_name": "a"}},
                {"weight": 2, "sound_effect": {"event_name": "b"}}
            ]},
            {"sound_effect": {"event_name": "last"}},
            {"particle_effect": {"effect": "test:child", "type": "particle_with_velocity"}}
        ]}}),
    );
    let mut expected = emitter.rng.clone();
    let draw = expected.range(10.0, 20.0);
    let choice = if expected.unit() * 3.0 < 1.0 {
        "a"
    } else {
        "b"
    };
    let mut output = Outputs::with_limits(limits(100, 4, 4, 4));
    assert!(emitter.run_event("ordered", [3.0; 3], [1.0, 2.0, 3.0], &mut output));
    assert_eq!(names(&output), ["first", choice, "last"]);
    assert_eq!(
        emitter.vars[emitter.def.interner.get("draw").unwrap() as usize],
        draw
    );
    assert_eq!(emitter.rng.next_u32(), expected.next_u32());
    assert_eq!(output.spawns.len(), 1);
    assert_eq!(output.spawns[0].effect, "test:child");
    assert_eq!(output.spawns[0].position, [3.0; 3]);
    assert_eq!(output.spawns[0].inherit_velocity, Some([1.0, 2.0, 3.0]));
    assert_eq!(output.budget.overflow, EventOverflows::default());
}

#[test]
fn random_fanout_is_refused_before_scanning_options_or_drawing_rng() {
    let options: Vec<_> = (0..1000)
        .map(|_| {
            json!({
                "weight": 1, "sound_effect": {"event_name": "unused"}
            })
        })
        .collect();
    let mut emitter = emitter(json!({}), json!({"fan": {"randomize": options}}));
    let mut expected = emitter.rng.clone();
    let mut output = Outputs::with_limits(limits(16, 4, 4, 4));
    assert!(!emitter.run_event("fan", [0.0; 3], [0.0; 3], &mut output));
    assert!(output.sounds.is_empty());
    assert_eq!(emitter.rng.next_u32(), expected.next_u32());
    assert!(output.budget.overflow.work > 0);
}

/// Two expiring particles draw during update before their deferred event expressions may draw.
fn deferred_emitter() -> Emitter {
    let mut emitter = emitter(
        json!({
            "minecraft:emitter_rate_instant": {"num_particles": 2},
            "minecraft:particle_lifetime_expression": {"max_lifetime": 0.05},
            "minecraft:particle_initialization": {"per_update_expression": "v.step = math.random(0, 1);"},
            "minecraft:particle_lifetime_events": {
                "timeline": {"0": ["first", "second"]}, "expiration_event": "last"
            }
        }),
        json!({
            "first": {"sound_effect": {"event_name": "first"}, "expression": "v.deferred = math.random(0, 1);"},
            "second": {"sound_effect": {"event_name": "second"}},
            "last": {"sound_effect": {"event_name": "last"}}
        }),
    );
    emitter.advance(0.001, &mut Outputs::default(), 100);
    emitter
}

#[test]
fn deferred_events_fire_after_all_particle_updates_in_original_order_and_rng_stream() {
    let mut emitter = deferred_emitter();
    let mut expected = emitter.rng.clone();
    for _ in &emitter.particles {
        expected.unit();
    }
    expected.unit();
    let last_event_draw = expected.unit();
    let mut output = Outputs::with_limits(limits(100, 8, 4, 8));
    emitter.update_particles(0.1, &EmptyWorld, &mut Vec::new(), &mut output);
    assert!(emitter.particles.is_empty());
    assert_eq!(
        names(&output),
        ["first", "second", "last", "first", "second", "last"]
    );
    assert_eq!(
        emitter.vars[emitter.def.interner.get("deferred").unwrap() as usize],
        last_event_draw
    );
    assert_eq!(emitter.rng.next_u32(), expected.next_u32());
    assert_eq!(output.budget.overflow, EventOverflows::default());
}

#[test]
fn deferred_capacity_is_reserved_before_queueing_and_skipped_occurrences_do_not_replay() {
    let mut emitter = deferred_emitter();
    let mut output = Outputs::with_limits(limits(100, 3, 4, 8));
    emitter.update_particles(0.1, &EmptyWorld, &mut Vec::new(), &mut output);
    assert!(emitter.particles.is_empty());
    assert_eq!(names(&output), ["first", "second", "last"]);
    assert_eq!(output.budget.overflow.deferred, 3);
    let mut next = Outputs::default();
    emitter.update_particles(0.1, &EmptyWorld, &mut Vec::new(), &mut next);
    assert!(next.sounds.is_empty());
}

#[test]
fn rejected_due_timeline_and_travel_events_advance_without_losing_future_events() {
    let mut events = serde_json::Map::new();
    for name in ["travel1", "travel2", "travel3", "time1", "time2", "future"] {
        events.insert(name.into(), json!({"sound_effect": {"event_name": name}}));
    }
    let mut emitter = emitter(
        json!({
            "minecraft:particle_lifetime_expression": {"max_lifetime": 5},
            "minecraft:particle_initial_speed": 1,
            "minecraft:particle_motion_dynamic": {},
            "minecraft:particle_lifetime_events": {
                "travel_distance_events": [
                    {"distance": 0.1, "event": "travel1"},
                    {"distance": 0.2, "event": "travel2"},
                    {"distance": 0.3, "event": "travel3"}
                ],
                "timeline": {"0.1": "time1", "0.2": "time2", "0.8": "future"}
            }
        }),
        events.into(),
    );
    emitter.advance(0.001, &mut Outputs::default(), 100);
    let mut output = Outputs::with_limits(limits(100, 1, 4, 8));
    emitter.update_particles(0.5, &EmptyWorld, &mut Vec::new(), &mut output);
    assert_eq!(names(&output), ["travel1"]);
    assert_eq!(output.budget.overflow.deferred, 4);
    assert_eq!(emitter.particles.len(), 1);
    let mut next = Outputs::default();
    emitter.update_particles(0.5, &EmptyWorld, &mut Vec::new(), &mut next);
    assert_eq!(names(&next), ["future"]);
}

struct Floor;

impl crate::ParticleWorld for Floor {
    fn solid_boxes(&self, _: [f32; 3], _: [f32; 3], out: &mut Vec<[f32; 6]>) {
        out.push([-10.0, -1.0, -10.0, 10.0, 0.0, 10.0]);
    }
    fn light(&self, _: [i32; 3]) -> (u8, u8) {
        (0, 15)
    }
    fn fluid(&self, _: [i32; 3]) -> crate::Fluid {
        crate::Fluid::None
    }
}

#[test]
fn collision_travel_loop_timeline_and_expiration_keep_their_deferred_order() {
    let mut events = serde_json::Map::new();
    for name in ["collision", "travel", "loop", "time", "end"] {
        events.insert(name.into(), json!({"sound_effect": {"event_name": name}}));
    }
    let mut emitter = emitter(
        json!({
            "minecraft:emitter_shape_point": {"offset": [0, 1, 0], "direction": [0, -1, 0]},
            "minecraft:particle_initial_speed": 4,
            "minecraft:particle_motion_dynamic": {},
            "minecraft:particle_motion_collision": {
                "collision_radius": 0.1, "expire_on_contact": true,
                "events": [{"event": "collision", "min_speed": 0}]
            },
            "minecraft:particle_lifetime_events": {
                "travel_distance_events": [{"distance": 0.5, "event": "travel"}],
                "looping_travel_distance_events": [{"distance": 0.5, "event": "loop"}],
                "timeline": {"0.1": "time"}, "expiration_event": "end"
            }
        }),
        events.into(),
    );
    emitter.advance(0.001, &mut Outputs::default(), 100);
    let mut output = Outputs::with_limits(limits(100, 8, 4, 8));
    emitter.update_particles(0.5, &Floor, &mut Vec::new(), &mut output);
    assert!(emitter.particles.is_empty());
    assert_eq!(
        names(&output),
        ["collision", "travel", "loop", "time", "end"]
    );
    assert_eq!(output.budget.overflow, EventOverflows::default());
}
