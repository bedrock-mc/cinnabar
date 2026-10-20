//! Hands committed mod render output to the renderer; depth sampling follows its grant.

use bevy::{prelude::*, render::render_resource::TextureUsages};
use client_presentation::camera::FlyCamera;
use render_scene::ModRenderScene;

/// Republishes only when some mod's generation changed, so steady frames extract nothing.
pub(super) fn publish(
    scene: Option<ResMut<ModRenderScene>>,
    runtime: &mut super::ModRuntime,
    drawing: Option<&super::packet_render::Drawing>,
) {
    let Some(mut scene) = scene else { return };
    if let Some(drawing) = drawing {
        drawing.publish_blocks(&mut scene);
    }
    let unchanged = runtime.render_sources.iter().copied().eq(runtime
        .hosts
        .render_outputs()
        .map(|(_, generation)| generation)
        .chain(drawing.map(|drawing| drawing.output().1)));
    if unchanged {
        return;
    }
    let mut cache = std::mem::take(&mut runtime.render_merge);
    let merged = cache.merge(
        runtime
            .hosts
            .render_outputs()
            .map(|(output, _)| output)
            .chain(drawing.map(|drawing| drawing.output().0)),
    );
    runtime.render_merge = cache;
    let generation = scene.generation().wrapping_add(1);
    scene.apply(&merged, generation);
    runtime.render_sources = runtime
        .hosts
        .render_outputs()
        .map(|(_, generation)| generation)
        .chain(drawing.map(|drawing| drawing.output().1))
        .collect();
}

/// Applies a host-owned overlay setter with change detection bypassed, marking the scene
/// changed only when the setter replaced an overlay vertex buffer. The scene's overlay setters
/// replace their buffer exactly when their inputs change, and the renderer reads overlays only
/// through those buffers, so steady frames extract nothing.
pub(super) fn update_overlay(
    scene: &mut ResMut<ModRenderScene>,
    update: impl FnOnce(&mut ModRenderScene),
) {
    let overlays = |scene: &ModRenderScene| {
        (
            std::sync::Arc::as_ptr(&scene.marker_vertices).cast::<()>(),
            std::sync::Arc::as_ptr(&scene.block_vertices).cast::<()>(),
        )
    };
    let before = overlays(scene);
    update(scene.bypass_change_detection());
    if overlays(scene) != before {
        scene.set_changed();
    }
}

/// Scene depth becomes sampleable once a mod holds both render grants.
pub(super) fn grant_depth_sampling(
    runtime: Option<Res<super::ModRuntime>>,
    mut cameras: Query<&mut Camera3d, With<FlyCamera>>,
) {
    if !runtime.is_some_and(|runtime| {
        (0..runtime.hosts.host_count()).any(|index| {
            let grants = runtime.hosts.host(index).grants();
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
