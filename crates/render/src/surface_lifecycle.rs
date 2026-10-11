use bevy::{
    app::SubApp,
    prelude::*,
    render::{
        Render, RenderSystems,
        camera::ExtractedCamera,
        view::{ViewTarget, window::create_surfaces},
    },
};

pub(crate) fn install(render_app: &mut SubApp) {
    render_app.add_systems(
        Render,
        release_orphan_targets::<ViewTarget>
            .in_set(RenderSystems::PrepareViews)
            .before(create_surfaces),
    );
}

/// Inactive and zero-size cameras lose `ExtractedCamera` during extraction, but Bevy
/// retains their `ViewTarget`. Its output view must drop before DXGI resizes the window.
/// Bevy's ordinary resize cleanup only sees targets with an extracted camera.
fn release_orphan_targets<T: Component>(
    mut commands: Commands,
    targets: Query<Entity, (With<T>, Without<ExtractedCamera>)>,
) {
    for entity in &targets {
        commands.entity(entity).remove::<T>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        camera::{CameraOutputMode, ClearColorConfig, MsaaWriteback},
        core_pipeline::Core3d,
        ecs::schedule::ScheduleLabel,
    };
    use std::sync::{Arc, Weak};

    #[derive(Component)]
    struct RetainedTarget(
        #[expect(dead_code, reason = "retains the surface ownership fixture")] Arc<()>,
    );

    #[derive(Resource)]
    struct SurfaceOwner(Weak<()>);

    fn configure_surface(owner: Res<SurfaceOwner>) {
        assert!(
            owner.0.upgrade().is_none(),
            "resize retained the old surface view"
        );
    }

    fn camera() -> ExtractedCamera {
        ExtractedCamera {
            target: None,
            physical_viewport_size: None,
            physical_target_size: None,
            viewport: None,
            schedule: Core3d.intern(),
            order: 0,
            output_mode: CameraOutputMode::default(),
            msaa_writeback: MsaaWriteback::default(),
            clear_color: ClearColorConfig::default(),
            sorted_camera_index_for_target: 0,
            exposure: 1.0,
            hdr: false,
            compositing_space: None,
        }
    }

    #[test]
    fn camera_deactivation_releases_surface_views_before_reconfiguration() {
        let mut world = World::new();
        let retained = Arc::new(());
        world.insert_resource(SurfaceOwner(Arc::downgrade(&retained)));
        let entity = world.spawn((RetainedTarget(retained), camera())).id();
        let mut schedule = Schedule::default();
        schedule.add_systems(release_orphan_targets::<RetainedTarget>);
        schedule.run(&mut world);
        assert!(world.entity(entity).contains::<RetainedTarget>());

        world.entity_mut(entity).remove::<ExtractedCamera>();
        schedule.add_systems(configure_surface.after(release_orphan_targets::<RetainedTarget>));
        schedule.run(&mut world);
        assert!(!world.entity(entity).contains::<RetainedTarget>());
    }

    #[test]
    #[ignore = "requires a native GPU"]
    fn native_gpu_orphan_target_survives_bevy_resize_cleanup_and_is_released() {
        use bevy::{
            camera::{CameraMainTextureUsages, NormalizedRenderTarget, RenderTarget},
            render::{
                render_resource::TextureView,
                renderer::RenderDevice,
                texture::{OutputColorAttachment, TextureCache},
                view::{
                    ExtractedView, Msaa, RetainedViewEntity, ViewTargetAttachments,
                    cleanup_view_targets_for_resize, prepare_view_targets,
                    window::ExtractedWindows,
                },
            },
        };
        const SIDE: u32 = 8;
        let gpu = crate::gpu_snapshot::Gpu::new().unwrap();
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("surface ownership regression"),
            size: wgpu::Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let output = TextureView::from(texture.create_view(&Default::default()));
        let target: NormalizedRenderTarget =
            RenderTarget::Window(bevy::window::WindowRef::Entity(Entity::PLACEHOLDER))
                .normalize(None)
                .unwrap();
        let mut attachments = ViewTargetAttachments::default();
        attachments.insert(
            target.clone(),
            OutputColorAttachment::new(output, texture.format()),
        );
        let mut world = World::new();
        world.insert_resource(RenderDevice::from(gpu.device));
        world.insert_resource(TextureCache::default());
        world.insert_resource(ClearColor::default());
        world.insert_resource(attachments);
        world.insert_resource(ExtractedWindows::default());
        let mut camera = camera();
        camera.target = Some(target);
        camera.physical_target_size = Some(UVec2::splat(SIDE));
        let entity = world
            .spawn((
                camera,
                ExtractedView {
                    retained_view_entity: RetainedViewEntity::new(
                        Entity::PLACEHOLDER.into(),
                        None,
                        0,
                    ),
                    clip_from_view: Mat4::IDENTITY,
                    world_from_view: GlobalTransform::default(),
                    clip_from_world: None,
                    target_format: crate::SCENE_COLOR_FORMAT,
                    viewport: UVec4::new(0, 0, SIDE, SIDE),
                    color_grading: Default::default(),
                    invert_culling: false,
                },
                CameraMainTextureUsages::default(),
                Msaa::Off,
            ))
            .id();
        let mut prepare = Schedule::default();
        prepare.add_systems(prepare_view_targets);
        prepare.run(&mut world);
        assert!(world.entity(entity).contains::<ViewTarget>());
        world.resource_mut::<ViewTargetAttachments>().clear();
        world.entity_mut(entity).remove::<ExtractedCamera>();
        let mut bevy_cleanup = Schedule::default();
        bevy_cleanup.add_systems(cleanup_view_targets_for_resize);
        bevy_cleanup.run(&mut world);
        assert!(
            world.entity(entity).contains::<ViewTarget>(),
            "pinned Bevy retains the orphan"
        );
        let mut cleanup = Schedule::default();
        cleanup.add_systems(release_orphan_targets::<ViewTarget>);
        cleanup.run(&mut world);
        assert!(!world.entity(entity).contains::<ViewTarget>());
    }
}
