//! Bounded native render-graph coverage using the installed bamboo geometry and art.
use bevy::{
    asset::AssetPlugin,
    camera::{Camera3dDepthTextureUsage, CameraPlugin, RenderTarget},
    core_pipeline::{CorePipelinePlugin, tonemapping::Tonemapping},
    mesh::MeshPlugin,
    post_process::{PostProcessPlugin, bloom::Bloom},
    prelude::*,
    render::{
        RenderApp, RenderPlugin,
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::*,
        renderer::RenderDevice,
        view::Hdr,
    },
    window::WindowPlugin,
};
use std::{path::PathBuf, sync::Arc};

const SIZE: [u32; 2] = [256, 192];

#[derive(Resource, Default)]
struct Capture(Vec<u8>);

/// Reads the requested installed carrier, skipping only an absent fixture.
fn fixture() -> Option<(
    Vec<assets::RegistryRecord>,
    Arc<assets::RuntimeAssets>,
    assets::RuntimeBlockEntityAssets,
)> {
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../assets/bedrock-target.json")).unwrap();
    let path = std::env::var_os("RUST_MCBE_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(target["artifacts"]["world_assets"].as_str().unwrap())
        });
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "missing fixture: Enhanced bamboo world carrier {}",
                path.display()
            );
            return None;
        }
        Err(error) => panic!("Enhanced bamboo carrier {}: {error}", path.display()),
    };
    let records = assets::read_registry_for_protocol(
        include_bytes!("../../../assets/data/block-registry-v2193.bin"),
        target["wire_protocol"].as_u64().unwrap() as u32,
    )
    .unwrap()
    .into_vec();
    let block_entities = path
        .parent()
        .unwrap()
        .join(assets::carriers::BLOCK_ENTITY.output);
    let entity_bytes = match std::fs::read(&block_entities) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "missing fixture: Enhanced bamboo block entity carrier {}",
                block_entities.display()
            );
            return None;
        }
        Err(error) => panic!(
            "Enhanced bamboo carrier {}: {error}",
            block_entities.display()
        ),
    };
    Some((
        records,
        Arc::new(assets::RuntimeAssets::decode(&bytes).unwrap()),
        assets::RuntimeBlockEntityAssets::decode(&entity_bytes).unwrap(),
    ))
}

/// Meshes real stalk states and a receiving floor through the production mesher.
fn mesh(
    records: &[assets::RegistryRecord],
    assets: &assets::RuntimeAssets,
    overrides: [u32; 2],
) -> meshing::ChunkMesh {
    let air = records
        .iter()
        .find(|record| record.flags.contains(assets::BlockFlags::AIR))
        .unwrap()
        .sequential_id;
    let stone = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:stone")
        .unwrap()
        .sequential_id;
    let stalks: Vec<_> = records
        .iter()
        .filter_map(|record| {
            assets::bamboo::BambooState::from_record(record)
                .map(|state| (record.sequential_id, state))
        })
        .collect();
    assert!(!stalks.is_empty());
    let key = world::SubChunkKey::new(0, 0, 0, 0);
    let mut store = world::ChunkStore::new();
    store.mark_sub_chunk_loaded(key).unwrap();
    for x in 0..10 {
        for z in 0..10 {
            store
                .update_block(key, world::BlockUpdate::new(x, 0, z, 0, stone), air)
                .unwrap();
        }
    }
    for (index, (id, state)) in stalks.iter().enumerate() {
        let x = 2 + (index % 4) as u8 * 2;
        let z = 2 + (index / 4) as u8 * 2;
        let bare = stalks
            .iter()
            .find(|(_, other)| {
                other.thick == state.thick && other.leaves == assets::bamboo::LeafSize::None
            })
            .unwrap()
            .0;
        for y in 1..=4 {
            store
                .update_block(
                    key,
                    world::BlockUpdate::new(x, y, z, 0, if y == 4 { *id } else { bare }),
                    air,
                )
                .unwrap();
        }
    }
    for (index, id) in overrides.into_iter().enumerate() {
        store
            .update_block(
                key,
                world::BlockUpdate::new(1 + index as u8 * 2, 3, 8, 0, id),
                air,
            )
            .unwrap();
    }
    let mesh = meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(air),
        assets,
        assets::NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &store.sub_chunk(key).unwrap(),
    );
    assert_eq!(mesh.model_refs().len(), stalks.len() * 4 + overrides.len());
    assert_eq!(
        mesh.model_refs()
            .iter()
            .filter(|reference| {
                reference.words()[0] & meshing::MODEL_REF_FLAG_RANDOM_OFFSET != 0
            })
            .count(),
        overrides.len()
    );
    assert!(!mesh.model_draw_refs().is_empty());
    mesh
}

