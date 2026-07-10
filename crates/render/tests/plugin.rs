use bevy::{
    camera::primitives::Aabb,
    prelude::{App, MinimalPlugins, Vec3, Visibility},
};
use render::{
    BlockClassifier, ChunkRenderInstance, ChunkRenderQueue, ChunkUploadPriority, DebugWorldPlugin,
    Neighbourhood, mesh_sub_chunk,
};
use world::{SubChunk, SubChunkKey};

const AIR: u32 = 12_530;

fn zig_zag_i32(value: i32) -> Vec<u8> {
    let mut value = ((value as u32) << 1) ^ ((value >> 31) as u32);
    let mut encoded = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            return encoded;
        }
    }
}

fn solid_mesh(runtime_id: u32) -> render::ChunkMesh {
    let mut encoded = vec![9, 1, 0, 1];
    encoded.extend(zig_zag_i32(runtime_id as i32));
    let sub_chunk = SubChunk::decode(&encoded).expect("uniform sub-chunk");
    mesh_sub_chunk(
        &BlockClassifier::new(AIR),
        &Neighbourhood::empty(),
        &sub_chunk,
    )
}

#[test]
fn upload_budget_is_nearest_first_and_queue_supports_update_remove() {
    let near = SubChunkKey::new(0, 1, 2, 3);
    let far = SubChunkKey::new(0, 20, 2, 20);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(DebugWorldPlugin::new(1));

    {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        queue.insert(far, solid_mesh(7), ChunkUploadPriority::new(100.0));
        queue.insert(near, solid_mesh(11), ChunkUploadPriority::new(1.0));
    }
    app.update();

    let rendered = app
        .world_mut()
        .query::<&ChunkRenderInstance>()
        .iter(app.world())
        .map(|instance| {
            (
                instance.key(),
                instance.quad_count(),
                instance.quads()[0].runtime_id(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered, vec![(near, 6, 11)]);
    assert_eq!(
        app.world()
            .resource::<ChunkRenderQueue>()
            .gpu_upload_bytes(),
        64
    );
    assert_eq!(
        app.world_mut()
            .query::<(&Aabb, &Visibility)>()
            .iter(app.world())
            .count(),
        1,
    );

    app.update();
    assert_eq!(
        app.world_mut()
            .query::<&ChunkRenderInstance>()
            .iter(app.world())
            .count(),
        2
    );
    assert_eq!(
        app.world()
            .resource::<ChunkRenderQueue>()
            .gpu_upload_bytes(),
        128
    );

    {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        queue.update(far, solid_mesh(13), ChunkUploadPriority::new(0.0));
        queue.remove(near);
    }
    app.update();

    let rendered = app
        .world_mut()
        .query::<&ChunkRenderInstance>()
        .iter(app.world())
        .map(|instance| {
            (
                instance.key(),
                instance.quad_count(),
                instance.quads()[0].runtime_id(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(rendered, vec![(far, 6, 13)]);
    assert_eq!(
        app.world()
            .resource::<ChunkRenderQueue>()
            .gpu_upload_bytes(),
        192
    );

    assert!(
        ChunkUploadPriority::from_camera(near, Vec3::splat(16.0))
            < ChunkUploadPriority::from_camera(far, Vec3::splat(16.0))
    );
}

#[test]
fn packed_chunk_shader_parses_and_validates() {
    let shader = include_str!("../src/chunk.wgsl").replacen(
        "#import bevy_render::view::View",
        "struct View { clip_from_world: mat4x4<f32>, }",
        1,
    );
    let module = naga::front::wgsl::parse_str(&shader).expect("parse packed chunk WGSL");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("validate packed chunk WGSL");
}
