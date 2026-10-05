//! Publishes camera overlays, using admitted block art for fire and optional mask images.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use bevy::{
    log::warn,
    prelude::{Local, Mat4, Projection, Query, Res, ResMut, Time, Transform, With},
};
use render::{
    ChunkTextureAssetIdentity, ChunkTextureAssets, SCREEN_OVERLAY_TEXTURE_SIDE, ScreenFireTexture,
    ScreenOverlayKind, ScreenOverlayLayer, ScreenOverlayScene, ScreenOverlayTextures,
};

use super::{
    FlyCamera,
    overlay::{OverlayKind, OverlayLayer, ScreenOverlays},
};
use launcher::install_layout::InstallLayout;

fn overlay_kind(kind: OverlayKind) -> ScreenOverlayKind {
    match kind {
        OverlayKind::Suffocation | OverlayKind::ServerFade => ScreenOverlayKind::Flat,
        OverlayKind::PowderSnow => ScreenOverlayKind::PowderSnow,
        OverlayKind::Fire => ScreenOverlayKind::Fire,
        OverlayKind::Portal => ScreenOverlayKind::Portal,
        OverlayKind::PumpkinBlur => ScreenOverlayKind::PumpkinBlur,
        OverlayKind::SpyglassScope => ScreenOverlayKind::SpyglassScope,
    }
}

pub fn render_layer(layer: &OverlayLayer) -> ScreenOverlayLayer {
    ScreenOverlayLayer {
        kind: overlay_kind(layer.kind),
        rgb: layer.rgb,
        alpha: layer.alpha,
    }
}

/// Candidate files for one overlay texture, preferred first: a shipped carrier, then the local vanilla pack.
fn candidates(resource_root: &Path, name: &str, vanilla_dir: &str) -> [PathBuf; 2] {
    [
        resource_root.join("overlays").join(name),
        resource_root
            .join(launcher::install_layout::vanilla_pack_relative())
            .join("textures")
            .join(vanilla_dir)
            .join(name),
    ]
}

fn load_rgba(paths: &[PathBuf]) -> Option<Vec<u8>> {
    let image = paths
        .iter()
        .find_map(|path| image::open(path).ok())?
        .to_rgba8();
    let side = SCREEN_OVERLAY_TEXTURE_SIDE;
    let image = if image.dimensions() == (side, side) {
        image
    } else {
        image::imageops::resize(&image, side, side, image::imageops::FilterType::Nearest)
    };
    Some(image.into_raw())
}

/// Loads both overlay textures, or none when either is missing.
pub fn load_textures(resource_root: &Path) -> Option<ScreenOverlayTextures> {
    let pumpkin = load_rgba(&candidates(resource_root, "pumpkinblur.png", "misc"))?;
    let spyglass = load_rgba(&candidates(resource_root, "spyglass_scope.png", "ui"))?;
    ScreenOverlayTextures::new(pumpkin, spyglass)
}

pub fn load_overlay_textures(scene: Option<ResMut<ScreenOverlayScene>>) {
    let Some(mut scene) = scene else {
        return;
    };
    let textures = InstallLayout::discover()
        .ok()
        .and_then(|layout| load_textures(&layout.resource_root));
    if textures.is_none() {
        warn!(
            "camera overlay textures not found; pumpkin and spyglass overlays use procedural art"
        );
    }
    scene.set_textures(textures.map(Arc::new));
}

fn pinned_fire_state() -> Option<u32> {
    assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .ok()?
    .iter()
    .find(|state| state.name.as_ref() == "minecraft:fire")
    .map(|state| state.sequential_id)
}

pub fn publish_screen_overlays(
    time: Res<Time>,
    overlays: Res<ScreenOverlays>,
    cameras: Query<(&Projection, &Transform), With<FlyCamera>>,
    scene: Option<ResMut<ScreenOverlayScene>>,
    textures: Option<Res<ChunkTextureAssets>>,
    mut fire_source: Local<Option<(ChunkTextureAssetIdentity, u32)>>,
) {
    let Some(mut scene) = scene else {
        return;
    };
    if let Some(textures) = textures {
        let identity = textures.identity();
        if fire_source
            .as_ref()
            .is_none_or(|source| source.0 != identity)
        {
            let state = fire_source
                .as_ref()
                .map(|source| source.1)
                .or_else(pinned_fire_state);
            if let Some(state) = state {
                scene.set_fire_texture(
                    ScreenFireTexture::from_assets(textures.assets(), state).map(Arc::new),
                );
                *fire_source = Some((identity, state));
            }
        }
    }
    if let Some((Projection::Perspective(projection), _)) = cameras.iter().next() {
        scene.set_fire_projection(projection.fov, projection.aspect_ratio);
    }
    scene.set_layers(
        overlays.layers.iter().map(render_layer),
        time.elapsed_secs(),
    );
    if let Ok((projection, transform)) = cameras.single() {
        scene.set_portal_from_clip(
            Mat4::from_quat(transform.rotation) * projection.get_clip_from_view().inverse(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_fire_binding_uses_the_active_pinned_registry_protocol() {
        assert!(pinned_fire_state().is_some());
    }

    #[test]
    fn every_overlay_kind_maps_to_a_shader_pattern() {
        let layer = OverlayLayer {
            kind: OverlayKind::SpyglassScope,
            alpha: 0.5,
            rgb: [1.0; 3],
            texture: None,
        };
        let mapped = render_layer(&layer);
        assert_eq!(mapped.kind, ScreenOverlayKind::SpyglassScope);
        assert_eq!(mapped.alpha, 0.5);
        assert_eq!(
            overlay_kind(OverlayKind::ServerFade),
            ScreenOverlayKind::Flat
        );
    }

    #[test]
    fn missing_texture_files_degrade_to_none() {
        assert!(load_textures(Path::new("/nonexistent/cinnabar-overlay-root")).is_none());
    }

    #[test]
    fn candidates_prefer_the_shipped_carrier() {
        let paths = candidates(Path::new("root"), "pumpkinblur.png", "misc");
        assert!(paths[0].ends_with("overlays/pumpkinblur.png"));
        assert!(paths[1].ends_with("textures/misc/pumpkinblur.png"));
    }
}
