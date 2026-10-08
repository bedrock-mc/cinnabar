use crate::chunk::*;

/// One scene-sized scratch texture per admitted camera, with two views of the
/// same encoded bytes. Resize replaces it; no unbounded historical target map.
#[derive(Component)]
pub(super) struct GammaTarget {
    pub(super) texture: Texture,
    pub(super) gamma_view: TextureView,
    pub(super) srgb_view: TextureView,
}

pub(super) fn admitted(hdr: bool, msaa: Msaa, enhanced: bool) -> bool {
    !hdr && msaa == Msaa::Off && !(crate::ENHANCED_RENDERING_ENABLED && enhanced)
}

/// Cameras and their existing scratch targets, borrowed during target preparation.
type GammaTargetViews<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static ExtractedView,
        &'static ViewTarget,
        &'static Msaa,
        Option<&'static crate::EnhancedRendering>,
        Option<&'static GammaTarget>,
    ),
    With<Camera3d>,
>;

pub(super) fn prepare_gamma_targets(
    mut commands: Commands,
    device: Res<RenderDevice>,
    views: GammaTargetViews<'_, '_>,
) {
    for (entity, view, target, msaa, enhanced, previous) in &views {
        if !admitted(view.hdr, *msaa, enhanced.is_some()) {
            if previous.is_some() {
                commands.entity(entity).remove::<GammaTarget>();
            }
            continue;
        }
        let size = target.main_texture().size();
        if previous.is_some_and(|scratch| scratch.texture.size() == size) {
            continue;
        }
        let srgb = TextureFormat::bevy_default();
        let gamma = srgb.remove_srgb_suffix();
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("ordinary gamma transparent scene"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: gamma,
            usage: TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::COPY_SRC
                | TextureUsages::COPY_DST,
            view_formats: &[srgb],
        });
        let gamma_view = texture.create_view(&TextureViewDescriptor::default());
        let srgb_view = texture.create_view(&TextureViewDescriptor {
            format: Some(srgb),
            ..default()
        });
        commands.entity(entity).insert(GammaTarget {
            texture,
            gamma_view,
            srgb_view,
        });
    }
}
