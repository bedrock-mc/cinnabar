//! Loading backgrounds preserve opaque startup evidence while covering terrain.

use crate::chunk::gpu_cull::app_tests::{
    KEYS, camera_transform, chunk_app, frame, insert_meshes, noop_render_plugin,
};
use crate::chunk::*;
use bevy::{camera::RenderTarget, prelude::On};

/// Loading keeps opaque submissions alive until startup receives a fresh GPU completion.
#[test]
fn loading_panorama_keeps_startup_visibility_completions_advancing() {
    let (mut app, _) = chunk_app(
        noop_render_plugin(WgpuFeatures::empty()),
        Msaa::Sample4,
        camera_transform(),
    );
    let mut panorama = render_scene::PanoramaScene::default();
    panorama.set_game_visible(false);
    panorama.show(Some(render_model::panorama::launcher_view(
        0.0, 1.0, [0.0; 4],
    )));
    app.sub_app_mut(RenderApp).insert_resource(panorama);
    app.world_mut()
        .resource_mut::<render_scene::VisibilityDiagnosticsInput>()
        .set_startup_probe_enabled(true);
    insert_meshes(&mut app, &KEYS);
    for _ in 0..16 {
        app.world_mut()
            .resource_mut::<render_scene::VisibilityDiagnosticsInput>()
            .advance(KEYS, KEYS);
        frame(&mut app);
    }
    let diagnostics = app
        .world()
        .resource::<render_scene::VisibilityDiagnostics>();
    let ready = diagnostics.snapshot();
    assert!(
        ready
            .gpu_completed_opaque
            .is_some_and(|digest| digest.count > 0),
        "loading must receive GPU-completed terrain submissions: {ready:?}"
    );
    for _ in 0..4 {
        app.world_mut()
            .resource_mut::<render_scene::VisibilityDiagnosticsInput>()
            .advance(KEYS, KEYS);
        frame(&mut app);
    }
    let fresh = app
        .world()
        .resource::<render_scene::VisibilityDiagnostics>()
        .snapshot();
    assert!(
        fresh.frame_generation > ready.frame_generation,
        "terrain readiness must be followed by a fresh completion"
    );
    assert!(
        fresh
            .gpu_completed_opaque
            .is_some_and(|digest| digest.count > 0)
    );
    assert!(
        !app.sub_app(RenderApp)
            .world()
            .resource::<render_scene::PanoramaScene>()
            .game_visible(),
        "startup probes must not uncover the unfinished world"
    );
}

#[derive(Resource, Default)]
struct PanoramaPixels(Vec<u8>);

/// The real panorama covers submitted terrain and gives way to it when loading ends.
#[test]
fn startup_terrain_stays_behind_the_panorama_on_a_native_gpu() {
    use crate::chunk::gpu_cull::app_tests::{chunk_app_with, render_plugin};
    use bevy::render::gpu_readback::{Readback, ReadbackComplete};
    for msaa in [Msaa::Off, Msaa::Sample4] {
        let Some(render) = render_plugin(wgpu::Backends::PRIMARY, WgpuFeatures::empty()) else {
            eprintln!("skipping startup terrain panorama: missing native GPU adapter fixture");
            return;
        };
        let (mut app, camera) = chunk_app_with(render, msaa, camera_transform(), |app| {
            app.add_plugins(crate::PanoramaRenderPlugin);
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            for (_, image) in images.iter_mut() {
                image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
            }
        });
        app.world_mut()
            .entity_mut(camera)
            .insert(bevy::core_pipeline::tonemapping::Tonemapping::Linear);
        let RenderTarget::Image(image) = app.world().get::<RenderTarget>(camera).unwrap().clone()
        else {
            panic!("fixture must use an image target");
        };
        app.init_resource::<PanoramaPixels>();
        app.world_mut()
            .spawn(Readback::texture(image.handle))
            .observe(
                |event: On<ReadbackComplete>, mut pixels: ResMut<PanoramaPixels>| {
                    pixels.0.clone_from(&event.data);
                },
            );
        let mut scene = app
            .world_mut()
            .resource_mut::<render_scene::PanoramaScene>();
        scene.set_faces(Some(std::sync::Arc::new(
            render_model::PanoramaFaces::new(1, std::array::from_fn(|_| vec![0, 0, 255, 255]))
                .unwrap(),
        )));
        scene.set_game_visible(false);
        scene.show(Some(render_model::panorama::launcher_view(
            0.0, 1.0, [0.0; 4],
        )));
        app.world_mut()
            .resource_mut::<render_scene::VisibilityDiagnosticsInput>()
            .set_startup_probe_enabled(true);
        insert_meshes(&mut app, &KEYS);
        for _ in 0..16 {
            app.world_mut()
                .resource_mut::<render_scene::VisibilityDiagnosticsInput>()
                .advance(KEYS, KEYS);
            frame(&mut app);
        }
        let snapshot = app
            .world()
            .resource::<render_scene::VisibilityDiagnostics>()
            .snapshot();
        assert!(
            snapshot
                .gpu_completed_opaque
                .is_some_and(|digest| digest.count > 0),
            "{snapshot:?}"
        );
        let pixels = &app.world().resource::<PanoramaPixels>().0;
        assert_eq!(pixels.len(), 64 * 48 * 4);
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [0, 0, 255, 255]),
            "terrain leaked through the panorama with {msaa:?}"
        );
        let mut scene = app
            .world_mut()
            .resource_mut::<render_scene::PanoramaScene>();
        scene.set_faces(None);
        scene.show(None);
        for _ in 0..8 {
            frame(&mut app);
        }
        assert!(
            app.world()
                .resource::<PanoramaPixels>()
                .0
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [0, 0, 0, 255]),
            "missing panorama faces must still cover unfinished terrain"
        );
        app.world_mut()
            .resource_mut::<render_scene::PanoramaScene>()
            .set_game_visible(true);
        for _ in 0..8 {
            frame(&mut app);
        }
        assert!(
            app.world()
                .resource::<PanoramaPixels>()
                .0
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| *pixel != [0, 0, 0, 255]),
            "the world must replace the loading backdrop"
        );
    }
}
