//! Env-gated pinned-vanilla mob motion replay through the production native GPU actor pass.
//! PNGs and a pose trace stay in the caller's scratch directory; no server/world is touched.
use std::{path::Path, sync::Arc};

use assets::RuntimeEntityAssets;
use bevy::prelude::*;
use chunk_pipeline::WorldStream;
use client_world::BoneTransform;
use protocol::{
    ActorEvent, ActorKind, ActorMoveEvent, ActorPositionOrigin, ActorSpawnEvent, WorldEvent,
};
use render::{ActorArtworkPages, ActorRenderFrame, ActorRenderScene};

use super::render_report::world_for;

mod gait;
mod gpu;

const MOB_IDS: [u64; 3] = [42, 43, 44];
const SPECIES: [&str; 3] = ["minecraft:cow", "minecraft:chicken", "minecraft:bat"];
const INITIAL: [[f32; 3]; 3] = [[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.8, -0.3]];
const STATIONARY_TICKS: u32 = 120;
const WALKING_TICKS: u32 = 60;
const SLOW_STEP: f32 = 0.025;
const FAST_STEP: f32 = 0.1;
const SHEET_COLUMNS: u32 = 4;

struct Vanilla {
    entities: Arc<RuntimeEntityAssets>,
    artwork: ActorArtworkPages,
    candidates: Vec<u32>,
}

#[derive(serde::Serialize)]
struct PoseSample {
    scenario: String,
    tick: u32,
    runtime_id: u64,
    position: [f32; 3],
    bones: Vec<(String, [f32; 4])>,
}