/// Adds real bamboo geometry and art with nonzero and explicit-zero session components.
fn overridden_assets(
    records: &[assets::RegistryRecord],
    assets: &assets::RuntimeAssets,
) -> (Arc<assets::RuntimeAssets>, [u32; 2]) {
    let record = records
        .iter()
        .find(|record| {
            assets::bamboo::BambooState::from_record(record)
                .is_some_and(|state| state.leaves == assets::bamboo::LeafSize::Large)
        })
        .unwrap();
    let visual = assets.resolve(assets::NetworkIdMode::Sequential, record.sequential_id);
    let template = assets.model_templates()[visual.model_template().unwrap() as usize];
    let mut overlay = assets::BlockOverlay {
        texture: Some(assets.texture_pages()[0].texture.clone()),
        ..Default::default()
    };
    let mut materials = std::collections::HashMap::new();
    let mut remap = |id: u32| {
        *materials.entry(id).or_insert_with(|| {
            let mut material = assets.materials()[id as usize];
            material.texture = assets::TextureRef::new(1, material.texture.layer()).unwrap();
            assert_eq!(material.animation, assets::NO_ANIMATION);
            let local = overlay.materials.len() as u32;
            overlay.materials.push(material);
            local
        })
    };
    let faces = assets::BlockFace::ALL.map(|face| remap(visual.face(face).material_id()));
    for quad in &assets.model_quads()
        [template.quad_start as usize..(template.quad_start + template.quad_count) as usize]
    {
        overlay.model_quads.push(assets::ModelQuad {
            material: remap(quad.material),
            ..*quad
        });
    }
    let quads = overlay.model_quads.clone();
    for (index, component) in [block_transform::random_offset::BAMBOO, Default::default()]
        .into_iter()
        .enumerate()
    {
        if index != 0 {
            overlay.model_quads.extend_from_slice(&quads);
        }
        overlay.model_templates.push(assets::ModelTemplate {
            quad_start: index as u32 * template.quad_count,
            quad_count: template.quad_count,
            flags: 0,
        });
        overlay.model_random_offsets.push((index as u32, component));
        overlay.visuals.push(assets::BlockVisual {
            faces,
            flags: visual.flags(),
            kind: visual.kind(),
            support: visual.support(),
            contributor_role: visual.contributor_role(),
            model_template: index as u32,
            animation: assets::NO_ANIMATION,
            variant: visual.variant(),
        });
        overlay.light_properties.push(visual.light_properties());
    }
    let first = records.len() as u32;
    (
        Arc::new(assets.with_block_overlay(first, &overlay).unwrap()),
        [first, first + 1],
    )
}

