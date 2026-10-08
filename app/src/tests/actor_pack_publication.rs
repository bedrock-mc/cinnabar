//! Item and artwork refreshes must retain unchanged actor geometry.

use std::sync::Arc;

use bevy::prelude::{Vec3, World};
use render::{ActorArtworkPages, ActorRenderFrame, ActorRenderScene};

use crate::runtime::{
    network::{
        HandRigBuilder,
        entity_pack::{SessionEntityPack, SessionItems},
        prepare_actor_render_frame, publish_actor_render_frame,
    },
    world::ClientWorld,
};

/// Publishes through the actor stages used by the native main frame.
fn publish(world: &mut World) -> ActorRenderFrame {
    world
        .run_system_cached(crate::runtime::network::advance_actor_frame)
        .unwrap();
    world.run_system_cached(prepare_actor_render_frame).unwrap();
    world.run_system_cached(publish_actor_render_frame).unwrap();
    world.resource::<ActorRenderFrame>().clone()
}

/// Gives a compiled catalog its own immutable session identity.
fn session_pack(assets: Arc<assets::RuntimeEntityAssets>) -> Arc<SessionEntityPack> {
    Arc::new(SessionEntityPack {
        assets,
        textures: Arc::from([]),
        bindings: Arc::from([]),
        equipment: None,
    })
}

/// Installs one session into the production actor publication systems.
fn session_world(
    pack: Arc<SessionEntityPack>,
    artwork: ActorArtworkPages,
    scene: ActorRenderScene,
) -> World {
    let entities = pack.assets.clone();
    let mut client = ClientWorld::new_with_entity_assets(
        Arc::new(assets::RuntimeAssets::diagnostic()),
        entities.clone(),
    );
    client.stream = Some(super::actor_rest_presentation::stream(entities.clone()));
    client.pack_entities = Some(pack);
    super::actor_frame_allocations::actor_frame_world(
        client,
        scene,
        artwork,
        HandRigBuilder::from_runtime_assets(&entities).unwrap(),
        (Vec3::new(0.0, 66.0, -12.0), Vec3::new(0.0, 64.0, 4.0)),
    )
}

/// Refreshing item facts or artwork must not repack the entity and equipment namespaces.
#[test]
fn item_and_artwork_refreshes_retain_pack_geometry() {
    let (_fixture, artwork, entities) = super::actor_rest_presentation::compiled_fixture(
        "1.0",
        1,
        assets::ActorPoseMode::CompiledLiteral,
    );
    let scene = ActorRenderScene::with_runtime_entity_assets(&entities).unwrap();
    let mut world = session_world(session_pack(entities.clone()), artwork, scene);
    let original = publish(&mut world);
    assert!(
        world
            .resource::<ActorRenderScene>()
            .contains_geometry(render_model::pack_rig_id(0))
    );
    for artwork_changed in [false, true] {
        if artwork_changed {
            let artwork = world.resource::<ActorArtworkPages>().clone();
            *world.resource_mut::<ActorArtworkPages>() = artwork;
        } else {
            world.resource_mut::<ClientWorld>().session_items = Some(Arc::new(SessionItems {
                components: Arc::default(),
                icons: None,
            }));
        }
        let next = publish(&mut world);
        assert_eq!(next.rig.geometry_revision, original.rig.geometry_revision);
        assert_eq!(next.rig.geometry_spans, original.rig.geometry_spans);
        for span in next.rig.geometry_spans.iter() {
            assert_eq!(
                next.rig.geometry_vertices.span(*span),
                original.rig.geometry_vertices.span(*span),
            );
        }
        assert!(
            Arc::ptr_eq(
                &next.rig.geometry_vertices.segments,
                &original.rig.geometry_vertices.segments,
            ),
            "unchanged geometry was rebuilt after artwork_changed={artwork_changed}",
        );
    }
    world.resource_mut::<ClientWorld>().pack_entities = Some(session_pack(entities));
    let replacement = publish(&mut world);
    assert!(!Arc::ptr_eq(
        &replacement.rig.geometry_vertices.segments,
        &original.rig.geometry_vertices.segments,
    ));
    assert!(
        world
            .resource::<ActorRenderScene>()
            .contains_geometry(render_model::pack_rig_id(0))
    );
    world.resource_mut::<ClientWorld>().pack_entities = None;
    publish(&mut world);
    assert!(
        !world
            .resource::<ActorRenderScene>()
            .contains_geometry(render_model::pack_rig_id(0))
    );
}

