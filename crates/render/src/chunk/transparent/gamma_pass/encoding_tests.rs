//! Executes the production transparent pass against a validation device.

use super::*;
use bevy::{
    camera::{CameraMainTextureUsages, RenderTarget},
    ecs::system::RunSystemOnce,
    math::UVec2,
    render::{
        render_phase::{
            Draw, DrawError, PhaseItemExtraIndex, SortedRenderPhase, TrackedRenderPass,
        },
        render_resource::*,
        renderer::{CurrentView, PendingCommandBuffers, RenderQueue},
        texture::{OutputColorAttachment, TextureCache},
        view::{RetainedViewEntity, ViewTargetAttachments, prepare_view_targets},
    },
};
use std::sync::Mutex;

#[derive(Resource, Default)]
struct EncodedItems(Mutex<Vec<Entity>>);

struct RecordItem;

impl Draw<Transparent3d> for RecordItem {
    fn draw<'w>(
        &mut self,
        world: &'w World,
        _pass: &mut TrackedRenderPass<'w>,
        _view: Entity,
        item: &Transparent3d,
    ) -> Result<(), DrawError> {
        world
            .resource::<EncodedItems>()
            .0
            .lock()
            .unwrap()
            .push(item.entity());
        Ok(())
    }
}

/// Creates the real view, scene and depth attachments without a display surface.
fn fixture(modes: &[bool]) -> (World, Entity, Vec<Entity>) {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let size = Extent3d {
        width: 4,
        height: 4,
        depth_or_array_layers: 1,
    };
    let output = device.create_texture(&TextureDescriptor {
        label: Some("transparent encoding output"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: render_scene::SCENE_COLOR_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let target = RenderTarget::Window(bevy::window::WindowRef::Entity(Entity::PLACEHOLDER))
        .normalize(None)
        .unwrap();
    let mut attachments = ViewTargetAttachments::default();
    attachments.insert(
        target.clone(),
        OutputColorAttachment::new(output.create_view(&Default::default()), output.format()),
    );
    let mut textures = TextureCache::default();
    let depth = textures.get(
        &device,
        TextureDescriptor {
            label: Some("transparent encoding depth"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
    );
    let mut world = World::new();
    world.insert_resource(device);
    world.insert_resource(RenderQueue::new(queue));
    world.insert_resource(attachments);
    world.insert_resource(textures);
    world.init_resource::<bevy::camera::ClearColor>();
    world.init_resource::<bevy::render::camera::CameraMainPassTextureFormats>();
    world.init_resource::<PendingCommandBuffers>();
    world.init_resource::<EncodedItems>();
    world.init_resource::<DrawFunctions<Transparent3d>>();
    let retained = RetainedViewEntity::new(Entity::PLACEHOLDER.into(), None, 0);
    let camera = world
        .spawn((
            Camera3d::default(),
            ExtractedCamera {
                target: Some(target),
                physical_target_size: Some(UVec2::splat(size.width)),
                physical_viewport_size: Some(UVec2::splat(size.width)),
                ..crate::render_test_support::camera(false)
            },
            ExtractedView {
                retained_view_entity: retained,
                clip_from_view: Mat4::IDENTITY,
                world_from_view: GlobalTransform::default(),
                clip_from_world: None,
                target_format: output.format(),
                viewport: UVec4::new(0, 0, size.width, size.height),
                color_grading: Default::default(),
                invert_culling: false,
            },
            CameraMainTextureUsages::default(),
            Msaa::Off,
            ViewDepthStencilTexture::new(depth, Some(0.0), None),
        ))
        .id();
    world.insert_resource(CurrentView(camera));
    world.run_system_once(prepare_view_targets).unwrap();
    world
        .run_system_once(crate::scene_target::prepare_scene_targets)
        .unwrap();
    let functions = world.resource::<DrawFunctions<Transparent3d>>();
    let mut functions = functions.write();
    let encoded = functions.add_with::<DrawTransparentLiquidCommands, _>(RecordItem);
    functions.add_with::<DrawTransparentLiquidDirectCommands, _>(RecordItem);
    functions.add_with::<DrawTransparentLiquidIndirectCommands, _>(RecordItem);
    functions.add_with::<DrawTransparentModelCommands, _>(RecordItem);
    functions.add_with::<crate::chunk::transparent::mixed::DrawMixedTerrainCommands, _>(RecordItem);
    let linear = functions.add(RecordItem);
    drop(functions);
    let mut phase = SortedRenderPhase::default();
    let items: Vec<_> = modes.iter().map(|_| world.spawn_empty().id()).collect();
    for (&entity, &gamma) in items.iter().zip(modes) {
        phase.add_retained(Transparent3d {
            sorting_info: bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
            distance: 0.0,
            pipeline: CachedRenderPipelineId::INVALID,
            entity: (entity, entity.into()),
            draw_function: if gamma { encoded } else { linear },
            batch_range: 0..1,
            extra_index: PhaseItemExtraIndex::None,
            indexed: false,
        });
    }
    let mut phases = ViewSortedRenderPhases::default();
    phases.insert(retained, phase);
    world.insert_resource(phases);
    (world, camera, items)
}

#[test]
fn mixed_ranges_share_one_command_buffer_and_keep_draw_order() {
    for modes in [&[true, true][..], &[true, false, true, false][..]] {
        let (mut world, _, items) = fixture(modes);
        world
            .run_system_once::<_, (), _>(gamma_transparent::<false>)
            .unwrap();
        let buffers = world
            .resource_mut::<PendingCommandBuffers>()
            .finish()
            .collect::<Vec<_>>();
        assert_eq!(buffers.len(), 1, "one sorted phase submission");
        assert_eq!(*world.resource::<EncodedItems>().0.lock().unwrap(), items);
    }
}

#[test]
fn outside_viewport_clears_once_without_drawing_items() {
    let (mut world, camera, _) = fixture(&[true, false, true]);
    world.get_mut::<ExtractedCamera>(camera).unwrap().viewport = Some(Viewport {
        physical_position: UVec2::splat(8),
        physical_size: UVec2::splat(4),
        ..Default::default()
    });
    world
        .run_system_once::<_, (), _>(gamma_transparent::<false>)
        .unwrap();
    assert_eq!(
        world
            .resource_mut::<PendingCommandBuffers>()
            .finish()
            .count(),
        1
    );
    assert!(
        world
            .resource::<EncodedItems>()
            .0
            .lock()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn empty_phase_submits_no_commands() {
    let (mut world, _, _) = fixture(&[]);
    world
        .run_system_once::<_, (), _>(gamma_transparent::<false>)
        .unwrap();
    assert_eq!(
        world
            .resource_mut::<PendingCommandBuffers>()
            .finish()
            .count(),
        0
    );
}
