use super::*;
use crate::{
    SpawnRequest, TileRequest,
    actor::{ActorAttachment, ActorAttachmentUpdate},
    atlas::{Placement, TILE_SLOT},
    system::{
        MAX_EMITTERS, MAX_LIVE_PARTICLES, ParticleSystem, TERRAIN_EMITTER_LIMIT,
        TERRAIN_PARTICLE_LIMIT,
    },
    world::EmptyWorld,
};

/// Builds a small authored effect with caller-selected lifetime, rate and texture.
fn effect(name: &str, texture: &str, count: usize, lifetime: f32) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"particle_effect": {
        "description": {"identifier": name, "basic_render_parameters": {
            "material": "particles_alpha", "texture": texture}},
        "components": {
            "minecraft:emitter_lifetime_once": {"active_time": 1},
            "minecraft:emitter_rate_instant": {"num_particles": count},
            "minecraft:emitter_shape_point": {},
            "minecraft:particle_lifetime_expression": {"max_lifetime": lifetime},
            "minecraft:particle_appearance_billboard": {"size": [0.1, 0.1]}
        }
    }}))
    .unwrap()
}

/// Supplies deterministic tile pixels and variables without runtime game assets.
fn request(name: &str, key: u64) -> SpawnRequest {
    SpawnRequest {
        effect: name.into(),
        tile: Some(TileRequest {
            key,
            size: 1,
            pixels: vec![key as u8; 4].into(),
        }),
        seed: key,
        ..SpawnRequest::default()
    }
}

/// Independently walks entries to check both lifetimes and every observed placement.
fn assert_walk(system: &ParticleSystem, seen: &mut Vec<Placement>) {
    let mut counts = HashMap::<Box<str>, EffectCounts>::new();
    let mut particles = 0;
    for emitter in &system.emitters {
        let count = counts.entry(emitter.def.identifier.clone()).or_default();
        count.emitters += 1;
        count.particles += emitter.particles.len();
        particles += emitter.particles.len();
        if !seen.contains(&emitter.texture) {
            seen.push(emitter.texture);
        }
    }
    assert_eq!(system.bookkeeping.effects, counts);
    assert_eq!(system.live_particles(), particles);
    for &placement in seen.iter() {
        let expected = system
            .emitters
            .iter()
            .filter(|emitter| {
                emitter.texture == placement && (!emitter.done || !emitter.particles.is_empty())
            })
            .count();
        let expected = if placement.width == TILE_SLOT && placement.height == TILE_SLOT {
            expected
        } else {
            0
        };
        assert_eq!(system.atlas.placement_references(placement), expected);
    }
}

#[test]
fn mixed_lifetimes_replacements_rejections_and_eviction_match_an_emitter_walk() {
    let mut system = ParticleSystem::default();
    let mut seen = Vec::new();
    for name in ["minecraft:a", "minecraft:b"] {
        assert!(system.register_effect(&effect(name, "atlas.terrain", 4, 0.1)));
    }
    for index in 0..MAX_EMITTERS + 32 {
        let name = if index % 2 == 0 { "a" } else { "b" };
        let req = request(name, (index % 7) as u64);
        // Admission follows identifiers even when definitions are replaced in flight.
        if index % 19 == 0 {
            assert!(system.register_effect(&effect("minecraft:a", "atlas.terrain", 3, 0.05)));
        }
        let identifier = system.resolve(name).unwrap().identifier.clone();
        let old = system
            .emitters
            .iter()
            .filter(|e| e.def.identifier == identifier);
        let emitters = old.clone().count();
        let particles: usize = old.map(|e| e.particles.len()).sum();
        let allowed = emitters <= TERRAIN_EMITTER_LIMIT && particles <= TERRAIN_PARTICLE_LIMIT;
        assert_eq!(system.spawn_terrain(&req).is_some(), allowed);
        // Ordinary spawns still evict the oldest table entry at the fixed cap.
        let oldest = system.emitters.first().map(|e| e.id);
        let full = system.emitter_count() == MAX_EMITTERS;
        system.spawn(&req).unwrap();
        if full {
            assert_ne!(system.emitters.first().map(|e| e.id), oldest);
        }
        if index % 11 == 0 {
            let id = system.emitters[index % system.emitters.len()].id;
            system.stop(id);
            system.stop(id);
        }
        assert_walk(&system, &mut seen);
    }
    let dropped = system.dropped_spawns;
    assert!(system.spawn(&request("missing", 1)).is_none());
    let mut distant = request("a", 1);
    distant.position[0] = 500.0;
    assert!(system.spawn(&distant).is_none());
    distant.position[0] = f32::NAN;
    assert!(system.spawn(&distant).is_none());
    assert_eq!(system.dropped_spawns, dropped + 3);
    assert_walk(&system, &mut seen);
    for _ in 0..5 {
        system.tick(0.25, &EmptyWorld);
        assert_walk(&system, &mut seen);
    }
    assert_eq!(system.emitter_count(), 0);
    system.spawn(&request("a", 99)).unwrap();
    system.clear();
    assert_walk(&system, &mut seen);
    // Replacing the complete asset/system owner starts with no references or counts.
    system = ParticleSystem::default();
    assert_walk(&system, &mut seen);
}

