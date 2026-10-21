//! Native offscreen frames exercise depth edges, stock SMAA lookup tables and scene samples.

use super::super::*;
use bevy::render::{
    RenderPlugin,
    renderer::{RenderAdapter, RenderContext, RenderQueue},
};
use std::time::{Duration, Instant};

#[path = "blend_tests.rs"]
mod blend_tests;
#[path = "composed_tests.rs"]
mod composed_tests;

use crate::render_test_support::scene_raster::*;

/// Initializes the scene fixture with the production depth SMAA plugin.
fn app(render: RenderPlugin) -> (App, Entity, Handle<Image>) {
    app_with_effects(render, |_| {})
}

/// Adds SMAA and fixture effects before finishing the shared offscreen renderer.
pub(crate) fn app_with_effects(
    render: RenderPlugin,
    setup: impl FnOnce(&mut App),
) -> (App, Entity, Handle<Image>) {
    crate::render_test_support::scene_raster::app_with_effects(render, |app| {
        app.add_plugins(DepthSmaaPlugin);
        setup(app);
    })
}

/// Waits for real SMAA lookup images, bindings and all production pipelines to become usable.
pub(crate) fn ready(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        frame(app);
        let world = app.sub_app_mut(RenderApp).world_mut();
        let prepared = world
            .query::<(&DepthSmaaView, &bevy::anti_alias::smaa::SmaaBindGroups)>()
            .iter(world)
            .next()
            .map(|(view, _)| view.ids);
        if let Some(ids) = prepared {
            let cache = world.resource::<PipelineCache>();
            if [ids.edge, ids.weights, ids.blend]
                .into_iter()
                .all(|id| cache.get_render_pipeline(id).is_some())
            {
                frame(app);
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "native SMAA pipelines and lookup tables did not become ready"
        );
        std::thread::yield_now();
    }
}

/// Reads the final output image after all scene and SMAA graph nodes have completed.
fn pixels(app: &mut App, image: &Handle<Image>) -> Vec<u8> {
    frame(app);
    read_pixels(app, image)
}

#[test]
fn spatial_smaa_preserves_texels_and_filters_depth_silhouettes() {
    let Some(render) = renderer() else {
        return;
    };
    let (mut app, camera, image) = app(render);
    for msaa in [Msaa::Off, Msaa::Sample4] {
        let world = app.sub_app(RenderApp).world();
        let adapter = world.resource::<RenderAdapter>();
        if [
            render_scene::SCENE_COLOR_FORMAT,
            bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
        ]
        .into_iter()
        .any(|format| {
            !adapter
                .get_texture_format_features(format)
                .flags
                .sample_count_supported(msaa.samples())
        }) {
            eprintln!(
                "skipping native SMAA {}x MSAA: missing attachment sample support",
                msaa.samples()
            );
            continue;
        }
        *app.world_mut().get_mut::<Msaa>(camera).unwrap() = msaa;
        let device = app.sub_app(RenderApp).world().resource::<RenderDevice>();
        let fixture = FixturePipelines {
            checker: fixture_pipeline(device, msaa.samples(), "checker"),
            sloped_checker: fixture_pipeline(device, msaa.samples(), "sloped_checker"),
            silhouette: fixture_pipeline(device, msaa.samples(), "silhouette"),
            scene: 0,
        };
        app.sub_app_mut(RenderApp)
            .world_mut()
            .insert_resource(fixture);
        app.world_mut().entity_mut(camera).insert(Smaa::default());
        ready(&mut app);
        app.world_mut().entity_mut(camera).remove::<Smaa>();
        frame(&mut app);
        let checker_off = pixels(&mut app, &image);
        assert!(
            checker_off
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[0] > 240 && pixel[1] < 10)
        );
        assert!(
            checker_off
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[1] > 240 && pixel[0] < 10)
        );
        app.world_mut().entity_mut(camera).insert(Smaa::default());
        ready(&mut app);
        let checker_on = pixels(&mut app, &image);
        capture("checker-off", msaa.samples(), &checker_off);
        capture("checker-smaa", msaa.samples(), &checker_on);
        assert_eq!(
            checker_off,
            checker_on,
            "flat-depth authored texel boundaries changed at {}x MSAA",
            msaa.samples()
        );
        app.sub_app_mut(RenderApp)
            .world_mut()
            .resource_mut::<FixturePipelines>()
            .scene = 1;
        app.world_mut().entity_mut(camera).remove::<Smaa>();
        frame(&mut app);
        let sloped_off = pixels(&mut app, &image);
        app.world_mut().entity_mut(camera).insert(Smaa::default());
        ready(&mut app);
        assert_eq!(
            sloped_off,
            pixels(&mut app, &image),
            "continuous sloped depth smeared authored texel boundaries at {}x MSAA",
            msaa.samples()
        );
        app.sub_app_mut(RenderApp)
            .world_mut()
            .resource_mut::<FixturePipelines>()
            .scene = 2;
        app.world_mut().entity_mut(camera).remove::<Smaa>();
        frame(&mut app);
        let silhouette_off = pixels(&mut app, &image);
        app.world_mut().entity_mut(camera).insert(Smaa::default());
        ready(&mut app);
        let silhouette_on = pixels(&mut app, &image);
        capture("silhouette-off", msaa.samples(), &silhouette_off);
        capture("silhouette-smaa", msaa.samples(), &silhouette_on);
        let changed = silhouette_off
            .as_chunks::<4>()
            .0
            .iter()
            .zip(silhouette_on.as_chunks::<4>().0.iter())
            .filter(|(before, after)| before != after)
            .count();
        assert!(
            changed > 0,
            "SMAA did not filter the diagonal depth silhouette at {}x MSAA",
            msaa.samples()
        );
        assert!(
            silhouette_on
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[0] > 0 && pixel[0] < 255),
            "SMAA silhouette lacks intermediate coverage"
        );
    }
}
