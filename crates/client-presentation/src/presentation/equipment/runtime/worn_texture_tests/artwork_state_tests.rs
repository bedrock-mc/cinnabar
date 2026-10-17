//! Unavailable artwork does not restart authored attachable scripts.
use super::*;
use actor_animation::{AttachableAnimationInput, AttachablesRuntime};

const DELTA: f32 = 0.125;

/// Compiles a persistent counter that selects available artwork on its third evaluation.
fn counter_pack() -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = coat_pack("v.elapsed = v.elapsed + q.delta_time; v.coat = v.elapsed >= 0.5;");
    let mut definition: serde_json::Value = serde_json::from_slice(&files[0].1).unwrap();
    let description = &mut definition["minecraft:attachable"]["description"];
    description["scripts"]["initialize"] = serde_json::json!(["v.elapsed = 0.125;"]);
    description["scripts"]["animate"] = serde_json::json!(["counter"]);
    description["animations"] = serde_json::json!({"counter":"animation.fixture.counter"});
    description["textures"]["plain"] = "textures/models/unavailable".into();
    files[0].1 = serde_json::to_vec(&definition).unwrap();
    files.push((
        "animations/counter.json".into(),
        serde_json::to_vec(&serde_json::json!({
            "format_version":"1.8.0", "animations":{"animation.fixture.counter":{
                "loop":true, "bones":{"head":{"position":["v.elapsed",0,0]}}
            }}
        }))
        .unwrap(),
    ));
    files
}

/// Samples the authored counter through the public runtime, leaving unavailable draws as None.
fn sample(runtime: &mut AttachablesRuntime, tick: u64) -> Option<f32> {
    let owner = owner();
    let names = [Box::<str>::from("body")];
    let rig = owner_rig(&owner, &names, tick);
    runtime
        .evaluate(
            "minecraft:iron_helmet",
            &owner,
            &rig,
            AttachableAnimationInput {
                worn: true,
                delta_seconds: Some(DELTA),
                frame_alpha: 1.0,
                ..Default::default()
            },
        )
        .map(|snapshot| snapshot.pose[0].translation_scale[0])
}

#[test]
fn unavailable_artwork_retains_initialization_and_persistent_pre_animation() {
    let (entities, _) = pack_catalog(counter_pack());
    let mut runtime = AttachablesRuntime::new(entities);
    assert_eq!(sample(&mut runtime, 1), None);
    assert_eq!(sample(&mut runtime, 2), None);
    assert_eq!(sample(&mut runtime, 3), Some(-0.5));
    assert_eq!(sample(&mut runtime, 4), Some(-0.625));
}

#[test]
fn unavailable_preview_commits_once_and_discard_preserves_the_authored_counter() {
    let (entities, _) = pack_catalog(counter_pack());
    let mut runtime = AttachablesRuntime::new(entities);
    runtime.begin_preview();
    assert_eq!(sample(&mut runtime, 1), None);
    runtime.end_preview();
    runtime.finish_preview(false);
    for tick in [1, 2] {
        runtime.begin_preview();
        assert_eq!(sample(&mut runtime, tick), None);
        runtime.end_preview();
        runtime.finish_preview(true);
        runtime.finish_preview(true);
    }
    runtime.begin_preview();
    assert_eq!(sample(&mut runtime, 3), Some(-0.5));
    runtime.end_preview();
    runtime.finish_preview(false);
    assert_eq!(sample(&mut runtime, 3), Some(-0.5));
    assert_eq!(sample(&mut runtime, 4), Some(-0.625));
}

#[test]
fn worn_missing_artwork_fallback_yields_to_the_counters_available_selection() {
    let (mut runtime, base_pages) = startup_runtime(coat_pack("v.coat=0;"));
    let body = player_body(&mut runtime);
    let (entities, catalog) = pack_catalog(counter_pack());
    let (pages, locations) =
        base_pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
    runtime.set_pack_layer(Some((entities, Some(catalog), locations)));
    let owner = owner();
    let names = [Box::<str>::from("body")];
    for (tick, expected) in [(1, PLAIN), (2, PLAIN), (3, PAINTED), (4, PAINTED)] {
        let rig = owner_rig(&owner, &names, tick);
        let drawn = runtime
            .layers_for(
                &body,
                &helmet(),
                Some(EquipmentAnimation {
                    owner: &owner,
                    rig: &rig,
                    frame_alpha: 1.0,
                    delta_seconds: DELTA,
                }),
            )
            .to_vec();
        assert_eq!(drawn.len(), 1);
        assert_eq!(selected_pixel(&pages, &drawn[0]), expected, "tick {tick}");
    }
}
