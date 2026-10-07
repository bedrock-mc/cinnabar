//! Publishes target highlights from prepared pack textures and the final camera pose.

use std::sync::Arc;

use bevy::prelude::*;
use client_presentation::aim_assist::{AimAssistFrame, TargetKind};
use render::{AIM_ASSIST_TEXTURES, AimAssistHighlight, AimAssistHighlightScene, AimAssistTexture};

use super::FlyCamera;
use crate::{local_player::LocalViewPose, runtime::network::PackReload};

#[derive(Resource, Default)]
pub(crate) struct BaseTextures([Option<Arc<AimAssistTexture>>; 2]);

/// Optional highlight art is loaded once from the installed pack and never synthesized.
pub(crate) fn load_base_textures(mut commands: Commands) {
    let root = launcher::install_layout::InstallLayout::discover()
        .ok()
        .map(|layout| {
            layout
                .resource_root
                .join(launcher::install_layout::vanilla_pack_relative())
        });
    let textures = AIM_ASSIST_TEXTURES.map(|name| {
        let image = image::open(root.as_ref()?.join(name).with_extension("png"))
            .ok()?
            .to_rgba8();
        AimAssistTexture::new([image.width(), image.height()], image.into_raw().into())
            .map(Arc::new)
    });
    if textures.iter().any(Option::is_none) {
        warn!("aim-assist highlight textures unavailable in the installed vanilla pack");
    }
    commands.insert_resource(BaseTextures(textures));
}

/// Pack decoding happens on the preparation worker and records exact texture dependencies.
pub(crate) fn prepare_pack_textures(
    view: &resource_pack::LayeredPackView,
) -> [Option<Arc<AimAssistTexture>>; 2] {
    AIM_ASSIST_TEXTURES.map(|name| {
        let texture = client_session::pack_textures::decode_pack_texture(view, name)?;
        AimAssistTexture::new([texture.width, texture.height], texture.rgba8.into()).map(Arc::new)
    })
}

/// Publication only copies fixed poses and retained texture handles, regardless of debug flags.
pub(crate) fn publish(
    frame: Res<AimAssistFrame>,
    view: Res<LocalViewPose>,
    cameras: Query<&Transform, With<FlyCamera>>,
    scene: Option<ResMut<AimAssistHighlightScene>>,
    base: Option<Res<BaseTextures>>,
    packs: Option<Res<PackReload>>,
) {
    let Some(mut scene) = scene else { return };
    let overrides = packs.as_deref().map(PackReload::aim_assist_textures);
    for index in 0..AIM_ASSIST_TEXTURES.len() {
        let texture = overrides
            .and_then(|textures| textures[index].as_ref())
            .or_else(|| {
                base.as_ref()
                    .and_then(|textures| textures.0[index].as_ref())
            });
        let same = match (&scene.textures[index], texture) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            scene.textures[index] = texture.cloned();
        }
    }
    let target = frame.target.and_then(|target| match target.kind {
        TargetKind::Block { face, .. } => {
            AimAssistHighlight::block(target.point, face, view.rotation() * Vec3::NEG_Z)
        }
        TargetKind::Actor(_) => {
            let camera = cameras.single().ok()?;
            AimAssistHighlight::actor(
                target.point,
                camera.rotation * Vec3::NEG_Z,
                camera.rotation * Vec3::Y,
            )
        }
    });
    if scene.target != target {
        scene.target = target;
    }
}
