//! Remote armour must survive packet admission, frame publication and renderer preparation.
use std::sync::Arc;

use bevy::prelude::{Vec3, World};
use client_presentation::{
    actor_publication::{HandRigBuilder, publish_actor_render_frame},
    presentation::equipment::EquipmentRuntime,
};
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldEvent};
use render_model::ActorArtworkPages;
use render_scene::{ActorRenderFrame, ActorRenderScene};

const REMOTE: u64 = 42;
const ARMOUR_PIXEL: [u8; 4] = [30, 180, 210, 255];
const ATTACHABLE_PIXEL: [u8; 4] = [220, 100, 30, 255];

/// Encodes an original solid image so texture routing can be checked without game assets.
fn png(color: [u8; 4]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(32, 32, image::Rgba(color))
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

/// Compiles a player, inherited chestplate shell and an independently textured held attachable.
fn fixture() -> (
    Arc<assets::RuntimeEntityAssets>,
    Arc<assets::RuntimeEquipmentCatalog>,
) {
    let files = vec![
        ("entity/player.json".into(), br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{
            "identifier":"minecraft:player","materials":{"default":"entity_alphatest"},
            "textures":{"default":"textures/fixture/body"},"geometry":{"default":"geometry.fixture.body"},
            "render_controllers":["controller.render.fixture"]}}}"#.to_vec()),
        ("models/entity/fixture.json".into(), br#"{"format_version":"1.8.0",
            "geometry.fixture.body":{"texturewidth":32,"textureheight":32,"bones":[
                {"name":"body","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[0,0]}]},
                {"name":"rightItem","parent":"body","pivot":[-6,15,1]}]},
            "geometry.fixture.armour:geometry.fixture.body":{"bones":[{"name":"body","inflate":1.0}]},
            "geometry.fixture.held":{"texturewidth":32,"textureheight":32,"bones":[
                {"name":"rightItem","pivot":[0,0,0],"cubes":[{"origin":[-1,0,-1],"size":[2,8,2],"uv":[0,0]}]}]}
        }"#.to_vec()),
        ("attachables/chestplate.json".into(), br#"{"format_version":"1.10.0","minecraft:attachable":{"description":{
            "identifier":"minecraft:diamond_chestplate","materials":{"default":"armor"},
            "textures":{"default":"textures/fixture/armour"},"geometry":{"default":"geometry.fixture.armour"},
            "render_controllers":["controller.render.fixture"]}}}"#.to_vec()),
        ("attachables/held.json".into(), br#"{"format_version":"1.10.0","minecraft:attachable":{"description":{
            "identifier":"minecraft:diamond_sword","materials":{"default":"entity_alphatest"},
            "textures":{"default":"textures/fixture/held"},"geometry":{"default":"geometry.fixture.held"},
            "render_controllers":["controller.render.fixture"]}}}"#.to_vec()),
        ("render_controllers/fixture.json".into(), br#"{"format_version":"1.8.0","render_controllers":{
            "controller.render.fixture":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],
            "textures":["Texture.default"]}}}"#.to_vec()),
        ("textures/fixture/body.png".into(), png([70, 120, 80, 255])),
        ("textures/fixture/armour.png".into(), png(ARMOUR_PIXEL)),
        ("textures/fixture/held.png".into(), png(ATTACHABLE_PIXEL)),
    ];
    let compiled = pack_entity_compiler::compile_actor_pack(files)
        .unwrap()
        .unwrap();
    let equipment = assets::RuntimeEquipmentCatalog::from_parts(
        compiled.identity,
        compiled.equipment_bindings,
        compiled.equipment_textures,
    )
    .unwrap();
    (
        Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap()),
        Arc::new(equipment),
    )
}

/// Resolves a vanilla identifier through the same registry used by equipment packet admission.
fn stack(identifier: &str) -> protocol::NetworkItemStack {
    let registry = protocol::vanilla_item_registry();
    let entry = registry
        .iter()
        .find(|entry| entry.identifier.as_ref() == identifier)
        .unwrap();
    protocol::NetworkItemStack {
        network_id: entry.network_id,
        count: 1,
        ..Default::default()
    }
}

