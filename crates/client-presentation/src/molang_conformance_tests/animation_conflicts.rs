//! Compiled controller conflicts must not escape through animated-face activation.
use super::*;
use actor_animation::{ActorAnimationStore, ActorTickContext};
use render_api::{SkinAnimation, SkinAnimationKind, SkinGeometrySource};
use std::collections::HashMap;

/// Builds a drawable player whose blink alias can be activated explicitly or by its skin.
fn blink_pack(declared: bool, conflicting: bool) -> Pack {
    let controller = |blinking| {
        serde_json::json!({
            "format_version":"1.10.0", "animation_controllers":{
                "controller.animation.persona.blink":{"initial_state":"default", "states":{
                    "default":{"transitions":[{"blink":"1"}]},
                    "blink":{"on_entry":[format!("variable.is_blinking = {blinking};")]}
                }}
            }
        })
    };
    let mut files = vec![
        (
            "entity/player.entity.json",
            entity_file(
                "player",
                serde_json::json!({
                    "initialize":["variable.body_scale = 1.25;"],
                    "pre_animation":["variable.body_scale = 2;"],
                    "scale":"variable.body_scale",
                    "animate": if declared { vec!["blink"] } else { vec![] }
                }),
                serde_json::json!({"blink":"controller.animation.persona.blink"}),
            ),
        ),
        (
            "models/entity/player.geo.json",
            geometry_file(
                "player",
                vec![serde_json::json!({"name":"body", "pivot":[0,0,0],
                "cubes":[{"origin":[-4,0,-2],"size":[8,12,4],"uv":[0,0]}]})],
            ),
        ),
        ("render_controllers/player.json", render_file("player")),
        (
            "animations/empty.json",
            serde_json::json!({"format_version":"1.8.0", "animations":{}}),
        ),
        ("animation_controllers/blink.json", controller(1)),
    ];
    if conflicting {
        files.push(("animation_controllers/duplicate.json", controller(2)));
    }
    write_pack("player", files)
}

/// Uses an original two-frame face atlas with a matching body and face skeleton.
fn animated_face() -> Arc<SkinGeometrySource> {
    let geometry = geometry_file(
        "player",
        vec![serde_json::json!({
            "name":"body", "pivot":[0,0,0],
            "cubes":[{"origin":[-4,0,-2],"size":[8,12,4],"uv":[0,0]}]
        })],
    );
    Arc::new(SkinGeometrySource {
        resource_patch:
            r#"{"geometry":{"default":"geometry.player","animated_face":"geometry.player"}}"#.into(),
        geometry_data: geometry.to_string().into(),
        animations: Arc::from([SkinAnimation {
            kind: SkinAnimationKind::Face,
            width: 16,
            height: 32,
            rgba8: vec![255; 16 * 32 * 4].into(),
            frames: 2,
            blinking: true,
        }]),
    })
}

/// Compiles the pack and advances a player with its prepared animated-face appearance.
fn tick_blink_pack(pack: &Pack) -> ActorAnimationStore {
    let (world, entities) = spawned("player", pack);
    let mut actor = world.authority().actor(5).unwrap().clone();
    actor.kind = ActorKind::Player {
        uuid: [0; 16],
        username: "fixture".into(),
    };
    let mut store = ActorAnimationStore::with_assets(entities);
    store.insert(1, 0, &actor);
    let source = animated_face();
    store.request_skin_preparation(&source);
    store.submit_skin_preparation();
    store.finish_skin_fixture_batch();
    store.sync_skin_model(5, Some(&source));
    let actors = HashMap::from([(5, actor)]);
    for _ in 0..2 {
        store.advance_tick(&actors, None, None, true, false, |_| ActorTickContext {
            skin_geometry: Some(Arc::clone(&source)),
            ..Default::default()
        });
    }
    store
}

#[test]
fn conflicting_blink_controllers_never_run_for_an_animated_face() {
    for declared in [false, true] {
        let store = tick_blink_pack(&blink_pack(declared, true));
        let rig = store.get(5).unwrap();
        assert!(rig.pose_evaluated);
        assert!(rig.skin_mesh.is_some());
        assert!(!rig.render.is_empty());
        assert_eq!(rig.current, rig.rest);
        assert_eq!(rig.scale, 2.0, "the body scripts still run");
        assert_eq!(rig.skin_layers.len(), 1);
        assert_eq!(
            rig.skin_layers[0].uv_anim[1], 0.0,
            "a conflicting controller ran through the animated-face lookup; declared={declared}"
        );
    }
}

#[test]
fn unambiguous_blink_controller_still_runs_for_an_animated_face() {
    let store = tick_blink_pack(&blink_pack(false, false));
    let rig = store.get(5).unwrap();
    assert!(rig.pose_evaluated);
    assert_eq!(rig.skin_layers.len(), 1);
    assert_eq!(rig.skin_layers[0].uv_anim[1], 0.5);
}