#[test]
fn shared_pins_release_at_the_last_owner_and_empty_stops_release_before_tick() {
    let mut system = ParticleSystem::default();
    let mut seen = Vec::new();
    assert!(system.register_effect(&effect("minecraft:a", "atlas.terrain", 1, 0.05)));
    let first = system.spawn(&request("a", 1)).unwrap();
    let second = system.spawn(&request("a", 1)).unwrap();
    let tile = system.emitters[0].texture;
    assert_eq!(system.atlas.placement_references(tile), 2);
    system.stop(first);
    assert_eq!(system.atlas.placement_references(tile), 1);
    assert_eq!(system.bookkeeping.effect("minecraft:a").emitters, 2);
    system.tick(0.01, &EmptyWorld);
    system.stop(second);
    assert_eq!(system.atlas.placement_references(tile), 1);
    system.tick(0.1, &EmptyWorld);
    assert_eq!(system.atlas.placement_references(tile), 0);
    assert_walk(&system, &mut seen);

    let bound = system
        .spawn(&SpawnRequest {
            bound: Some((42, [0.0; 3])),
            ..request("a", 1)
        })
        .unwrap();
    system.update_bound_emitters(|_, _| None);
    assert!(system.emitters.iter().find(|e| e.id == bound).unwrap().done);
    assert_eq!(system.atlas.placement_references(tile), 0);
    system
        .spawn(&SpawnRequest {
            actor_attachment: Some(ActorAttachment {
                runtime_id: 42,
                spawn_revision: 1,
                locator: None,
                queries: Box::default(),
                entered_tick: 0,
                world_offset: [0.0; 3],
            }),
            ..request("a", 1)
        })
        .unwrap();
    system.update_actor_attachments(|_| ActorAttachmentUpdate::Pending);
    assert_eq!(system.atlas.placement_references(tile), 1);
    system.update_actor_attachments(|_| ActorAttachmentUpdate::Removed);
    assert_eq!(system.atlas.placement_references(tile), 0);
    assert_walk(&system, &mut seen);
}

#[test]
fn budget_trimming_releases_stopped_tiles_but_retains_table_counts() {
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(&effect("minecraft:a", "atlas.terrain", 512, 100.0)));
    for key in 0..MAX_LIVE_PARTICLES / 512 + 1 {
        system.spawn(&request("a", key as u64)).unwrap();
    }
    // Exercise trimming with independently admitted bursts, bypassing the tick's spawn budget.
    for emitter in &mut system.emitters {
        system
            .bookkeeping
            .edit(emitter, &mut system.atlas, |emitter| {
                emitter.advance(0.01, &mut crate::emitter::Outputs::default(), usize::MAX);
            });
    }
    let id = system.emitters[0].id;
    let tile = system.emitters[0].texture;
    system.stop(id);
    system.enforce_budget();
    assert_eq!(system.live_particles(), MAX_LIVE_PARTICLES);
    assert!(system.emitters[0].particles.is_empty());
    assert_eq!(system.atlas.placement_references(tile), 0);
    assert_eq!(
        system.bookkeeping.effect("minecraft:a").emitters,
        system.emitter_count()
    );
    assert_walk(&system, &mut Vec::new());
}