/// Set `CINNABAR_MOB_MOTION_PACK` to the pinned vanilla `resource_pack` directory and
/// `CINNABAR_MOB_MOTION_OUT` to a scratch directory. This intentionally requires a native GPU.
#[test]
fn vanilla_mob_motion_on_native_gpu() {
    let (Some(pack), Some(out)) = (
        std::env::var_os("CINNABAR_MOB_MOTION_PACK"),
        std::env::var_os("CINNABAR_MOB_MOTION_OUT"),
    ) else {
        return;
    };
    let vanilla = compile(Path::new(&pack));
    gait::verify_source(Path::new(&pack));
    let out = Path::new(&out);
    std::fs::create_dir_all(out).unwrap();
    let (mut app, camera) = gpu::app();
    let mut frames = Vec::new();
    let mut trace = Vec::new();
    let mut gait_crossings = Vec::new();
    for (scenario, step) in [
        ("stationary", 0.0),
        ("slow", SLOW_STEP),
        ("fast", FAST_STEP),
    ] {
        let mut world = spawn(&vanilla);
        let mut scene = ActorRenderScene::default();
        scene
            .replace_pack_entities(Some(&vanilla.entities))
            .unwrap();
        scene.configure_artwork(vanilla.artwork.clone());
        let mut baseline = None;
        let mut wing = None;
        let mut wing_changed = false;
        let mut moving_legs: Option<[Vec<BoneTransform>; 2]> = None;
        let mut legs_changed = [false; 2];
        let mut previous_leg_x: Option<[f32; 2]> = None;
        let mut crossings = [0_u32; 2];
        for tick in 1..=STATIONARY_TICKS {
            let travel = if step == 0.0 {
                0.0
            } else {
                step * tick.min(WALKING_TICKS) as f32
            };
            if step != 0.0 && tick <= WALKING_TICKS {
                move_mobs(&mut world, tick, travel);
            }
            world.advance_actor_interpolation_frame(1);
            let legs = gait::verify(&world);
            let current_wing = selected_bones(&world, MOB_IDS[2], "wing");
            if let Some(previous) = &wing {
                wing_changed |= previous != &current_wing;
            }
            wing = Some(current_wing);
            if step == 0.0 {
                if let Some(baseline) = &baseline {
                    assert_eq!(&legs, baseline, "stationary legs must hold for six seconds");
                } else {
                    baseline = Some(legs.clone());
                }
            } else if tick <= WALKING_TICKS {
                if let Some(previous) = &moving_legs {
                    for (index, current) in legs.iter().enumerate() {
                        legs_changed[index] |= previous[index] != *current;
                    }
                }
                let current_x = std::array::from_fn(|index| legs[index][0].rotation[0]);
                if let Some(previous_x) = previous_leg_x {
                    for index in 0..crossings.len() {
                        crossings[index] += u32::from(previous_x[index] * current_x[index] < 0.0);
                    }
                }
                previous_leg_x = Some(current_x);
                moving_legs = Some(legs);
            }
            publish(&mut app, &mut scene, &world, &vanilla.artwork);
            *app.world_mut().get_mut::<Transform>(camera).unwrap() = gpu::camera_transform(travel);
            gpu::update(&mut app);
            let capture = if step == 0.0 {
                matches!(tick, 1 | 4 | STATIONARY_TICKS)
            } else {
                matches!(tick, 15 | 30 | 45 | WALKING_TICKS | STATIONARY_TICKS)
            };
            if capture {
                let filename = format!("{:02}_{scenario}_{tick:03}.png", frames.len());
                let image = gpu::capture(&mut app);
                let background = image.get_pixel(0, 0);
                let drawn = image.pixels().filter(|pixel| *pixel != background).count();
                assert!(
                    drawn > image.as_raw().len() / 2_000,
                    "{filename}: native GPU frame contains visible actor geometry"
                );
                image.save(out.join(&filename)).unwrap();
                frames.push((filename, image));
                trace.extend(samples(&world, scenario, tick));
            }
        }
        assert!(
            wing_changed,
            "stationary bat's wall-clock wing animation must advance"
        );
        if step != 0.0 {
            gait_crossings.push((scenario, crossings));
            for (index, changed) in legs_changed.into_iter().enumerate() {
                assert!(
                    changed,
                    "{} must walk in the {scenario} replay",
                    SPECIES[index]
                );
            }
            let stopped = gait::verify(&world);
            for _ in 0..40 {
                world.advance_actor_interpolation_frame(1);
            }
            for (index, current) in gait::verify(&world).iter().enumerate() {
                assert!(
                    stopped[index]
                        .iter()
                        .zip(current)
                        .all(|(a, b)| close(*a, *b)),
                    "{} stopped legs must settle instead of continuing an idle walking cycle",
                    SPECIES[index]
                );
                gait::verify_stopped(&world, index);
            }
        }
    }
    for (index, species) in SPECIES.iter().enumerate().take(MOB_IDS.len() - 1) {
        assert!(
            gait_crossings[1].1[index] > gait_crossings[0].1[index],
            "{} fast walking must complete more gait half-cycles than slow walking over the same elapsed time: {:?}",
            species,
            gait_crossings
        );
    }
    gpu::verify(&app);
    let rows = frames.len().div_ceil(SHEET_COLUMNS as usize) as u32;
    let mut sheet =
        image::RgbaImage::new(gpu::VIEWPORT[0] * SHEET_COLUMNS, gpu::VIEWPORT[1] * rows);
    for (index, (_, frame)) in frames.iter().enumerate() {
        image::imageops::replace(
            &mut sheet,
            frame,
            (index as i64 % i64::from(SHEET_COLUMNS)) * i64::from(gpu::VIEWPORT[0]),
            (index as i64 / i64::from(SHEET_COLUMNS)) * i64::from(gpu::VIEWPORT[1]),
        );
    }
    sheet.save(out.join("contact_sheet.png")).unwrap();
    std::fs::write(
        out.join("poses.json"),
        serde_json::to_vec_pretty(&trace).unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("frames.json"),
        serde_json::to_vec_pretty(&frames.iter().map(|(name, _)| name).collect::<Vec<_>>())
            .unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "platform": std::env::consts::OS,
            "viewport": gpu::VIEWPORT,
            "scale": 1.0,
            "render_path": "ActorRenderPlugin, offscreen RGBA native GPU readback",
            "source_manifest": serde_json::from_slice::<serde_json::Value>(include_bytes!(
                "../../../../../assets/vanilla-source.json"
            )).unwrap(),
            "actor_tick_seconds": world::TICK_DURATION.as_secs_f32(),
            "walking_packets_per_tick": 1,
            "slow_blocks_per_tick": SLOW_STEP,
            "fast_blocks_per_tick": FAST_STEP,
            "walking_ticks": WALKING_TICKS,
            "stationary_ticks": STATIONARY_TICKS,
            "gait_half_cycles": gait_crossings,
            "camera": "fixed three-quarter view, translated with walking targets",
            "world_mutated": false
        }))
        .unwrap(),
    )
    .unwrap();
}

fn compile(pack: &Path) -> Vanilla {
    let manifest = include_bytes!("../../../../../assets/vanilla-source.json");
    let compiled = pack_compiler::compile_entity_assets(pack, manifest).unwrap();
    let bytes = assets::encode_entity_blob(&compiled).unwrap();
    let entities = Arc::new(RuntimeEntityAssets::decode(&bytes).unwrap());
    let artwork = pack_compiler::compile_actor_assets(pack, manifest).unwrap();
    let catalog = assets::RuntimeActorCatalog::decode(&artwork.bytes, &entities).unwrap();
    Vanilla {
        candidates: catalog
            .bindings()
            .iter()
            .map(|binding| binding.geometry_candidate)
            .collect(),
        artwork: ActorArtworkPages::default()
            .with_pack_artwork(catalog.textures(), catalog.bindings()),
        entities,
    }
}

