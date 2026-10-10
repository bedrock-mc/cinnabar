//! Shared sample-count selection for camera attachments and the Video setting.

use super::{CameraSettingsAuthority, FlyCamera};
use bevy::{
    anti_alias::smaa::Smaa,
    core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
    prelude::*,
    render::{
        RenderApp,
        render_resource::{
            TextureFormat, TextureFormatFeatureFlags, TextureFormatFeatures, TextureUsages,
            WgpuFeatures,
        },
        renderer::{RenderAdapter, RenderDevice},
        view::ViewTarget,
    },
};

/// Sample counts supported by every color and depth attachment in either graphics mode.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct CameraAntiAliasingSupport(pub ui::AntiAliasingSupport);

impl CameraAntiAliasingSupport {
    /// Selects a supported camera sample count without applying a post-process filter.
    pub fn msaa(self, requested: u32) -> Msaa {
        match self.0.select(requested) {
            2 => Msaa::Sample2,
            4 => Msaa::Sample4,
            8 => Msaa::Sample8,
            _ => Msaa::Off,
        }
    }
}

/// Publishes the active device's attachment intersection after renderer initialization.
pub(super) fn install_device_support(app: &mut App) {
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    let (Some(adapter), Some(device)) = (
        render_app.world().get_resource::<RenderAdapter>(),
        render_app.world().get_resource::<RenderDevice>(),
    ) else {
        return;
    };
    let support = device_support(adapter, device);
    app.insert_resource(support);
}

/// Honors both adapter capabilities and the features actually enabled on its device.
pub fn device_support(adapter: &RenderAdapter, device: &RenderDevice) -> CameraAntiAliasingSupport {
    let features = device.features();
    let format_features = |format: TextureFormat| {
        if features.contains(WgpuFeatures::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES) {
            adapter.get_texture_format_features(format)
        } else {
            format.guaranteed_format_features(features)
        }
    };
    let colors = [
        TextureFormat::bevy_default(),
        TextureFormat::bevy_default().remove_srgb_suffix(),
        ViewTarget::TEXTURE_FORMAT_HDR,
    ]
    .map(format_features);
    let depth = format_features(CORE_3D_DEPTH_FORMAT);
    let stencil = format_features(TextureFormat::Stencil8);
    CameraAntiAliasingSupport(attachment_support(&colors, depth, stencil))
}

/// Accepts only counts that can resolve color and expose multisampled depth to the shaders.
fn attachment_support(
    colors: &[TextureFormatFeatures],
    depth: TextureFormatFeatures,
    stencil: TextureFormatFeatures,
) -> ui::AntiAliasingSupport {
    ui::AntiAliasingSupport::from_counts(ui::ANTI_ALIASING_SAMPLE_COUNTS.into_iter().filter(
        |samples| {
            let color_ok = colors.iter().all(|color| {
                color
                    .allowed_usages
                    .contains(TextureUsages::RENDER_ATTACHMENT)
                    && color.flags.sample_count_supported(*samples)
                    && (*samples == 1
                        || color
                            .flags
                            .contains(TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE))
            });
            color_ok
                && depth
                    .allowed_usages
                    .contains(TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING)
                && depth.flags.sample_count_supported(*samples)
                && stencil
                    .allowed_usages
                    .contains(TextureUsages::RENDER_ATTACHMENT)
                && stencil.flags.sample_count_supported(*samples)
        },
    ))
}

