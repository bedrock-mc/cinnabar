//! Optional fixed-camera projectile frames through the actor scene publication path.
use std::{path::Path, sync::Arc};

use bevy::math::{Mat4, Vec3};
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldEvent};

use super::{
    render_report::world_for,
    scene_report::{Frame, HEIGHT, WIDTH, draw_actors},
};

/// Compiles the supplied vanilla pack and renders four fixed projectile states to scratch.
#[test]
fn render_projectile_states() {
    let (Ok(pack), Ok(out)) = (
        std::env::var("CINNABAR_PROJECTILE_PACK"),
        std::env::var("CINNABAR_PROJECTILE_OUT"),
    ) else {
        eprintln!(
            "skipping render_projectile_states: fixture unavailable; offline image export; requires CINNABAR_PROJECTILE_PACK and CINNABAR_PROJECTILE_OUT"
        );
        return;
    };
    let manifest = include_bytes!("../../../../../assets/vanilla-source.json");
    let compiled = asset_compiler::compile_entity_assets(Path::new(&pack), manifest).unwrap();
    let bytes = assets::encode_entity_blob(&compiled).unwrap();
    let entities = Arc::new(assets::RuntimeEntityAssets::decode(&bytes).unwrap());
    let artwork = asset_compiler::compile_actor_assets(Path::new(&pack), manifest).unwrap();
    let catalog = assets::RuntimeActorCatalog::decode(&artwork.bytes, &bytes).unwrap();
    let candidates = catalog
        .bindings()
        .iter()
        .map(|binding| binding.geometry_candidate)
        .collect::<Vec<_>>();
    let pages = render::ActorArtworkPages::default()
        .with_pack_artwork(catalog.textures(), catalog.bindings());
    std::fs::create_dir_all(&out).unwrap();
    for (name, identifier, pitch, yaw) in [
        ("arrow_flight", "minecraft:arrow", -20.0, 60.0),
        ("arrow_stuck", "minecraft:arrow", 0.0, -60.0),
        ("ender_pearl", "minecraft:ender_pearl", 0.0, 0.0),
        ("snowball", "minecraft:snowball", 0.0, 0.0),
    ] {
        let eye = Vec3::new(0.0, 0.25, 2.0);
        let mut world = world_for(&entities, &candidates, eye.to_array());
        world.set_actor_camera_rotation([0.0, 180.0]);
        world
            .submit(
                1,
                WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                    dimension: 0,
                    unique_id: -42,
                    runtime_id: 42,
                    kind: ActorKind::Entity {
                        identifier: identifier.into(),
                    },
                    position: if name == "arrow_stuck" {
                        [-0.5, 0.0, 0.0]
                    } else {
                        [0.0; 3]
                    },
                    velocity: [0.0; 3],
                    pitch,
                    yaw,
                    head_yaw: yaw,
                    body_yaw: yaw,
                    held_item: Default::default(),
                    metadata: Arc::from([]),
                    attributes: Arc::from([]),
                    properties: Arc::from([]),
                    links: Arc::from([]),
                })),
            )
            .unwrap();
        world.advance_actor_interpolation_ticks(1);
        let clip = Mat4::perspective_rh(
            45_f32.to_radians(),
            WIDTH as f32 / HEIGHT as f32,
            0.05,
            20.0,
        ) * Mat4::look_at_rh(eye, Vec3::new(0.0, 0.25, 0.0), Vec3::Y);
        let mut frame = Frame::new(clip);
        if name == "arrow_stuck" {
            draw_stone_block(&mut frame, Path::new(&pack));
        }
        draw_actors(&mut frame, &world, &[42], &entities, &pages);
        frame
            .image
            .save(Path::new(&out).join(format!("{name}.png")))
            .unwrap();
    }
}

/// Draws the support block from the same vanilla pack, without changing actor geometry.
fn draw_stone_block(frame: &mut Frame, pack: &Path) {
    let stone = image::open(pack.join("textures/blocks/stone.png"))
        .unwrap()
        .into_rgba8();
    let corners = [
        [-1.3, -0.4, -0.1],
        [-0.55, -0.4, -0.1],
        [-0.55, 0.4, -0.1],
        [-1.3, 0.4, -0.1],
        [-1.3, -0.4, 0.65],
        [-0.55, -0.4, 0.65],
        [-0.55, 0.4, 0.65],
        [-1.3, 0.4, 0.65],
    ]
    .map(Vec3::from_array);
    let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let shade = |uv: [f32; 2]| {
        let x = (uv[0].clamp(0.0, 1.0) * stone.width() as f32) as u32;
        let y = (uv[1].clamp(0.0, 1.0) * stone.height() as f32) as u32;
        Some(
            stone
                .get_pixel(x.min(stone.width() - 1), y.min(stone.height() - 1))
                .0,
        )
    };
    for face in [
        [3, 2, 1, 0],
        [6, 7, 4, 5],
        [7, 3, 0, 4],
        [2, 6, 5, 1],
        [7, 6, 2, 3],
        [0, 1, 5, 4],
    ] {
        let quad = std::array::from_fn::<_, 4, _>(|i| (corners[face[i]], uv[i]));
        frame.triangle([quad[0], quad[1], quad[2]], true, true, &shade);
        frame.triangle([quad[0], quad[2], quad[3]], true, true, &shade);
    }
}
