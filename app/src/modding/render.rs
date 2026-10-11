//! Hands committed mod render output to the renderer; depth sampling follows its grant.

use bevy::{prelude::*, render::render_resource::TextureUsages};
use client_presentation::camera::FlyCamera;
use render::ModRenderScene;

/// Republishes only when some mod's generation changed, so steady frames extract nothing.
pub(super) fn publish(scene: Option<ResMut<ModRenderScene>>, runtime: &mut super::ModRuntime) {
    let Some(mut scene) = scene else { return };
    if runtime.host_count() == 1 {
        let (output, generation) = runtime.host.render();
        if scene.generation() != generation {
            scene.apply(output, generation);
        }
        return;
    }
    let unchanged = runtime
        .render_sources
        .iter()
        .copied()
        .eq(runtime.render_outputs().map(|(_, generation)| generation));
    if unchanged {
        return;
    }
    let mut cache = std::mem::take(&mut runtime.render_merge);
    let merged = cache.merge(runtime.render_outputs().map(|(output, _)| output));
    runtime.render_merge = cache;
    let generation = scene.generation().wrapping_add(1);
    scene.apply(&merged, generation);
    runtime.render_sources = runtime
        .render_outputs()
        .map(|(_, generation)| generation)
        .collect();
}

/// Scene depth becomes sampleable once a mod holds both render grants.
pub(super) fn grant_depth_sampling(
    runtime: Option<Res<super::ModRuntime>>,
    mut cameras: Query<&mut Camera3d, With<FlyCamera>>,
) {
    if !runtime.is_some_and(|runtime| {
        (0..runtime.host_count()).any(|index| {
            let grants = runtime.host(index).grants();
            grants.render && grants.render_depth
        })
    }) {
        return;
    }
    for mut camera in &mut cameras {
        let usage = TextureUsages::from(camera.depth_texture_usages);
        if !usage.contains(TextureUsages::TEXTURE_BINDING) {
            camera.depth_texture_usages = (usage | TextureUsages::TEXTURE_BINDING).into();
        }
    }
}