const EQUIPMENT_GEOMETRY: &str = "geometry.example.held";

/// Adds an equipment-only model whose vertex storage cannot alias the entity model.
fn with_equipment_geometry(
    entities: &assets::RuntimeEntityAssets,
) -> Arc<assets::RuntimeEntityAssets> {
    let index = render_model::find_geometry_index(entities, "geometry.example").unwrap() as usize;
    let mut geometry = entities.geometries()[index].clone();
    geometry.identifier = EQUIPMENT_GEOMETRY.into();
    geometry.bones[0].cubes[0].origin[0] = assets::EntityGeometryScalar::new(16.0).unwrap();
    let mut geometries = entities.geometries().to_vec();
    geometries.push(geometry.clone());
    let mut symbols = entities.symbols().to_vec();
    let mut symbol = symbols
        .iter()
        .find(|symbol| {
            symbol.kind == assets::EntityAssetKind::Geometry
                && symbol.identifier.as_ref() == "geometry.example"
        })
        .unwrap()
        .clone();
    symbol.identifier = geometry.identifier;
    symbols.push(symbol);
    symbols.sort_by(|left, right| {
        (left.kind, &left.identifier, left.source_index).cmp(&(
            right.kind,
            &right.identifier,
            right.source_index,
        ))
    });
    let symbol_indices = entities
        .symbols()
        .iter()
        .map(|old| u32::try_from(symbols.iter().position(|new| new == old).unwrap()).unwrap())
        .collect::<Vec<_>>();
    let mut compiled = assets::CompiledEntityAssets {
        source_manifest_sha256: entities.source_manifest_sha256(),
        block_visual_count: entities.block_visual_count(),
        sources: entities.sources().into(),
        symbols: symbols.into(),
        geometries: geometries.into(),
        animation_clips: entities.animation_clips().into(),
        animation_channels: entities.animation_channels().into(),
        animation_keyframes: entities.animation_keyframes().into(),
        molang_symbols: entities.molang_symbols().into(),
        molang_expressions: entities.molang_expressions().into(),
        molang_ops: entities.molang_ops().into(),
        molang_collections: entities.molang_collections().into(),
        molang_collection_items: entities.molang_collection_items().into(),
        controllers: entities.controllers().into(),
        controller_states: entities.controller_states().into(),
        controller_animations: entities.controller_animations().into(),
        controller_transitions: entities.controller_transitions().into(),
        rig_bindings: entities.rig_bindings().into(),
        rig_geometries: entities.rig_geometries().into(),
        rig_animations: entities.rig_animations().into(),
        rig_controllers: entities.rig_controllers().into(),
        item_visuals: entities.item_visuals().into(),
        item_visual_aliases: entities.item_visual_aliases().into(),
        render: entities.render_data().clone(),
    };
    for clip in &mut compiled.animation_clips {
        clip.symbol = symbol_indices[clip.symbol as usize];
    }
    for controller in &mut compiled.controllers {
        controller.symbol = symbol_indices[controller.symbol as usize];
    }
    for rig in &mut compiled.rig_bindings {
        rig.entity_symbol = symbol_indices[rig.entity_symbol as usize];
        rig.render_controller = symbol_indices[rig.render_controller as usize];
    }
    let bytes = assets::encode_entity_blob(&compiled).unwrap();
    Arc::new(assets::RuntimeEntityAssets::decode(&bytes).unwrap())
}

/// One attachable uses a distinct model through the normal equipment compiler.
fn fixture_equipment() -> Arc<assets::RuntimeEquipmentCatalog> {
    let reference = |identifier: &str| assets::EquipmentReference {
        identifier: identifier.into(),
        resolution: assets::EntityDependencyResolution::Catalog,
    };
    Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            [1; 32],
            vec![assets::EquipmentBinding {
                identifier: "test:held".into(),
                category: assets::EquipmentCategory::Held,
                geometry: reference(EQUIPMENT_GEOMETRY),
                texture: reference("textures/entity/example"),
                material: "entity_alphatest".into(),
                render_controller: "controller.render.example".into(),
                first_person: assets::EquipmentTransform::NeedsMeasurement,
                third_person: assets::EquipmentTransform::NeedsMeasurement,
                dropped: assets::EquipmentTransform::NeedsMeasurement,
                poses: Box::new([]),
            }],
            Vec::new(),
        )
        .unwrap(),
    )
}

