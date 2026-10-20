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
        update_overlay(&mut scene, |scene| drawing.publish_blocks(scene));
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    /// Unchanged packet overlays must not extract a steady component scene again.
    #[test]
    fn unchanged_packet_blocks_leave_the_scene_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let empty = br#"(component
          (core module $m (func (export "init")) (func (export "frame")))
          (core instance $i (instantiate $m))
          (func (export "init") (canon lift (core func $i "init")))
          (func (export "frame") (canon lift (core func $i "frame"))))"#;
        let host = mod_host::ModHost::load_snapshot_with_grants(
            &directory.path().join("empty.wat"),
            empty,
            Default::default(),
        )
        .unwrap();
        let mut world = World::new();
        world.insert_resource(super::super::ModRuntime::new(
            host,
            super::super::registration::HostMetadata::environment(0),
        ));
        world.init_resource::<ModRenderScene>();
        world.init_resource::<super::super::packet_render::Drawing>();
        world.run_system_once(tick).unwrap();
        world.clear_trackers();
        world.run_system_once(tick).unwrap();
        assert!(
            !world
                .get_resource_ref::<ModRenderScene>()
                .unwrap()
                .is_changed()
        );
    }

    /// Runs the actual merge/publication path with an inactive packet drawing producer.
    fn tick(
        scene: ResMut<ModRenderScene>,
        mut runtime: ResMut<super::super::ModRuntime>,
        drawing: Res<super::super::packet_render::Drawing>,
    ) {
        publish(Some(scene), &mut runtime, Some(&drawing));
    }
}