fn spawn(vanilla: &Vanilla) -> WorldStream {
    let mut world = world_for(
        &vanilla.entities,
        &vanilla.candidates,
        gpu::camera_transform(0.0).translation.to_array(),
    );
    for (index, identifier) in SPECIES.iter().enumerate() {
        world
            .submit(
                index as u64 + 1,
                WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                    dimension: 0,
                    unique_id: -(MOB_IDS[index] as i64),
                    runtime_id: MOB_IDS[index],
                    kind: ActorKind::Entity {
                        identifier: (*identifier).into(),
                    },
                    position: INITIAL[index],
                    velocity: [0.0; 3],
                    pitch: 0.0,
                    yaw: -90.0,
                    head_yaw: -90.0,
                    body_yaw: -90.0,
                    held_item: Default::default(),
                    metadata: Arc::from([]),
                    attributes: Arc::from([]),
                    properties: Arc::from([]),
                    links: Arc::from([]),
                })),
            )
            .unwrap();
    }
    world
}

fn move_mobs(world: &mut WorldStream, tick: u32, travel: f32) {
    for (index, runtime_id) in MOB_IDS[..2].iter().enumerate() {
        world
            .submit(
                4 + (u64::from(tick) - 1) * 2 + index as u64,
                WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
                    dimension: 0,
                    runtime_id: *runtime_id,
                    position: [
                        Some(INITIAL[index][0] + travel),
                        Some(INITIAL[index][1]),
                        Some(INITIAL[index][2]),
                    ],
                    position_origin: ActorPositionOrigin::Feet,
                    pitch: None,
                    yaw: None,
                    head_yaw: None,
                    on_ground: Some(true),
                    teleported: false,
                    player_mode: None,
                    source_tick: Some(u64::from(tick)),
                    interpolation: Default::default(),
                })),
            )
            .unwrap();
    }
}

fn publish(
    app: &mut App,
    scene: &mut ActorRenderScene,
    world: &WorldStream,
    artwork: &ActorArtworkPages,
) {
    let bodies = MOB_IDS.into_iter().enumerate().map(|(index, id)| {
        let rig = world
            .authority()
            .actor_rig(id)
            .unwrap_or_else(|| panic!("{} has no compiled vanilla rig", SPECIES[index]));
        assert!(
            !rig.current.is_empty(),
            "{} has no compiled pose bones",
            SPECIES[index]
        );
        client_presentation::presentation::actors::entity_rig_presentation(
            &rig,
            world.authority().actor(id).unwrap(),
            artwork,
            1.0,
        )
        .expect("drawable vanilla body")
    });
    let mut batch = client_presentation::presentation::actors::select_actor_presentations(
        1, false, None, bodies,
    );
    client_presentation::presentation::entity_layers::apply_render_layers(
        &mut batch,
        |id| world.authority().actor_rig(id),
        artwork,
    );
    let frame: ActorRenderFrame = scene
        .update_rigs_with_artwork(1.0, None, batch.submissions.clone(), &[], &batch.artwork)
        .clone();
    for (index, id) in MOB_IDS.into_iter().enumerate() {
        let entry = frame
            .rig
            .manifest
            .iter()
            .find(|entry| entry.identity.runtime_id == id)
            .unwrap_or_else(|| {
                panic!(
                    "{} has no drawable geometry; rejects={:?}",
                    SPECIES[index], frame.rig.rejects
                )
            });
        let instance = &frame.rig.instances[entry.instance_index as usize];
        assert!(
            frame.rig.geometry_spans[instance.geometry_id as usize].vertex_count > 0,
            "{} compiled geometry contains no GPU vertices",
            SPECIES[index]
        );
    }
    app.world_mut().insert_resource(frame);
}

fn selected_bones(world: &WorldStream, id: u64, prefix: &str) -> Vec<BoneTransform> {
    let rig = world.authority().actor_rig(id).unwrap();
    let selected: Vec<_> = rig
        .bone_names
        .iter()
        .zip(rig.current)
        .filter(|(name, _)| name.contains(prefix))
        .map(|(_, bone)| *bone)
        .collect();
    assert!(!selected.is_empty(), "vanilla rig has {prefix} bones");
    selected
}

fn close(a: BoneTransform, b: BoneTransform) -> bool {
    a.rotation
        .iter()
        .chain(&a.translation_scale)
        .chain(&a.axis_scale)
        .zip(
            b.rotation
                .iter()
                .chain(&b.translation_scale)
                .chain(&b.axis_scale),
        )
        .all(|(a, b)| (a - b).abs() < 1.0e-5)
}

fn samples(world: &WorldStream, scenario: &str, tick: u32) -> Vec<PoseSample> {
    MOB_IDS
        .into_iter()
        .map(|runtime_id| {
            let rig = world.authority().actor_rig(runtime_id).unwrap();
            PoseSample {
                scenario: scenario.into(),
                tick,
                runtime_id,
                position: world.authority().actor(runtime_id).unwrap().position,
                bones: rig
                    .bone_names
                    .iter()
                    .zip(rig.current)
                    .map(|(name, bone)| (name.to_string(), bone.rotation))
                    .collect(),
            }
        })
        .collect()
}