/// A rejected equipment range retries after capacity is freed without rebuilding accepted entities.
#[test]
fn rejected_equipment_retries_while_accepted_entities_stay_shared() {
    let (_fixture, artwork, entities) = super::actor_rest_presentation::compiled_fixture(
        "1.0",
        1,
        assets::ActorPoseMode::CompiledLiteral,
    );
    let entities = with_equipment_geometry(&entities);
    let mut pack = session_pack(entities.clone());
    Arc::get_mut(&mut pack).unwrap().equipment = Some(fixture_equipment());
    let equipment = render_model::pack_equipment_rig_id(
        render_model::find_geometry_index(&entities, EQUIPMENT_GEOMETRY).unwrap(),
    );
    let entity_geometries = render_model::pack_geometries(&entities);
    let entity_geometry = &entity_geometries[0];
    let equipment_geometry = render_model::equipment_geometry(
        &entities,
        render_model::find_geometry_index(&entities, EQUIPMENT_GEOMETRY).unwrap() as usize,
        equipment,
    )
    .unwrap();
    assert_eq!(
        entity_geometry.vertices.len(),
        equipment_geometry.vertices.len()
    );
    assert_ne!(entity_geometry.vertices, equipment_geometry.vertices);
    assert!(
        entity_geometries
            .iter()
            .all(|geometry| geometry.vertices != equipment_geometry.vertices),
        "equipment-only geometry must not be selected by an entity render route"
    );
    let mut scene = ActorRenderScene::with_runtime_entity_assets(&entities).unwrap();
    scene.replace_pack_entities(Some(&entities)).unwrap();
    scene.reset();
    let filler_count =
        render_model::MAX_ACTOR_CATALOG_VERTICES - scene.frame().rig.geometry_vertices.len();
    scene.replace_pack_entities(None).unwrap();
    let filler = |slot, count| {
        render_model::ActorRigGeometry::new(
            render_model::item_mesh_rig_id(slot),
            vec![
                render_model::ActorRigVertex {
                    position: [slot as f32 + 1.0; 3],
                    ..render_model::ActorRigVertex::default()
                };
                count
            ],
            vec![[0.0; 3]],
        )
        .unwrap()
    };
    let fillers = (0..filler_count)
        .step_by(render_model::MAX_ACTOR_RIG_VERTICES)
        .enumerate()
        .map(|(slot, start)| {
            filler(
                u32::try_from(slot).unwrap(),
                (filler_count - start).min(render_model::MAX_ACTOR_RIG_VERTICES),
            )
        })
        .collect();
    scene.insert_geometries(fillers).unwrap();
    let mut world = session_world(pack, artwork, scene);
    let original = publish(&mut world);
    assert_eq!(
        original.rig.geometry_vertices.len(),
        render_model::MAX_ACTOR_CATALOG_VERTICES
    );
    assert!(
        world
            .resource::<ActorRenderScene>()
            .contains_geometry(render_model::pack_rig_id(0))
    );
    assert!(
        !world
            .resource::<ActorRenderScene>()
            .contains_geometry(equipment)
    );
    world.resource_mut::<ClientWorld>().session_items = Some(Arc::new(SessionItems {
        components: Arc::default(),
        icons: None,
    }));
    let retried = publish(&mut world);
    assert!(Arc::ptr_eq(
        &retried.rig.geometry_vertices.segments,
        &original.rig.geometry_vertices.segments,
    ));
    assert!(
        !world
            .resource::<ActorRenderScene>()
            .contains_geometry(equipment)
    );
    world
        .resource_mut::<ActorRenderScene>()
        .insert_geometry(filler(0, 3))
        .unwrap();
    world.resource_mut::<ClientWorld>().session_items = Some(Arc::new(SessionItems {
        components: Arc::default(),
        icons: None,
    }));
    publish(&mut world);
    assert!(
        world
            .resource::<ActorRenderScene>()
            .contains_geometry(equipment)
    );
    assert!(
        world
            .resource::<ActorRenderScene>()
            .contains_geometry(render_model::pack_rig_id(0))
    );
}