#[test]
fn zero_particle_completion_and_nested_events_leave_no_stale_accounting() {
    let mut system = ParticleSystem::default();
    let mut parent: serde_json::Value =
        serde_json::from_slice(&effect("minecraft:parent", "atlas.terrain", 1, 0.1)).unwrap();
    parent["particle_effect"]["components"]["minecraft:particle_lifetime_events"] =
        serde_json::json!({"creation_event": "child"});
    parent["particle_effect"]["events"] = serde_json::json!({"child": {
        "particle_effect": {"effect": "minecraft:child", "type": "emitter"}
    }});
    assert!(system.register_effect(&serde_json::to_vec(&parent).unwrap()));
    assert!(system.register_effect(&effect("minecraft:child", "x", 2, 0.1)));
    assert!(system.register_effect(&effect("minecraft:zero", "atlas.terrain", 0, 0.1)));
    system.spawn(&request("parent", 1)).unwrap();
    system.spawn(&request("zero", 2)).unwrap();
    system.tick(0.01, &EmptyWorld);
    assert_eq!(system.bookkeeping.effect("minecraft:child").emitters, 1);
    assert_eq!(
        system.bookkeeping.effect("minecraft:zero"),
        EffectCounts::default()
    );
    let mut seen = Vec::new();
    assert_walk(&system, &mut seen);
    system.tick(0.01, &EmptyWorld);
    assert_eq!(system.bookkeeping.effect("minecraft:child").particles, 2);
    assert_walk(&system, &mut seen);
    system.clear();
    assert_walk(&system, &mut seen);
}

#[test]
fn native_and_named_spawns_share_identifier_counts_across_replacement() {
    let native = assets::facts::ParticleEffect::BasicFlameParticle;
    let name = format!("minecraft:{}", native.name());
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(&effect(&name, "x", 4, 0.1)));
    for _ in 0..=TERRAIN_EMITTER_LIMIT {
        system
            .spawn(&SpawnRequest {
                native_effect: Some(native),
                ..Default::default()
            })
            .unwrap();
    }
    assert!(system.register_effect(&effect(&name, "x", 2, 0.1)));
    assert!(system.spawn_terrain(&request(native.name(), 1)).is_none());
    assert_walk(&system, &mut Vec::new());
}

#[test]
fn oldest_entry_releases_its_tile_before_the_replacement_resolves_pixels() {
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(&effect("minecraft:a", "atlas.terrain", 1, 1.0)));
    system.spawn(&request("a", 1)).unwrap();
    let first = system.emitters[0].texture;
    for _ in 1..MAX_EMITTERS {
        system.spawn(&request("a", 2)).unwrap();
    }
    for key in 3..=crate::atlas::MAX_SLOTS as u64 {
        system.atlas.tile(key, 1, &[9; 4]).unwrap();
    }
    system
        .spawn(&request("a", crate::atlas::MAX_SLOTS as u64 + 1))
        .unwrap();
    assert_eq!(system.emitters.last().unwrap().texture, first);
    assert_walk(&system, &mut vec![first]);
}

