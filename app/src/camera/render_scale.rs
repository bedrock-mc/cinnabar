//! Scales the 3D view; screen-space UI is composited later at native output resolution.
use bevy::{
    camera::{CameraOutputMode, MainPassResolutionOverride},
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        camera::{ExtractedCamera, sort_cameras},
        sync_world::RenderEntity,
        view::ExtractedView,
    },
};
use client_presentation::camera::FlyCamera;
use client_state::{PlayerRuntime, RuntimeSettings};

const MIN_RENDER_SCALE: f32 = ui::MIN_RENDER_SCALE_PERCENT as f32 / 100.0;

#[derive(Resource)]
struct RenderScaleOverride(Option<f32>);

#[derive(Component)]
struct RenderScaleRequest(f32);

/// Resolves the saved setting during extraction while retaining a diagnostic override.
pub(super) fn install(app: &mut App) {
    let scale = parse_scale(std::env::var("CINNABAR_RENDER_SCALE").ok().as_deref());
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .insert_resource(RenderScaleOverride(scale))
        .add_systems(ExtractSchedule, extract_scale_requests)
        .add_systems(
            Render,
            scale_extracted_camera
                .in_set(RenderSystems::CreateViews)
                .in_set(render::RenderScaleSystems)
                .before(sort_cameras),
        );
}

/// Invalid overrides retain the saved setting instead of creating empty or oversized attachments.
fn parse_scale(value: Option<&str>) -> Option<f32> {
    value
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && (MIN_RENDER_SCALE..=1.0).contains(value))
}

/// Bounds rounding at small window sizes while keeping every attachment nonempty.
fn scaled_size(size: UVec2, scale: f32) -> UVec2 {
    (size.as_vec2() * scale).round().as_uvec2().max(UVec2::ONE)
}

/// Defers requests alongside camera extraction, before render-world camera creation completes.
fn extract_scale_requests(
    mut commands: Commands,
    scale: Res<RenderScaleOverride>,
    settings: Extract<Option<Res<RuntimeSettings>>>,
    player: Extract<Option<Res<PlayerRuntime>>>,
    owners: Extract<Query<RenderEntity, With<FlyCamera>>>,
    previous: Query<(Entity, &RenderScaleRequest)>,
    filter: Option<Res<render::RenderScaleSharpen>>,
) {
    let sharpen = settings
        .as_ref()
        .map_or(ui::DEFAULT_RENDER_SCALE_SHARPEN, |settings| {
            settings.user_settings_update().1.video.render_scale_sharpen
        })
        .then_some(ui::FSR_SHARPNESS_STOPS);
    if filter.as_ref().map(|filter| filter.0) != Some(sharpen) {
        commands.insert_resource(render::RenderScaleSharpen(sharpen));
    }
    let scale = scale.0.unwrap_or_else(|| {
        settings.as_ref().map_or(1.0, |settings| {
            f32::from(settings.user_settings_update().1.video.render_scale_percent) / 100.0
        })
    });
    // StartGame identity survives the between-frame world-stream handoff and clears on leave.
    let in_world = player
        .as_ref()
        .is_some_and(|player| player.inventory.local_runtime_id().is_some());
    let scale = if in_world && (MIN_RENDER_SCALE..=1.0).contains(&scale) {
        scale
    } else {
        1.0
    };
    for (entity, _) in &previous {
        if scale == 1.0 || !owners.iter().any(|owner| owner == entity) {
            commands
                .entity(entity)
                .remove::<(RenderScaleRequest, render::RenderScaleOutput)>();
        }
    }
    for owner in &owners {
        if scale < 1.0
            && !previous
                .get(owner)
                .is_ok_and(|(_, request)| request.0 == scale)
        {
            commands.entity(owner).insert(RenderScaleRequest(scale));
        }
    }
}

