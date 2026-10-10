//! Main-thread cost of per-frame paths, old shape against new, at crowded-server loads.
//! Run: `cargo test -p bedrock-client --features reports --lib frame_cost_bench -- --ignored --nocapture`.

use std::time::{Duration, Instant};

use render::{
    ActorRigFrameBuilder, BlockEntityKind, BlockEntityScene, BlockEntitySubmission, SceneClock,
};
use render_model::{
    ActorRigGeometry, ActorRigVertex, ActorSkinPixels, normalize_actor_skin,
    prepare_actor_skin_cached, skin_rig_id,
};

const FRAMES: u32 = 200;

fn per_frame(frames: u32, mut frame: impl FnMut(u32)) -> Duration {
    let started = Instant::now();
    for index in 0..frames {
        frame(index);
    }
    started.elapsed() / frames
}

fn report(name: &str, old: Duration, new: Duration) {
    eprintln!(
        "FRAME_COST {name}: old={:.3}ms new={:.3}ms",
        old.as_secs_f64() * 1e3,
        new.as_secs_f64() * 1e3
    );
}

#[test]
#[ignore = "benchmark"]
fn frame_cost_bench_skin_normalization_50_hd_players() {
    let skins: Vec<ActorSkinPixels> = (0..50)
        .map(|player| ActorSkinPixels {
            width: 256,
            height: 256,
            rgba8: vec![player as u8; 256 * 256 * 4].into(),
        })
        .collect();
    let old = per_frame(FRAMES, |_| {
        for skin in &skins {
            std::hint::black_box(normalize_actor_skin(skin));
        }
    });
    let new = per_frame(FRAMES, |_| {
        for skin in &skins {
            std::hint::black_box(prepare_actor_skin_cached(skin));
        }
    });
    report("skin_normalization_50_hd", old, new);
}

#[test]
#[ignore = "benchmark"]
fn frame_cost_bench_geometry_registration_30_models() {
    // About the vanilla catalog's size: 224 geometries of 500 vertices.
    let geometry = |id| {
        ActorRigGeometry::new(id, vec![ActorRigVertex::default(); 500], vec![[0.0; 3]; 4]).unwrap()
    };
    let catalog = || {
        ActorRigFrameBuilder::new((0..224).map(|index| geometry(render_model::EntityRigId(index))))
            .unwrap()
    };
    let models = || {
        (0..30)
            .map(|slot| geometry(skin_rig_id(slot)))
            .collect::<Vec<_>>()
    };
    let mut builder = catalog();
    let old = per_frame(10, |_| {
        for model in models() {
            builder.insert_geometry(model).unwrap();
        }
    });
    let mut builder = catalog();
    let new = per_frame(10, |_| builder.insert_geometries(models()).unwrap());
    report("geometry_registration_30_models_one_builder", old, new);
}

#[test]
#[ignore = "benchmark"]
fn frame_cost_bench_block_entity_scene_400_static() {
    let bytes = assets::encode_block_entity_catalog(
        b"{}",
        128,
        64,
        &vec![255u8; 128 * 64 * 4],
        &[assets::BlockEntityPlacement {
            name: "textures/entity/chest/normal".into(),
            x: 0,
            y: 0,
            width: 64,
            height: 64,
        }],
    )
    .unwrap();
    let mut scene = BlockEntityScene::default();
    scene.install_assets(&assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap());
    let chests: Vec<BlockEntitySubmission> = (0..400)
        .map(|index| BlockEntitySubmission {
            block: [index % 20, 64, index / 20],
            light: 1.0.into(),
            kind: BlockEntityKind::Chest(render::ChestModel {
                variant: render::ChestVariant::Normal,
                facing: render::Facing::North,
                pair: render::ChestPair::Single,
                lid: 0.0,
            }),
        })
        .collect();
    // Alternating light defeats reuse, which is what every frame paid before.
    let old = per_frame(FRAMES, |frame| {
        let light = if frame % 2 == 0 { 1.0 } else { 0.5 };
        let frame: Vec<_> = chests
            .iter()
            .map(|chest| BlockEntitySubmission {
                light: light.into(),
                ..chest.clone()
            })
            .collect();
        std::hint::black_box(scene.update(SceneClock::default(), &[], &frame).revision);
    });
    let new = per_frame(FRAMES, |_| {
        std::hint::black_box(scene.update(SceneClock::default(), &[], &chests).revision);
    });
    report("block_entity_scene_400_chests", old, new);
}

#[test]
#[ignore = "benchmark"]
fn frame_cost_bench_sound_decode() {
    let path = client_presentation::audio::sound_bank_path(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(crate::asset_startup::DEFAULT_ASSET_PATH),
    );
    let mut bank = client_presentation::audio::SoundBank::open(&path, None)
        .expect("offline sound timing requires a valid installed sound bank (make assets)")
        .expect("offline sound timing requires the installed sound bank (make assets)");
    // The largest streamed sound (re-decoded on every start) and common first-play effects.
    for sound in [
        "sounds/ambient/underwater/loop/underwater_ambience",
        "sounds/random/explode3",
        "sounds/random/use_totem",
        "sounds/random/pop",
    ] {
        let started = Instant::now();
        let decoded = bank.pcm(sound, true);
        let old = started.elapsed();
        let started = Instant::now();
        let _ = bank.lookup(sound, true);
        let new = started.elapsed();
        eprintln!(
            "FRAME_COST first_play {sound} ({:.1} s): old={:.3}ms (decoded in the frame) new={:.3}ms",
            decoded.map_or(0.0, |pcm| pcm.frames() as f64 / f64::from(pcm.rate.max(1))),
            old.as_secs_f64() * 1e3,
            new.as_secs_f64() * 1e3
        );
    }
}
