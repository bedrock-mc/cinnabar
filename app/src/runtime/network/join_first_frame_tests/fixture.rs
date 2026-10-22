//! Small original assets and packets for the join smoke test.

use super::*;
use bevy::{
    asset::{AssetPlugin, Assets},
    camera::{CameraPlugin, RenderTarget},
    core_pipeline::CorePipelinePlugin,
    image::ImagePlugin,
    mesh::MeshPlugin,
    render::render_resource::TextureFormat,
    window::WindowPlugin,
};
use std::io::Cursor;

pub(super) const EYE: [f32; 3] = [8.5, 2.620_01, 12.5];
pub(super) const ACTOR: u64 = 73;
pub(super) const KEY: world::SubChunkKey = world::SubChunkKey::new(0, 0, 0, 0);
pub(super) const TERRAIN_COLOR: [u8; 4] = [31, 173, 89, 255];
pub(super) const ACTOR_COLOR: [u8; 4] = [223, 67, 41, 255];

/// Declares one original cube using the normal server block-definition parser.
pub(super) fn blocks() -> protocol::CustomBlocks {
    let mut blocks =
        protocol::CustomBlocks::from_definitions([("join:floor", [10, 0, 0].as_slice())]);
    Arc::make_mut(&mut Arc::make_mut(&mut blocks.blocks)[0].visual)
        .base
        .materials = Some(Box::new([protocol::CustomMaterialInstance {
        name: "*".into(),
        texture: "join_floor".into(),
        render_method: None,
        tint_method: None,
        ambient_occlusion: None,
        face_dimming: None,
    }]));
    blocks
}

/// Encodes a solid-color original PNG with no filesystem inputs.
fn png(color: [u8; 4]) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba(color))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// Admits original terrain and actor art through the runtime archive and compiler path.
pub(super) fn pack() -> Arc<resource_pack::ValidatedPackStack> {
    pack_reload_tests::stack(&[
        ("textures/terrain_texture.json", br#"{"texture_data":{"join_floor":{"textures":"textures/blocks/join_floor"}}}"#),
        ("textures/blocks/join_floor.png", &png(TERRAIN_COLOR)),
        ("entity/join.json", br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"join:actor","geometry":{"default":"geometry.join"},"materials":{"default":"entity_alphatest"},"textures":{"default":"textures/entity/join"},"render_controllers":["controller.render.join"]}}}"#),
        ("models/entity/join.json", br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.join","texture_width":16,"texture_height":16},"bones":[{"name":"root","cubes":[{"origin":[-4,0,-4],"size":[8,16,8],"uv":[0,0]}]}]}]}"#),
        ("render_controllers/join.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.join":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#),
        ("textures/entity/join.png", &png(ACTOR_COLOR)),
    ])
}

/// Supplies the StartGame control produced by session preparation, including its pack inputs.
pub(super) fn bootstrap(
    packs: resource_packs::PackApplication,
    custom_blocks: protocol::CustomBlocks,
) -> session::NetworkControlEvent {
    session::NetworkControlEvent::Bootstrap {
        identity: Default::default(),
        session_generation: simulation_clock::identity::SessionGeneration::new(1),
        world: protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 42,
            local_player_unique_id: 1,
            player_position: EYE,
            world_spawn_position: [8, 1, 12],
            air_network_id: protocol::air_network_id(true),
            block_network_ids_are_hashes: true,
        },
        environment: protocol::WorldEnvironmentBootstrap {
            initial_time: 0,
            day_cycle_lock_time: -1,
            daylight_cycle_enabled: true,
            weather_cycle_enabled: true,
            rain_level: 0.0,
            lightning_level: 0.0,
        },
        custom_blocks,
        inventory: protocol::InventoryEvent::Authority(protocol::InventoryAuthority::Server),
        item_registry: None,
        player_game_mode: protocol::PlayerGameMode::Survival,
        world_default_game_mode: protocol::GameModeUpdate::Explicit(
            protocol::PlayerGameMode::Survival,
        ),
        player_game_mode_uses_world_default: false,
        server_authoritative_block_breaking: true,
        rewind_history_size: 20,
        base_game_version: protocol::BaseGameVersion::Any,
        hardcore: false,
        hud_rules: Default::default(),
        death_rules: Default::default(),
        crafting_rules: Default::default(),
        packs,
        terrain_before_spawn: true,
        level_identity: None,
    }
}

/// Writes Bedrock's unsigned varint framing; palette integers are zigzagged by the caller.
fn varint(bytes: &mut Vec<u8>, mut value: u32) {
    loop {
        let more = value >= 128;
        bytes.push((value as u8 & 127) | if more { 128 } else { 0 });
        value >>= 7;
        if !more {
            break;
        }
    }
}

/// Encodes a signed palette ID while preserving all bits of a hashed runtime ID.
fn palette_id(bytes: &mut Vec<u8>, id: u32) {
    varint(bytes, (id << 1) ^ ((id as i32 >> 31) as u32));
}

/// Encodes a loaded column with one custom floor and air above it.
pub(super) fn terrain(block: u32) -> protocol::WorldEvent {
    let air = protocol::air_network_id(true);
    let range = protocol::vanilla_dimension_range(0).unwrap();
    let mut payload = Vec::new();
    let count = (1 - range.base_sub_chunk_y) as usize;
    for y in range.base_sub_chunk_y..=0 {
        payload.extend([9, 1, y as u8]);
        if y < 0 {
            payload.push(1);
            palette_id(&mut payload, air);
        } else {
            payload.push(3);
            for word in 0u32..128 {
                let mut bits = 0u32;
                for bit in 0..32 {
                    if (word * 32 + bit).is_multiple_of(16) {
                        bits |= 1 << bit;
                    }
                }
                payload.extend(bits.to_le_bytes());
            }
            varint(&mut payload, 4);
            palette_id(&mut payload, air);
            palette_id(&mut payload, block);
        }
    }
    payload.extend([1, 0]);
    payload.extend(std::iter::repeat_n(0xff, range.sub_chunk_count - 1));
    payload.push(0);
    protocol::WorldEvent::LevelChunk(protocol::LevelChunkEvent {
        dimension: 0,
        x: 0,
        z: 0,
        mode: protocol::LevelChunkMode::Inline { count },
        payload,
    })
}

