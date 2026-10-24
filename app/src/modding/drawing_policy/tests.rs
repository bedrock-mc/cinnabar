use super::*;
use bevy::{ecs::system::SystemState, prelude::World};
use chunk_pipeline::WorldStream;
use mod_api::drawing::DrawingPermissions;
use mod_host::{ModGrants, ModHost};
use mod_render::retained::{Anchor, GeometrySpec, Instance, Store, Vertex};
use protocol::WorldBootstrap;
use render_scene::mod_scene::retained::DrawingFrame;
use world::{ChunkStore, SubChunkKey};

#[test]
fn block_anchors_follow_their_subchunk_authority() {
    let loaded = [-1, -17, 16];
    let pending = [-1, -16, 16];
    let absent = [16, -17, 16];
    let key = SubChunkKey::new(0, -1, -2, 1);
    let mut terrain = ChunkStore::default();
    terrain.mark_sub_chunk_loaded(key).unwrap();
    assert!(!terrain.is_chunk_loaded(key.chunk()));
    assert!(!terrain.contains_sub_chunk(key));

    let mut store = Store::default();
    let geometry = store
        .create_geometry(GeometrySpec::Polyline {
            vertices: [[0.; 3], [1., 0., 0.]]
                .map(|position| Vertex {
                    position,
                    color: [1.; 4],
                    uv: [0.; 2],
                    data: [0.; 4],
                })
                .to_vec(),
            closed: false,
        })
        .unwrap();
    for (dimension, position) in [(0, loaded), (0, pending), (0, absent), (1, absent)] {
        store
            .create(
                Instance {
                    geometry,
                    material: None,
                    params: mod_api::materials::Preset::default().0,
                    anchor: Anchor::Block {
                        dimension,
                        position,
                    },
                    transform: Default::default(),
                    tint: [1.; 4],
                    depth: Default::default(),
                    blend: Default::default(),
                    width: 1.,
                    width_mode: mod_render::retained::WidthMode::World,
                    join: mod_render::retained::Join::Bevel,
                    cap: mod_render::retained::Cap::Butt,
                },
                DrawingPermissions::ALL,
            )
            .unwrap();
    }
    store.commit();
    terrain
        .mark_chunk_loaded(world::ChunkKey::new(1, 1, 1))
        .unwrap();
    let mut frame = DrawingFrame::default();
    publish_retained(store.snapshot(), &terrain, &mut frame);
    assert_eq!(frame.loaded_blocks, [loaded].into());
    assert_eq!(
        frame.live_owners,
        [(geometry.owner, geometry.generation)].into()
    );

    terrain.mark_chunk_loaded(key.chunk()).unwrap();
    let mut frame = DrawingFrame::default();
    publish_retained(store.snapshot(), &terrain, &mut frame);
    assert_eq!(frame.loaded_blocks, [loaded, pending].into());

    terrain.evict_chunk(key.chunk());
    let mut frame = DrawingFrame::default();
    publish_retained(store.snapshot(), &terrain, &mut frame);
    assert!(frame.loaded_blocks.is_empty());
}

#[test]
fn material_only_snapshots_publish_their_live_owner() {
    let mut store = Store::default();
    let material = store
        .create_material(mod_render::material::BUILTIN, DrawingPermissions::ALL)
        .unwrap();
    store.commit();
    assert!(store.snapshot().geometry.is_empty());

    let mut frame = DrawingFrame::default();
    publish_retained(store.snapshot(), &ChunkStore::default(), &mut frame);
    assert_eq!(
        frame.live_owners,
        [(material.owner, material.generation)].into()
    );

    store.begin();
    store.remove(material).unwrap();
    store.commit();
    let mut frame = DrawingFrame::default();
    publish_retained(store.snapshot(), &ChunkStore::default(), &mut frame);
    assert!(frame.live_owners.is_empty());
}

/// Supplies a block-anchored contributor bound to the specified native session.
fn anchored_resources(
    connection: u64,
    position: [i32; 3],
) -> std::sync::Arc<mod_render::retained::Resources> {
    let mut store = Store::default();
    store.bind_session(connection, 0);
    let geometry = store
        .create_geometry(GeometrySpec::Polyline {
            vertices: [[0.; 3], [1., 0., 0.]]
                .map(|position| Vertex {
                    position,
                    color: [1.; 4],
                    uv: [0.; 2],
                    data: [0.; 4],
                })
                .to_vec(),
            closed: false,
        })
        .unwrap();
    store
        .create(
            Instance {
                geometry,
                material: None,
                params: mod_api::materials::Preset::default().0,
                anchor: Anchor::Block {
                    dimension: 0,
                    position,
                },
                transform: Default::default(),
                tint: [1.; 4],
                depth: Default::default(),
                blend: Default::default(),
                width: 1.,
                width_mode: mod_render::retained::WidthMode::World,
                join: mod_render::retained::Join::Bevel,
                cap: mod_render::retained::Cap::Butt,
            },
            DrawingPermissions::ALL,
        )
        .unwrap();
    store.commit();
    std::sync::Arc::clone(store.snapshot())
}

