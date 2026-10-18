use super::*;
use mod_render::retained::{Anchor, GeometrySpec, Instance, Store, Vertex};
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