/// Spawns the custom actor in front of the local player's camera.
pub(super) fn actor() -> protocol::WorldEvent {
    protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
        dimension: 0,
        unique_id: -(ACTOR as i64),
        runtime_id: ACTOR,
        kind: protocol::ActorKind::Entity {
            identifier: "join:actor".into(),
        },
        position: [8.5, 1.0, 8.5],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    }))
}

/// Changes the biome identity while the first atlas and geometry request is held.
pub(super) fn biomes() -> protocol::WorldEvent {
    protocol::WorldEvent::BiomeDefinitions(protocol::BiomeDefinitionsEvent {
        definitions: Arc::from([protocol::BiomeDefinitionEvent {
            biome_id: Some(0),
            name: "join:biome".into(),
            temperature: 0.8,
            downfall: 0.4,
            snow_foliage: 0.0,
            max_snow_accumulation: None,
            map_water_color: 0xff12_3456,
        }]),
    })
}

/// Adds the real render plugins and frame consumers around deterministic, injected time.
pub(super) fn install_renderer(
    app: &mut App,
    base: Arc<assets::RuntimeAssets>,
    artwork: render_model::ActorArtworkPages,
    entities: Arc<assets::RuntimeEntityAssets>,
) {
    app.add_plugins(MinimalPlugins)
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::ZERO))
        .add_plugins(WindowPlugin { primary_window: None, ..Default::default() })
        .add_plugins(AssetPlugin::default())
        .add_plugins(render::publication_noop_render_plugin())
        .add_plugins((ImagePlugin::default(), MeshPlugin, CameraPlugin, CorePipelinePlugin))
        .insert_resource(ChunkTextureAssets::new(base))
        .init_resource::<render_scene::ChunkBiomeTints>()
        .add_plugins((render::ChunkRenderPlugin::default(), render::ActorRenderPlugin))
        .init_resource::<client_interaction::movement::LocomotionState>()
        .init_resource::<CameraPose>()
        .init_resource::<client_presentation::local_player_camera_receipt::CameraPublicationAttempt>()
        .init_resource::<client_presentation::actor_publication::ActorFrameState>()
        .init_resource::<client_presentation::actor_publication::PreparedActorPublication>()
        .init_resource::<ActorFramePartialTick>()
        .init_resource::<render_scene::HandRigScene>()
        .insert_resource(render_scene::ActorArtworkResource(artwork))
        .insert_resource(render_scene::ActorRenderScene::with_runtime_entity_assets(&entities).unwrap())
        .insert_resource(HandRigBuilder::from_runtime_assets(&entities).unwrap())
        .insert_resource(client_shell::player_skin::LocalPlayerSkin::generated_default("join"))
        .insert_resource(LocalViewPose::new(Vec3::from_array(EYE), Quat::IDENTITY))
        .add_systems(Update, crate::movement::advance_local_physics.in_set(ClientFrameSet::Physics))
        .add_systems(Update, (
            crate::local_player::resolve_local_player_presentation,
            client_presentation::local_player_camera_receipt::begin_camera_publication_attempt,
            crate::local_player::resolve_camera_pose,
            crate::local_player::publish_local_player_frame,
        ).chain().in_set(ClientFrameSet::Camera))
        .add_systems(Update, (advance_actor_frame, prepare_actor_render_frame, publish_actor_render_frame).chain().in_set(ClientFrameSet::ActorPublication))
        .add_systems(Update, crate::runtime::telemetry::send_player_auth_inputs.in_set(ClientFrameSet::NetworkSend));
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            32,
            32,
            TextureFormat::Rgba8Unorm,
            None,
        ));
    app.world_mut().spawn((
        Camera3d::default(),
        Camera::default(),
        RenderTarget::Image(target.into()),
        Msaa::Off,
        Transform::from_translation(Vec3::from_array(EYE)),
        FlyCamera::default(),
    ));
    client_runtime::network::configure_network_frame_flush::<resource_packs::PackApplication>(app);
    app.finish();
    app.cleanup();
}

/// Meshes decoded resident blocks and sends them through the production CPU/GPU handoff.
pub(super) fn seed_resident_terrain(app: &mut App) {
    let mut meshes = None;
    test_time::eventually("resident terrain lighting", || {
        let mut world = app
            .world_mut()
            .resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = world.stream.as_mut().unwrap();
        stream.begin_frame_work();
        stream.poll(EYE, 0);
        meshes = stream
            .resource_mesh_snapshot([KEY])
            .and_then(|snapshot| snapshot.build(stream.runtime_assets()));
        meshes.is_some()
    });
    let mut meshes = meshes.unwrap();
    let (key, mesh, biome) = meshes.pop().unwrap();
    assert!(meshes.is_empty());
    assert!(!mesh.is_empty());
    app.world_mut()
        .resource_mut::<render_scene::ChunkRenderQueue>()
        .try_update_with_biome(
            key,
            mesh,
            biome,
            render_scene::ChunkUploadPriority::new(0.0),
        )
        .unwrap();
}