#[test]
fn all_tile_aliases_keep_append_order_caller_precedence_and_draw_output() {
    use crate::{ParticleView, def::TextureSource, emitter::Emitter};
    let aliases = [
        ("emitter_texture_coordinate", "emitter_texture_size"),
        ("emittertexturecoord", "emittertexturesize"),
        (
            "dig_particle_texture_coordinate",
            "dig_particle_texture_size",
        ),
        (
            "ground_particle_texture_coordinate",
            "ground_particle_texture_size",
        ),
        (
            "surface_particle_texture_coordinate",
            "surface_particle_texture_size",
        ),
    ];
    let names: Vec<String> = aliases
        .iter()
        .flat_map(|(coordinate, size)| {
            [
                format!("{coordinate}.u"),
                format!("{coordinate}.v"),
                format!("{size}.u"),
                format!("{size}.v"),
            ]
        })
        .collect();
    let mut document: serde_json::Value =
        serde_json::from_slice(&effect("minecraft:a", "atlas.terrain", 4, 1.0)).unwrap();
    let components = &mut document["particle_effect"]["components"];
    components["minecraft:emitter_initialization"] = serde_json::json!({
        "creation_expression": format!("v.sum={};", names.iter()
            .map(|name| format!("v.{name}")).collect::<Vec<_>>().join("+"))
    });
    components["minecraft:particle_appearance_billboard"]["uv"] = serde_json::json!({
        "uv": ["v.emitter_texture_coordinate.u", "v.ground_particle_texture_coordinate.v"],
        "uv_size": ["v.dig_particle_texture_size.u", "v.surface_particle_texture_size.v"]
    });
    let bytes = serde_json::to_vec(&document).unwrap();
    let mut actual = ParticleSystem::default();
    let mut expected = ParticleSystem::default();
    assert!(actual.register_effect(&bytes));
    assert!(expected.register_effect(&bytes));
    let mut actual_request = request("a", 1);
    actual_request.variables = names.iter().map(|name| (name.clone(), -9.0)).collect();
    actual_request.variables.push((names[0].clone(), -10.0));
    let mut expected_request = actual_request.clone();
    let placement = actual.resolve_texture(&TextureSource::Terrain, &mut actual_request);
    let expected_placement = expected.atlas.tile(1, 1, &[1; 4]).unwrap();
    let values = expected_placement.normalized();
    for names in names.chunks_exact(4) {
        for (name, value) in names.iter().zip(values) {
            expected_request.variables.push((name.clone(), value));
        }
    }
    assert_eq!(actual_request.variables, expected_request.variables);
    assert_eq!(actual_request.variables.len(), names.len() * 2 + 1);
    for (system, request, placement) in [
        (&mut actual, actual_request, placement),
        (&mut expected, expected_request, expected_placement),
    ] {
        let def = system.resolve("a").unwrap().clone();
        let emitter = Emitter::new(1, def, placement, &request, 17);
        for (name, value) in names.iter().zip(values.into_iter().cycle()) {
            let slot = emitter.def.interner.get(name).unwrap();
            assert_eq!(emitter.vars[usize::from(slot)], value);
        }
        system.bookkeeping.insert(&emitter, &mut system.atlas);
        system.emitters.push(emitter);
        system.tick(0.01, &EmptyWorld);
    }
    let view = ParticleView {
        position: [0.0, 0.0, 2.0],
        right: [1.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        forward: [0.0, 0.0, -1.0],
        half_diagonal: 1.0,
    };
    let a = actual.build_draw(&view, &EmptyWorld);
    let b = expected.build_draw(&view, &EmptyWorld);
    assert_eq!(a.opaque.len(), 4);
    assert_eq!(a.opaque, b.opaque);
    assert_eq!(a.blend, b.blend);
    assert_eq!(a.add, b.add);
    assert_walk(&actual, &mut Vec::new());
}

#[test]
fn absent_or_invalid_tiles_preserve_fallback_variables_and_never_pin_static_pixels() {
    use crate::def::TextureSource;
    let mut system = ParticleSystem::default();
    let fallback = system.atlas.fallback();
    for source in [TextureSource::Terrain, TextureSource::Items] {
        let mut absent = SpawnRequest {
            variables: vec![("custom".into(), 7.0)],
            ..Default::default()
        };
        assert_eq!(system.resolve_texture(&source, &mut absent), fallback);
        assert_eq!(absent.variables, [("custom".into(), 7.0)]);
        let mut invalid = request("a", 1);
        invalid.tile.as_mut().unwrap().pixels = Vec::new().into();
        assert_eq!(system.resolve_texture(&source, &mut invalid), fallback);
        let expected: Vec<_> = crate::system::TILE_VARIABLE_NAMES
            .into_iter()
            .flat_map(|names| names.into_iter().zip(fallback.normalized()))
            .map(|(name, value)| (name.to_owned(), value))
            .collect();
        assert_eq!(invalid.variables, expected);
        assert_eq!(system.atlas.placement_references(fallback), 0);
    }
}
