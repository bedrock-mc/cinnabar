//! Composed offscreen frames keep the HUD sharp while filtering the world beneath it.

use super::*;
use crate::{
    motion_blur::{CameraMotionBlur, CameraMotionBlurPlugin},
    ui_render::{UiRenderPlugin, UiRenderSceneResource, UiRenderStatsResource},
};
use render_model::{
    UiRenderBatch, UiRenderInput, UiRenderTextureArray, UiRenderVertex, UiScissor, UiTexturePage,
};

/// Publishes an opaque blue HUD rectangle over the red and green world fixture.
fn publish_hud(app: &mut App) {
    let vertices =
        [[25.0, 26.0], [39.0, 26.0], [39.0, 38.0], [25.0, 38.0]].map(|position| UiRenderVertex {
            position,
            clip_z: 0.0,
            clip_w: 1.0,
            uv: [0.5; 2],
            color: [0, 0, 255, 255],
            style_flags: 0,
            alpha_cutoff: -1.0,
            model_light: 1.0,
            overlay_color: [0.0; 4],
        });
    let input = UiRenderInput {
        revision: 1,
        viewport_size: [SIDE; 2],
        safe_area: [0; 4],
        vertices: vertices.to_vec().into(),
        indices: vec![0, 1, 2, 0, 2, 3].into(),
        batches: vec![UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, SIDE, SIDE),
            0,
            6,
            render_model::UI_BLEND_ALPHA,
        )]
        .into(),
        textures: Arc::new(
            UiRenderTextureArray::new(
                vec![UiTexturePage::owned([1, 1], vec![255; 4].into()).unwrap()],
                1,
            )
            .unwrap(),
        ),
    };
    let stats = app.world().resource::<UiRenderStatsResource>().clone();
    app.world_mut()
        .resource_mut::<UiRenderSceneResource>()
        .publish(input, &stats)
        .unwrap();
}

/// Samples final output without depending on any render schedule label.
fn pixel(bytes: &[u8], x: u32, y: u32) -> &[u8] {
    let index = ((y * SIDE + x) * 4) as usize;
    &bytes[index..index + 4]
}

/// Keeps camera exposure active while the real pipelines and retained HUD become ready.
fn moving_frames(app: &mut App, camera: Entity) {
    for _ in 0..8 {
        app.world_mut()
            .get_mut::<Transform>(camera)
            .unwrap()
            .rotate_y(0.06);
        frame(app);
    }
}

#[test]
fn hud_stays_sharp_over_spatial_filters_and_camera_exposure_in_either_toggle_order() {
    let Some(render) = renderer() else {
        return;
    };
    let (mut app, camera, image) = app_with_effects(render, |app| {
        app.insert_resource(crate::RuntimeStageProfiler::new(false));
        app.add_plugins((
            UiRenderPlugin,
            CameraMotionBlurPlugin,
            bevy::anti_alias::fxaa::FxaaPlugin,
            crate::GpuTimingPlugin,
        ));
    });
    let device = app.sub_app(RenderApp).world().resource::<RenderDevice>();
    let fixture = FixturePipelines {
        checker: fixture_pipeline(device, 1, "checker"),
        sloped_checker: fixture_pipeline(device, 1, "sloped_checker"),
        silhouette: fixture_pipeline(device, 1, "silhouette"),
        scene: 0,
    };
    app.sub_app_mut(RenderApp)
        .world_mut()
        .insert_resource(fixture);
    publish_hud(&mut app);
    moving_frames(&mut app, camera);
    let baseline = read_pixels(&app, &image);
    assert_eq!(pixel(&baseline, 32, 32), [0, 0, 255, 255]);
    for (smaa, blur, fxaa) in [
        (true, false, false),
        (true, true, false),
        (false, true, false),
        (false, false, false),
        (false, true, false),
        (true, true, false),
        (true, false, false),
        (false, false, false),
        (false, false, true),
        (false, true, true),
        (false, false, false),
    ] {
        if smaa {
            app.world_mut().entity_mut(camera).insert(Smaa::default());
        } else {
            app.world_mut().entity_mut(camera).remove::<Smaa>();
        }
        if blur {
            app.world_mut().entity_mut(camera).insert(CameraMotionBlur {
                exposure_seconds: 0.02,
                delta_seconds: 0.02,
                samples: 15,
                reset_epoch: 0,
            });
        } else {
            app.world_mut()
                .entity_mut(camera)
                .remove::<CameraMotionBlur>();
        }
        if fxaa {
            app.world_mut()
                .entity_mut(camera)
                .insert(bevy::anti_alias::fxaa::Fxaa::default());
        } else {
            app.world_mut()
                .entity_mut(camera)
                .remove::<bevy::anti_alias::fxaa::Fxaa>();
        }
        if smaa {
            ready(&mut app);
        }
        moving_frames(&mut app, camera);
        let output = read_pixels(&app, &image);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let blue = (25..39).contains(&x) && (26..38).contains(&y);
                assert_eq!(
                    pixel(&output, x, y)[2],
                    if blue { 255 } else { 0 },
                    "HUD boundary changed at ({x}, {y}), SMAA={smaa}, exposure={blur}, FXAA={fxaa}"
                );
            }
        }
        if blur {
            assert!(
                output
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(baseline.as_chunks::<4>().0.iter())
                    .any(|(actual, original)| actual != original && actual[2] == 0),
                "camera exposure did not filter world pixels"
            );
        } else if !fxaa {
            assert_eq!(
                output, baseline,
                "stationary authored texels changed with SMAA={smaa}"
            );
        }
    }
}