/// Builds the actual HDR, snapshot, shadow, post, hand and UI graph without a window.
fn app(assets: Arc<assets::RuntimeAssets>) -> (App, Entity) {
    render_model::enable_enhanced_diagnostics(SIZE).unwrap();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .add_plugins(WindowPlugin {
            primary_window: None,
            ..default()
        })
        .add_plugins(AssetPlugin::default())
        .add_plugins(RenderPlugin {
            synchronous_pipeline_compilation: true,
            ..default()
        })
        .add_plugins((
            ImagePlugin::default(),
            MeshPlugin,
            CameraPlugin,
            CorePipelinePlugin,
            PostProcessPlugin,
        ))
        .add_plugins((
            render::ChunkRenderPlugin::new(1),
            render::AtmospherePlugin,
            render::ActorRenderPlugin,
            render::UiRenderPlugin,
            render::ViewmodelRenderPlugin,
            render::HandRigRenderPlugin,
            render::ParticleRenderPlugin,
            render::BlockEntityRenderPlugin,
            render::EnhancedRenderPlugin,
        ))
        .insert_resource(render::ChunkTextureAssets::new(assets))
        .insert_resource(render::AtmosphereFrame::from_bedrock_time(2500.0, 0.0, 0.0))
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::ZERO,
        ))
        .init_resource::<Capture>();
    let mut image = Image::new_target_texture(
        SIZE[0],
        SIZE[1],
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    );
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let camera = app
        .world_mut()
        .spawn((
            Camera3d {
                depth_texture_usages: Camera3dDepthTextureUsage::from(
                    TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                ),
                ..default()
            },
            Camera::default(),
            RenderTarget::Image(image.clone().into()),
            Msaa::Off,
            Hdr,
            Tonemapping::None,
            Bloom::default(),
            render::EnhancedRendering::bounded_diagnostic(),
            Transform::from_xyz(12.0, 8.0, 14.0).looking_at(Vec3::new(5.0, 2.0, 5.0), Vec3::Y),
        ))
        .id();
    app.world_mut().spawn(Readback::texture(image)).observe(
        |event: On<ReadbackComplete>, mut capture: ResMut<Capture>| {
            capture.0.clone_from(&event.data);
        },
    );
    app.finish();
    app.cleanup();
    (app, camera)
}

/// Completes bounded frames and rejects pipeline or device failures before returning pixels.
fn capture(app: &mut App) -> Vec<u8> {
    for _ in 0..8 {
        app.update();
        app.sub_app(RenderApp)
            .world()
            .resource::<RenderDevice>()
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
    }
    let cache = app.sub_app(RenderApp).world().resource::<PipelineCache>();
    assert_eq!(cache.waiting_pipelines().count(), 0);
    for pipeline in cache.pipelines() {
        if let CachedPipelineState::Err(error) = &pipeline.state {
            panic!("Enhanced bamboo pipeline: {error}");
        }
    }
    let pixels = app.world().resource::<Capture>().0.clone();
    assert_eq!(pixels.len(), (SIZE[0] * SIZE[1] * 4) as usize);
    pixels
}