#[test]
fn server_only_anchors_disappear_on_unload_and_session_change() {
    let mut authority = client_world::WorldAuthority::new(
        bootstrap(),
        std::sync::Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.; 3],
        None,
    );
    let key = SubChunkKey::new(0, 0, 0, 0);
    authority.mark_sub_chunk_loaded(key).unwrap();
    let server = anchored_resources(authority.actor_session_id(), [0; 3]);
    let mut frame = DrawingFrame::default();
    rebuild_frame([&server], Some(&authority), &mut frame);
    assert_eq!(frame.loaded_blocks, [[0; 3]].into());
    let instance = server.instances.values().next().unwrap();
    let camera = bevy::prelude::GlobalTransform::IDENTITY;
    assert!(render_scene::mod_scene::retained::resolve(instance, &frame, &camera).is_some());

    authority.detach_chunks(&[key.chunk()].into());
    rebuild_frame([&server], Some(&authority), &mut frame);
    assert!(frame.loaded_blocks.is_empty());
    assert!(render_scene::mod_scene::retained::resolve(instance, &frame, &camera).is_none());

    authority.mark_sub_chunk_loaded(key).unwrap();
    rebuild_frame([&server], Some(&authority), &mut frame);
    assert_eq!(frame.loaded_blocks.len(), 1);
    let mut replacement = client_world::WorldAuthority::new(
        bootstrap(),
        std::sync::Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.; 3],
        None,
    );
    replacement.mark_sub_chunk_loaded(key).unwrap();
    rebuild_frame([&server], Some(&replacement), &mut frame);
    assert!(frame.loaded_blocks.is_empty());
    assert!(frame.live_owners.is_empty());
    assert_eq!(frame.connection, replacement.actor_session_id());

    rebuild_frame([&server], None, &mut frame);
    assert_eq!(frame, DrawingFrame::default());
}

#[test]
fn frame_rebuild_keeps_all_current_contributors_and_removes_only_retired_work() {
    let mut authority = client_world::WorldAuthority::new(
        bootstrap(),
        std::sync::Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.; 3],
        None,
    );
    authority
        .mark_sub_chunk_loaded(SubChunkKey::new(0, 0, 0, 0))
        .unwrap();
    let personal = anchored_resources(authority.actor_session_id(), [0; 3]);
    let server = anchored_resources(authority.actor_session_id(), [1; 3]);
    let mut frame = DrawingFrame::default();
    rebuild_frame([&personal, &server], Some(&authority), &mut frame);
    assert_eq!(frame.loaded_blocks, [[0; 3], [1; 3]].into());
    assert_eq!(frame.live_owners.len(), 2);
    rebuild_frame([&server], Some(&authority), &mut frame);
    assert_eq!(frame.loaded_blocks, [[1; 3]].into());
    assert_eq!(frame.live_owners.len(), 1);
}

/// Supplies the native world bootstrap shared by stream and availability fixtures.
fn bootstrap() -> WorldBootstrap {
    WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    }
}

/// Builds a world stream with an independently allocated actor session.
fn stream() -> WorldStream {
    WorldStream::new(bootstrap())
}

#[test]
fn drawing_fence_tracks_actor_incarnations_without_server_negotiation() {
    let bytes = br#"(component
        (core module $m (func (export "init")) (func (export "frame")))
        (core instance $i (instantiate $m))
        (func (export "init") (canon lift (core func $i "init")))
        (func (export "frame") (canon lift (core func $i "frame"))))"#;
    let directory = tempfile::tempdir().unwrap();
    let host = ModHost::load_snapshot_with_grants(
        &directory.path().join("selection.wasm"),
        bytes,
        ModGrants::default(),
    )
    .unwrap();
    let mut hosts = ModHosts::new(host, vec![]);
    let stream = stream();
    let actor_session = stream.authority().actor_session_id();
    let mut world = World::new();
    world.insert_resource(crate::runtime::world::ClientWorld {
        stream: Some(stream),
        ..Default::default()
    });
    let mut state = SystemState::<super::super::gameplay::GameplayContext>::new(&mut world);
    let mut frame = DrawingFrame::default();
    {
        let gameplay = state.get_mut(&mut world).unwrap();
        bind(&mut hosts, &gameplay);
    }
    assert_eq!(
        hosts.host(0).drawing_session().unwrap().connection,
        actor_session
    );
    assert_eq!(
        hosts.host(0).drawing_session().unwrap().permissions,
        DrawingPermissions::ALL
    );
    assert_eq!(hosts.host(0).grants(), &ModGrants::default());

    rebuild_frame(
        [],
        world
            .resource::<crate::runtime::world::ClientWorld>()
            .stream
            .as_ref()
            .map(|s| s.authority()),
        &mut frame,
    );
    assert_eq!(frame.connection, actor_session);

    let replacement = self::stream();
    let replacement_session = replacement.authority().actor_session_id();
    world
        .resource_mut::<crate::runtime::world::ClientWorld>()
        .stream = Some(replacement);
    let mut frame = DrawingFrame::default();
    {
        let gameplay = state.get_mut(&mut world).unwrap();
        bind(&mut hosts, &gameplay);
    }
    assert_eq!(
        hosts.host(0).drawing_session().unwrap().connection,
        replacement_session
    );
    rebuild_frame(
        [],
        world
            .resource::<crate::runtime::world::ClientWorld>()
            .stream
            .as_ref()
            .map(|s| s.authority()),
        &mut frame,
    );
    assert_eq!(frame.connection, replacement_session);
    assert_ne!(actor_session, replacement_session);
    world
        .resource_mut::<crate::runtime::world::ClientWorld>()
        .stream = None;
    let mut frame = DrawingFrame::default();
    {
        let gameplay = state.get_mut(&mut world).unwrap();
        bind(&mut hosts, &gameplay);
    }
    assert_eq!(hosts.host(0).drawing_session().unwrap().connection, 0);
    assert_eq!(
        hosts.host(0).drawing_session().unwrap().permissions,
        DrawingPermissions::default()
    );
    rebuild_frame([], None, &mut frame);
    assert_eq!(frame.connection, 0);
}