/// Changes only the extracted view before Bevy allocates its color and depth attachments.
fn scale_extracted_camera(
    mut commands: Commands,
    owners: Query<(Entity, &RenderScaleRequest)>,
    mut views: Query<(
        Entity,
        &mut ExtractedCamera,
        &mut ExtractedView,
        Has<MainPassResolutionOverride>,
    )>,
    outputs: Query<&render::RenderScaleOutput>,
) {
    for (owner, scale) in &owners {
        let Ok((_, camera, view, overridden)) = views.get(owner) else {
            continue;
        };
        let target = camera.target.clone();
        let compatible = target.is_some()
            && !overridden
            && camera.viewport.is_none()
            && matches!(
                camera.output_mode,
                CameraOutputMode::Write {
                    blend_state: None,
                    ..
                }
            )
            && view.viewport.xy() == UVec2::ZERO
            && camera.physical_target_size == camera.physical_viewport_size
            && camera.physical_target_size == Some(view.viewport.zw());
        let unique = views
            .iter()
            .filter(|(_, camera, _, _)| camera.target == target)
            .count()
            == 1;
        let (_, mut camera, mut view, _) = views.get_mut(owner).expect("extracted owner exists");
        if scale.0 == 1.0 || !compatible || !unique {
            if outputs.contains(owner) {
                commands.entity(owner).remove::<render::RenderScaleOutput>();
            }
            continue;
        }
        let output = camera.physical_target_size.expect("compatible target size");
        if output.min_element() == 0 {
            continue;
        }
        let size = scaled_size(output, scale.0);
        camera.physical_target_size = Some(size);
        camera.physical_viewport_size = Some(size);
        view.viewport = UVec4::new(0, 0, size.x, size.y);
        if !outputs.get(owner).is_ok_and(|current| current.0 == output) {
            commands
                .entity(owner)
                .insert(render::RenderScaleOutput(output));
            info!(target: "render_scale", output_width = output.x, output_height = output.y,
                render_width = size.x, render_height = size.y, "using reduced render resolution");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        camera::{ClearColorConfig, MsaaWriteback, NormalizedRenderTarget},
        ecs::schedule::ScheduleLabel,
        render::{
            MainWorld,
            render_resource::TextureFormat,
            view::{ColorGrading, RetainedViewEntity},
        },
    };

    /// Builds a native camera/view pair without changing its projection.
    fn camera(size: UVec2) -> (ExtractedCamera, ExtractedView) {
        (
            ExtractedCamera {
                target: Some(NormalizedRenderTarget::None {
                    width: size.x,
                    height: size.y,
                }),
                physical_viewport_size: Some(size),
                physical_target_size: Some(size),
                viewport: None,
                schedule: bevy::core_pipeline::Core3d.intern(),
                order: 0,
                output_mode: CameraOutputMode::default(),
                msaa_writeback: MsaaWriteback::default(),
                clear_color: ClearColorConfig::Default,
                sorted_camera_index_for_target: 0,
                exposure: 1.0,
                hdr: false,
            },
            ExtractedView {
                retained_view_entity: RetainedViewEntity::new(Entity::PLACEHOLDER.into(), None, 0),
                clip_from_view: Mat4::IDENTITY,
                world_from_view: GlobalTransform::default(),
                clip_from_world: None,
                target_format: TextureFormat::Rgba8UnormSrgb,
                viewport: UVec4::new(0, 0, size.x, size.y),
                color_grading: ColorGrading::default(),
                invert_culling: false,
            },
        )
    }

    #[test]
    fn extraction_scales_only_internal_sizes_and_restores_native_after_resize() {
        let mut world = World::new();
        let output = UVec2::new(1280, 720);
        let entity = world.spawn((camera(output), RenderScaleRequest(0.5))).id();
        let mut system = IntoSystem::into_system(scale_extracted_camera);
        system.initialize(&mut world);
        system.run((), &mut world).unwrap();
        let scaled = world.get::<ExtractedCamera>(entity).unwrap();
        assert_eq!(scaled.physical_target_size, Some(UVec2::new(640, 360)));
        assert_eq!(
            scaled.target,
            camera(output).0.target,
            "output target stays native"
        );
        let view = world.get::<ExtractedView>(entity).unwrap();
        assert_eq!(view.viewport, UVec4::new(0, 0, 640, 360));
        assert_eq!(
            view.clip_from_view,
            Mat4::IDENTITY,
            "projection is unchanged"
        );
        assert_eq!(
            world.get::<render::RenderScaleOutput>(entity).unwrap().0,
            output
        );

        let resized = UVec2::new(960, 540);
        world.entity_mut(entity).insert(camera(resized));
        system.run((), &mut world).unwrap();
        assert_eq!(
            world
                .get::<ExtractedCamera>(entity)
                .unwrap()
                .physical_target_size,
            Some(UVec2::new(480, 270))
        );
        assert_eq!(
            world.get::<render::RenderScaleOutput>(entity).unwrap().0,
            resized
        );

        world.entity_mut(entity).insert(camera(resized));
        world.get_mut::<RenderScaleRequest>(entity).unwrap().0 = 1.0;
        system.run((), &mut world).unwrap();
        assert_eq!(
            world
                .get::<ExtractedCamera>(entity)
                .unwrap()
                .physical_target_size,
            Some(resized)
        );
        assert_eq!(
            world.get::<ExtractedView>(entity).unwrap().viewport.zw(),
            resized
        );
        assert!(world.get::<render::RenderScaleOutput>(entity).is_none());
    }

    #[test]
    fn extraction_preserves_partial_and_shared_outputs_and_scales_hdr() {
        for mode in 0..3 {
            let mut world = World::new();
            let output = UVec2::new(1280, 720);
            let mut components = camera(output);
            match mode {
                0 => components.0.hdr = true,
                1 => {
                    components.0.viewport = Some(bevy::camera::Viewport {
                        physical_size: output / 2,
                        ..default()
                    })
                }
                _ => {
                    world.spawn(camera(output));
                }
            }
            let entity = world.spawn((components, RenderScaleRequest(0.5))).id();
            let mut system = IntoSystem::into_system(scale_extracted_camera);
            system.initialize(&mut world);
            system.run((), &mut world).unwrap();
            assert_eq!(
                world
                    .get::<ExtractedCamera>(entity)
                    .unwrap()
                    .physical_target_size,
                Some(if mode == 0 { output / 2 } else { output })
            );
            assert_eq!(
                world.get::<render::RenderScaleOutput>(entity).is_none(),
                mode != 0
            );
        }
    }

    #[test]
    fn native_extraction_stays_empty_and_losing_the_owner_clears_scaled_policy() {
        let mut world = World::new();
        let entity = world.spawn_empty().id();
        let mut main = MainWorld::default();
        let owner = main
            .spawn((FlyCamera::default(), RenderEntity::from(entity)))
            .id();
        let mut player = PlayerRuntime(player_state::PlayerState::new(1));
        player.inventory.publish_local_runtime_id(1, 42).unwrap();
        main.insert_resource(player);
        world.insert_resource(main);
        world.insert_resource(RenderScaleOverride(Some(1.0)));
        let mut system = IntoSystem::into_system(extract_scale_requests);
        system.initialize(&mut world);
        system.run((), &mut world).unwrap();
        assert_eq!(
            world.resource::<render::RenderScaleSharpen>().0,
            Some(ui::FSR_SHARPNESS_STOPS),
        );

        assert!(world.get::<RenderScaleRequest>(entity).is_none());
        assert!(world.get::<render::RenderScaleOutput>(entity).is_none());

        world.resource_mut::<RenderScaleOverride>().0 = Some(0.5);
        system.run((), &mut world).unwrap();
        assert_eq!(world.get::<RenderScaleRequest>(entity).unwrap().0, 0.5);
        world
            .entity_mut(entity)
            .insert(render::RenderScaleOutput(UVec2::new(1280, 720)));
        world
            .resource_mut::<MainWorld>()
            .entity_mut(owner)
            .remove::<FlyCamera>();
        system.run((), &mut world).unwrap();
        assert!(world.get::<RenderScaleRequest>(entity).is_none());
        assert!(world.get::<render::RenderScaleOutput>(entity).is_none());
    }

    #[test]
    fn scaling_requires_start_game_identity_and_clears_on_session_retirement() {
        let mut world = World::new();
        let entity = world.spawn_empty().id();
        let mut main = MainWorld::default();
        main.spawn((FlyCamera::default(), RenderEntity::from(entity)));
        world.insert_resource(main);
        world.insert_resource(RenderScaleOverride(Some(0.5)));
        let mut system = IntoSystem::into_system(extract_scale_requests);
        system.initialize(&mut world);
        system.run((), &mut world).unwrap();
        assert!(world.get::<RenderScaleRequest>(entity).is_none());

        world
            .resource_mut::<MainWorld>()
            .insert_resource(PlayerRuntime(player_state::PlayerState::new(1)));
        system.run((), &mut world).unwrap();
        assert!(world.get::<RenderScaleRequest>(entity).is_none());

        world
            .resource_mut::<MainWorld>()
            .resource_mut::<PlayerRuntime>()
            .inventory
            .publish_local_runtime_id(1, 42)
            .unwrap();
        // Extraction needs no borrowed world stream while its worker owns it.
        for _ in 0..2 {
            system.run((), &mut world).unwrap();
            assert_eq!(world.get::<RenderScaleRequest>(entity).unwrap().0, 0.5);
        }
        world
            .entity_mut(entity)
            .insert(render::RenderScaleOutput(UVec2::new(1280, 720)));
        world
            .resource_mut::<MainWorld>()
            .resource_mut::<PlayerRuntime>()
            .begin_session(2);
        system.run((), &mut world).unwrap();
        assert!(world.get::<RenderScaleRequest>(entity).is_none());
        assert!(world.get::<render::RenderScaleOutput>(entity).is_none());
    }

    #[test]
    fn scale_is_explicit_bounded_and_native_by_default() {
        assert_eq!(parse_scale(Some("0.1")), Some(0.1));
        assert_eq!(parse_scale(Some("0.37")), Some(0.37));
        assert_eq!(parse_scale(Some("0.5")), Some(0.5));
        assert_eq!(parse_scale(Some("0.75")), Some(0.75));
        assert_eq!(parse_scale(Some("1")), Some(1.0));
        for value in [
            None,
            Some("0"),
            Some("0.09"),
            Some("-0.5"),
            Some("2"),
            Some("NaN"),
            Some("inf"),
            Some("invalid"),
        ] {
            assert_eq!(parse_scale(value), None);
        }
    }

    #[test]
    fn resize_uses_fresh_output_dimensions_and_keeps_a_nonempty_frame() {
        assert_eq!(
            scaled_size(UVec2::new(1280, 720), 0.5),
            UVec2::new(640, 360)
        );
        assert_eq!(scaled_size(UVec2::new(961, 541), 0.5), UVec2::new(481, 271));
        assert_eq!(scaled_size(UVec2::ONE, 0.1), UVec2::ONE);
        assert_eq!(scaled_size(UVec2::new(961, 541), 0.1), UVec2::new(96, 54));
    }
}