/// Gives the fixture geometry a distinct, original texture and its normal pack route.
fn textured_pack(assets: Arc<assets::RuntimeEntityAssets>, color: u8) -> Arc<SessionEntityPack> {
    use sha2::{Digest, Sha256};
    let mut pack = session_pack(assets);
    let data = Arc::get_mut(&mut pack).unwrap();
    data.textures = Arc::from([assets::ActorTexture {
        source: 0,
        width: 1,
        height: 1,
        pixel_sha256: Sha256::digest([color; 4]).into(),
        rgba8: Arc::from([color; 4]),
    }]);
    data.bindings = Arc::from([assets::ActorArtworkBinding {
        rig: 0,
        geometry_candidate: 0,
        entity_symbol: 0,
        geometry: 0,
        render_controller: 0,
        texture: 0,
        material: "entity_alphatest".into(),
        pose_mode: assets::ActorPoseMode::CompiledLiteral,
    }]);
    pack
}

/// Compares every page byte and the pack's entity and variant routes.
fn same_artwork(actual: &ActorArtworkPages, expected: &ActorArtworkPages) {
    assert_eq!(actual.identity(), expected.identity());
    assert_eq!(actual.pages(), expected.pages());
    assert_eq!(actual.rejected_bindings(), expected.rejected_bindings());
    let rig = render_model::pack_rig_id(0);
    assert_eq!(actual.route(rig), expected.route(rig));
    assert_eq!(
        actual.variant_location(rig, 0),
        expected.variant_location(rig, 0)
    );
}

/// The first publication reuses worker pixels; a later pack or base change rejects stale preparation.
#[test]
fn prepared_actor_artwork_is_shared_and_stale_sources_fall_back() {
    use client_presentation::prepared_actor_artwork::PreparedActorArtwork;
    let (_fixture, artwork, entities) = super::actor_rest_presentation::compiled_fixture(
        "1.0",
        1,
        assets::ActorPoseMode::CompiledLiteral,
    );
    let pack = textured_pack(entities.clone(), 19);
    let expected = artwork
        .clone()
        .with_pack_artwork(&pack.textures, &pack.bindings);
    let base = artwork.clone();
    let input = pack.clone();
    let prepared = Arc::new(
        std::thread::spawn(move || PreparedActorArtwork::new(&base, &input))
            .join()
            .unwrap(),
    );
    let pages = prepared.pages_for(&artwork, &pack).unwrap();
    let pixel_page = pages.pages().last().unwrap().shared_pixels();
    let scene = ActorRenderScene::with_runtime_entity_assets(&entities).unwrap();
    let mut world = session_world(pack, artwork, scene);
    world.resource_mut::<ClientWorld>().prepared_actor_artwork = Some(prepared);
    let published = publish(&mut world);
    same_artwork(published.artwork_pages(), &expected);
    assert!(Arc::ptr_eq(
        &published
            .artwork_pages()
            .pages()
            .last()
            .unwrap()
            .shared_pixels(),
        &pixel_page,
    ));

    let replacement = textured_pack(entities.clone(), 71);
    let expected = world
        .resource::<ActorArtworkPages>()
        .clone()
        .with_pack_artwork(&replacement.textures, &replacement.bindings);
    world.resource_mut::<ClientWorld>().pack_entities = Some(replacement.clone());
    let changed_pack = publish(&mut world);
    same_artwork(changed_pack.artwork_pages(), &expected);
    assert!(!Arc::ptr_eq(
        &changed_pack
            .artwork_pages()
            .pages()
            .last()
            .unwrap()
            .shared_pixels(),
        &pixel_page,
    ));

    let prepared = Arc::new(PreparedActorArtwork::new(
        world.resource::<ActorArtworkPages>(),
        &replacement,
    ));
    world.resource_mut::<ClientWorld>().prepared_actor_artwork = Some(prepared);
    let (base, _) = world
        .resource::<ActorArtworkPages>()
        .clone()
        .with_equipment_rasters(&[render_model::equipment::EquipmentRaster {
            width: 1,
            height: 1,
            rgba8: Arc::from([43; 4]),
        }]);
    let expected = base
        .clone()
        .with_pack_artwork(&replacement.textures, &replacement.bindings);
    *world.resource_mut::<ActorArtworkPages>() = base.clone();
    let changed_base = publish(&mut world);
    same_artwork(changed_base.artwork_pages(), &expected);
    world.resource_mut::<ClientWorld>().stream = None;
    let disconnected = publish(&mut world);
    assert!(
        world
            .resource::<ClientWorld>()
            .prepared_actor_artwork
            .is_none()
    );
    same_artwork(disconnected.artwork_pages(), &base);
}
