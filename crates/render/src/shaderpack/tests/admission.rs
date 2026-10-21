//! Headless admission checks for geometry attachment feedback.

use super::*;

/// Builds a validation-only device for preparation tests that do not inspect pixels.
fn validation_gpu() -> GpuContext {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions {
                enable: true,
                ..Default::default()
            },
            ..Default::default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        bevy::tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    GpuContext {
        device,
        queue,
        adapter,
    }
}

#[test]
fn shadow_colour_feedback_is_rejected_before_recording() {
    let gpu = validation_gpu();
    for output in [0, 1] {
        let fragment = format!(
            "#version 120\nuniform sampler2D shadowcolor0;\n/* DRAWBUFFERS:{output} */\nvoid main() {{ gl_FragData[0] = texture2D(shadowcolor0, vec2(0.5)); }}"
        );
        let source = pack(&[
            ("gbuffers_basic.vsh", PASS_VERTEX),
            ("gbuffers_basic.fsh", FILL),
            ("gbuffers_terrain.vsh", PASS_VERTEX),
            ("gbuffers_terrain.fsh", FILL),
            ("shadow.vsh", PASS_VERTEX),
            ("shadow.fsh", &fragment),
        ]);
        let result = build(
            &source,
            "fixture",
            JAVA_DIMENSIONS[0],
            None,
            &gpu,
            &Default::default(),
        );
        if output == 0 {
            let Err(error) = result else {
                panic!("shadow colour feedback must fail preparation");
            };
            assert!(error.contains("shadowcolor0"), "{error}");
        } else {
            let loaded = result.expect("sampling a different shadow attachment remains supported");
            assert!(
                loaded.shadow.is_some(),
                "the fixture prepares active shadow draws"
            );
        }
    }
}

#[test]
fn geometry_storage_attachment_aliases_fail_view_admission() {
    let gpu = validation_gpu();
    for (output, sampled, accepted) in [(0, false, false), (1, false, true), (0, true, true)] {
        let fragment = format!(
            "#version 430\nlayout(rgba8) writeonly uniform image2D colorimg0;\n{}\n/* DRAWBUFFERS:{output} */\nvoid main() {{ imageStore(colorimg0, ivec2(0), vec4(1.0)); gl_FragData[0] = {}; }}",
            if sampled {
                "uniform sampler2D colortex0;"
            } else {
                ""
            },
            if sampled {
                "texelFetch(colortex0, ivec2(0), 0)"
            } else {
                "vec4(1.0)"
            },
        );
        let source = pack(&[
            ("gbuffers_basic.vsh", PASS_VERTEX),
            ("gbuffers_basic.fsh", FILL),
            ("gbuffers_terrain.vsh", PASS_VERTEX),
            ("gbuffers_terrain.fsh", &fragment),
        ]);
        let loaded = build(
            &source,
            "fixture",
            JAVA_DIMENSIONS[0],
            None,
            &gpu,
            &Default::default(),
        )
        .unwrap();
        assert!(
            !loaded.draws.is_empty(),
            "the fixture prepares active terrain draws"
        );
        let state = ViewState::new(&gpu.device, Arc::new(loaded), SIZE);
        assert_eq!(
            state.is_ok(),
            accepted,
            "output {output}, sampled {sampled}: {:?}",
            state.err()
        );
    }
}