/// Installs replicated equipment rather than replacing the presentation runtime's slot inputs.
fn world() -> World {
    let (entities, catalog) = fixture();
    let mut stream = super::actor_rest_presentation::stream(Arc::clone(&entities));
    assert!(stream.seed_item_registry(protocol::ItemRegistryEvent {
        entries: protocol::vanilla_item_registry(),
    }));
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: REMOTE as i64,
                runtime_id: REMOTE,
                kind: ActorKind::Player {
                    uuid: [2; 16],
                    username: "".into(),
                },
                position: [0.0, 64.0, 0.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 180.0,
                head_yaw: 180.0,
                body_yaw: 180.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::ArmorEquipment(Box::new(protocol::ArmorEquipmentEvent {
                actor_runtime_id: REMOTE,
                chestplate: stack("minecraft:diamond_chestplate"),
                helmet: Default::default(),
                leggings: Default::default(),
                boots: Default::default(),
                body: Default::default(),
            })),
        )
        .unwrap();
    stream
        .submit(
            3,
            WorldEvent::Equipment(protocol::EquipmentEvent {
                actor_runtime_id: REMOTE,
                stack: stack("minecraft:diamond_sword"),
                inventory_slot: 0,
                selected_slot: 0,
                window_id: 0,
                handedness: None,
            }),
        )
        .unwrap();
    stream.prepare_actor_appearance_fixture();
    stream.advance_actor_interpolation_ticks(1);
    let icons = assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog([0; 32], &[], &[]).unwrap(),
    )
    .unwrap();
    let (equipment, pages, geometries) = EquipmentRuntime::build(
        Arc::clone(&entities),
        Some(catalog),
        Arc::new(icons),
        None,
        None,
        ActorArtworkPages::default(),
    );
    let scene =
        ActorRenderScene::with_runtime_entity_assets_and_equipment(&entities, &geometries).unwrap();
    let mut client = crate::runtime::world::ClientWorld::new_with_entity_assets(
        Arc::new(assets::RuntimeAssets::diagnostic()),
        Arc::clone(&entities),
    );
    client.stream = Some(stream);
    let mut world = super::actor_frame_allocations::actor_frame_world(
        client,
        scene,
        pages,
        HandRigBuilder::from_runtime_assets(&entities).unwrap(),
        (Vec3::new(0.0, 65.5, -4.0), Vec3::new(0.0, 65.0, 0.0)),
    );
    world.insert_resource(equipment);
    world
}

/// Publishes through the native frame systems, then prepares their exact output for GPU drawing.
#[test]
fn remote_armour_and_attachable_keep_artwork_bound_and_shell_outside_skin() {
    let mut world = world();
    world
        .run_system_cached(crate::runtime::network::advance_actor_frame)
        .unwrap();
    world
        .run_system_cached(crate::runtime::network::prepare_actor_render_frame)
        .unwrap();
    world.run_system_cached(publish_actor_render_frame).unwrap();
    let frame = world.resource::<ActorRenderFrame>();
    let draws = render::prepare_actor_test_frame(frame.clone());
    let remote: Vec<_> = draws
        .iter()
        .filter(|draw| draw.entry.identity.runtime_id == REMOTE)
        .collect();
    assert_eq!(
        remote.len(),
        3,
        "body, held attachable and chestplate must reach render preparation"
    );
    let pixel_of = |draw: &render::PreparedActorDraw| {
        let instance = &frame.rig.instances[draw.entry.instance_index as usize];
        let page = &frame.artwork.pages[usize::from(draw.page) - 1];
        let offset = instance.texture_layer as usize
            * usize::from(page.width)
            * usize::from(page.height)
            * 4;
        &page.rgba8[offset..offset + 4]
    };
    let equipment: Vec<_> = remote
        .iter()
        .filter(|draw| draw.entry.identity.layer != render_model::ACTOR_LAYER_BODY)
        .collect();
    for draw in &equipment {
        assert!(draw.vertex_count > 0);
        assert!(
            draw.artwork_bound,
            "the prepared span must bind its own artwork"
        );
        assert!(
            draw.page > 0,
            "equipment cannot sample the player skin array"
        );
    }
    let chest = equipment
        .iter()
        .find(|draw| pixel_of(draw) == ARMOUR_PIXEL)
        .unwrap();
    assert!(
        equipment
            .iter()
            .any(|draw| pixel_of(draw) == ATTACHABLE_PIXEL)
    );
    let instance = &frame.rig.instances[chest.entry.instance_index as usize];
    let span = frame.rig.geometry_spans[instance.geometry_id as usize];
    let vertices = frame.rig.geometry_vertices.span(span).unwrap();
    for vertex in vertices {
        for (matrices, base) in [
            (&frame.rig.previous_bones, chest.entry.previous_bone_base),
            (&frame.rig.current_bones, chest.entry.current_bone_base),
        ] {
            let bone = matrices[(base + vertex.bone_index) as usize];
            assert!(
                bone.iter()
                    .flat_map(|row| &row[..3])
                    .any(|value| *value != 0.0),
                "armour must retain a visible pose at both endpoints"
            );
        }
    }
    for (axis, expected) in [5.0, 25.0, 3.0].into_iter().enumerate() {
        let high = vertices
            .iter()
            .map(|vertex| vertex.position[axis] * 16.0)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            (high - expected).abs() < 1e-5,
            "armour axis {axis}: {high} != {expected}; the shell must enclose the body"
        );
    }
}