/// Writes requested diagnostic frames outside the repository.
fn save(name: &str, pixels: &[u8]) {
    if let Some(directory) = std::env::var_os("CINNABAR_REVIEW_SNAPSHOT_DIR") {
        image::save_buffer(
            PathBuf::from(directory).join(format!("{name}.png")),
            pixels,
            SIZE[0],
            SIZE[1],
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}

#[test]
fn enhanced_bamboo_uses_the_production_graph_under_changing_views_and_shadows() {
    let Some((records, assets, block_entities)) = fixture() else {
        return;
    };
    if crate::gpu_snapshot::Gpu::for_fixture("Enhanced bamboo production graph").is_none() {
        return;
    }
    let (assets, overrides) = overridden_assets(&records, &assets);
    let mesh = mesh(&records, &assets, overrides);
    let (mut app, camera) = app(Arc::clone(&assets));
    let empty = capture(&mut app);
    app.world_mut()
        .resource_mut::<render::ChunkRenderQueue>()
        .try_insert(
            world::SubChunkKey::new(0, 0, 0, 0),
            mesh,
            render::ChunkUploadPriority::new(0.0),
        )
        .unwrap();
    let first = capture(&mut app);
    save("bamboo-enhanced-empty", &empty);
    save("bamboo-enhanced-front", &first);
    assert!(
        first
            .chunks_exact(4)
            .zip(empty.chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count()
            > 100
    );
    let stalk = records
        .iter()
        .find(|record| {
            assets::bamboo::BambooState::from_record(record)
                .is_some_and(|state| state.leaves == assets::bamboo::LeafSize::Large)
        })
        .unwrap();
    let index = records
        .iter()
        .filter(|record| assets::bamboo::BambooState::from_record(record).is_some())
        .position(|record| record.sequential_id == stalk.sequential_id)
        .unwrap();
    let block = [2 + (index % 4) as i32 * 2, 4, 2 + (index / 4) as i32 * 2];
    let visual = assets.resolve(assets::NetworkIdMode::Sequential, stalk.sequential_id);
    let shape = render::crack_shape_from_template(
        &assets,
        visual.model_template().unwrap(),
        visual.variant(),
        block,
    )
    .unwrap();
    let bounds = [block.map(|v| v as f32), block.map(|v| v as f32 + 1.0)];
    let original_view = *app.world().get::<Transform>(camera).unwrap();
    let center = Vec3::from_array(block.map(|value| value as f32))
        + Vec3::splat(0.5)
        + Vec3::from_array(block_transform::bamboo::column_offset(block));
    *app.world_mut().get_mut::<Transform>(camera).unwrap() =
        Transform::from_translation(center + Vec3::new(1.5, 0.4, 2.5)).looking_at(center, Vec3::Y);
    let close = capture(&mut app);
    save("bamboo-enhanced-close", &close);
    let target = render::BlockSelectionTarget {
        block,
        bounds,
        shape: shape.clone(),
    };
    app.world_mut()
        .resource_mut::<render::BlockSelectionFrame>()
        .update(Some(&target), false);
    let highlighted = capture(&mut app);
    assert_ne!(
        close, highlighted,
        "Enhanced model-face selection must reach the HDR scene"
    );
    save("bamboo-enhanced-highlight", &highlighted);
    app.world_mut()
        .resource_mut::<render::BlockSelectionFrame>()
        .update(None, false);
    let cracked = {
        let mut scene = app.world_mut().resource_mut::<render::BlockEntityScene>();
        scene.install_assets(&block_entities);
        scene
            .update(
                render::SceneClock::default(),
                &[render::CrackInstance {
                    block,
                    stage: 8,
                    shape,
                }],
                &[],
            )
            .clone()
    };
    assert!(!cracked.crack.is_empty());
    app.world_mut().insert_resource(cracked);
    let cracked = capture(&mut app);
    assert_ne!(
        close, cracked,
        "Enhanced destroy-stage faces must reach the HDR scene"
    );
    save("bamboo-enhanced-cracks", &cracked);
    app.world_mut()
        .insert_resource(render::BlockEntityFrame::default());
    *app.world_mut().get_mut::<Transform>(camera).unwrap() = original_view;
    app.world_mut()
        .get_mut::<render::EnhancedRendering>(camera)
        .unwrap()
        .shadows = false;
    let unshadowed = capture(&mut app);
    assert_ne!(
        first, unshadowed,
        "real shadow receiving must affect the scene"
    );
    save("bamboo-enhanced-no-shadows", &unshadowed);
    app.world_mut()
        .get_mut::<render::EnhancedRendering>(camera)
        .unwrap()
        .shadows = true;
    for (index, eye) in [Vec3::new(-4.0, 6.0, 5.0), Vec3::new(5.0, 9.0, -4.0)]
        .into_iter()
        .enumerate()
    {
        *app.world_mut().get_mut::<Transform>(camera).unwrap() =
            Transform::from_translation(eye).looking_at(Vec3::new(5.0, 2.0, 5.0), Vec3::Y);
        let pixels = capture(&mut app);
        assert_ne!(pixels, first);
        save(&format!("bamboo-enhanced-angle-{index}"), &pixels);
    }
}