/// Selects world-camera coverage before viewmodel publication, including Enhanced depth limits.
pub fn apply_camera_antialiasing(
    mut commands: Commands,
    settings: Res<CameraSettingsAuthority>,
    support: Res<CameraAntiAliasingSupport>,
    mut cameras: Query<
        (Entity, &mut Msaa, Option<&Smaa>, Has<render::EnhancedRendering>),
        With<FlyCamera>,
    >,
) {
    for (entity, mut msaa, smaa, enhanced) in &mut cameras {
        let desired = if enhanced {
            Msaa::Off
        } else {
            support.msaa(settings.anti_aliasing_samples())
        };
        if *msaa != desired {
            *msaa = desired;
        }
        match (settings.smaa_mode(), smaa.is_some()) {
            (ui::SmaaMode::Smaa, false) => {
                commands.entity(entity).insert(Smaa::default());
            }
            (ui::SmaaMode::Off, true) => {
                commands.entity(entity).remove::<Smaa>();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Describes an attachment with the supplied multisample capabilities.
    fn features(flags: TextureFormatFeatureFlags) -> TextureFormatFeatures {
        TextureFormatFeatures {
            allowed_usages: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            flags,
        }
    }

    #[test]
    fn antialiasing_intersects_color_depth_and_resolve_capabilities() {
        let color = features(
            TextureFormatFeatureFlags::MULTISAMPLE_X2
                | TextureFormatFeatureFlags::MULTISAMPLE_X4
                | TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE,
        );
        let depth = features(
            TextureFormatFeatureFlags::MULTISAMPLE_X4 | TextureFormatFeatureFlags::MULTISAMPLE_X8,
        );
        assert_eq!(
            attachment_support(&[color], depth, depth)
                .counts()
                .collect::<Vec<_>>(),
            [1, 4]
        );
        assert_eq!(
            attachment_support(
                &[features(TextureFormatFeatureFlags::MULTISAMPLE_X4)],
                depth,
                depth
            )
            .counts()
            .collect::<Vec<_>>(),
            [1]
        );
    }

    #[test]
    fn antialiasing_excludes_counts_without_shadow_stencil_coverage() {
        let all = features(
            TextureFormatFeatureFlags::MULTISAMPLE_X2
                | TextureFormatFeatureFlags::MULTISAMPLE_X4
                | TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE,
        );
        let stencil = features(TextureFormatFeatureFlags::MULTISAMPLE_X4);
        assert_eq!(
            attachment_support(&[all], all, stencil)
                .counts()
                .collect::<Vec<_>>(),
            [1, 4]
        );
    }

    #[test]
    fn antialiasing_edits_update_the_camera_once_and_fall_back_on_unsupported_devices() {
        let mut app = App::new();
        app.init_resource::<CameraSettingsAuthority>()
            .insert_resource(CameraAntiAliasingSupport(
                ui::AntiAliasingSupport::from_counts([1, 4]),
            ))
            .add_systems(Update, apply_camera_antialiasing);
        let entity = app
            .world_mut()
            .spawn((FlyCamera::default(), Msaa::Sample8))
            .id();
        app.update();
        assert_eq!(*app.world().get::<Msaa>(entity).unwrap(), Msaa::Off);
        let mut settings = ui::UserSettings::default();
        settings.video.anti_aliasing_samples = 8;
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .replace(1, &settings)
            .unwrap();
        app.update();
        assert_eq!(*app.world().get::<Msaa>(entity).unwrap(), Msaa::Sample4);
        app.world_mut().clear_trackers();
        app.update();
        assert!(
            !app.world()
                .entity(entity)
                .get_ref::<Msaa>()
                .unwrap()
                .is_changed()
        );
    }

    #[test]
    fn enhanced_cameras_publish_single_sample_coverage_and_restore_vanilla_msaa() {
        let mut app = App::new();
        app.init_resource::<CameraSettingsAuthority>()
            .insert_resource(CameraAntiAliasingSupport(
                ui::AntiAliasingSupport::from_counts([1, 4]),
            ))
            .add_systems(Update, apply_camera_antialiasing);
        let mut settings = ui::UserSettings::default();
        settings.video.anti_aliasing_samples = 4;
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .replace(1, &settings)
            .unwrap();
        let enhanced = app
            .world_mut()
            .spawn((
                FlyCamera::default(),
                Msaa::Sample4,
                render::EnhancedRendering::default(),
            ))
            .id();
        let vanilla = app
            .world_mut()
            .spawn((FlyCamera::default(), Msaa::Off))
            .id();
        for _ in 0..2 {
            app.update();
            assert_eq!(app.world().get::<Msaa>(enhanced).unwrap().samples(), 1);
            assert_eq!(app.world().get::<Msaa>(vanilla).unwrap().samples(), 4);
        }
        app.world_mut().clear_trackers();
        app.update();
        assert!(
            !app.world()
                .entity(enhanced)
                .get_ref::<Msaa>()
                .unwrap()
                .is_changed()
        );
        app.world_mut()
            .entity_mut(enhanced)
            .remove::<render::EnhancedRendering>();
        app.update();
        assert_eq!(app.world().get::<Msaa>(enhanced).unwrap().samples(), 4);
    }

    #[test]
    fn spatial_antialiasing_follows_world_settings_without_touching_other_cameras() {
        let mut app = App::new();
        app.init_resource::<CameraSettingsAuthority>()
            .init_resource::<CameraAntiAliasingSupport>()
            .add_systems(Update, apply_camera_antialiasing);
        let world_camera = app
            .world_mut()
            .spawn((FlyCamera::default(), Msaa::Off))
            .id();
        let ui_camera = app.world_mut().spawn(Msaa::Off).id();
        app.update();
        assert!(app.world().get::<Smaa>(world_camera).is_none());
        let mut user = ui::UserSettings::default();
        user.video.smaa_mode = ui::SmaaMode::Smaa;
        for (generation, samples) in [1, 4].into_iter().enumerate() {
            user.video.anti_aliasing_samples = samples;
            app.world_mut()
                .resource_mut::<CameraSettingsAuthority>()
                .replace(generation as u64 + 1, &user)
                .unwrap();
            app.update();
            assert!(
                app.world().get::<Smaa>(world_camera).unwrap().preset
                    == bevy::anti_alias::smaa::SmaaPreset::High
            );
            assert_eq!(
                app.world().get::<Msaa>(world_camera).unwrap().samples(),
                samples
            );
            assert!(app.world().get::<Smaa>(ui_camera).is_none());
        }
        app.world_mut().clear_trackers();
        app.update();
        assert!(
            !app.world()
                .entity(world_camera)
                .get_ref::<Smaa>()
                .unwrap()
                .is_changed()
        );
        user.video.smaa_mode = ui::SmaaMode::Off;
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .replace(3, &user)
            .unwrap();
        app.update();
        assert!(app.world().get::<Smaa>(world_camera).is_none());
        assert_eq!(app.world().get::<Msaa>(world_camera).unwrap().samples(), 4);
    }
}
