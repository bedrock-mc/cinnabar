//! Admission checks for overlapping render attachments.

use crate::shaderpack::{loader::build, passes::ViewState, pipelines::GpuContext};
use std::sync::Arc;

/// Creates a headless device that still validates resource and pass descriptors.
fn gpu() -> GpuContext {
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
fn duplicate_active_colour_attachments_fail_view_admission() {
    let gpu = gpu();
    for program in ["composite", "gbuffers_terrain", "shadow"] {
        for (destinations, second_output, accepted) in
            [("00", true, false), ("01", true, true), ("00", false, true)]
        {
            let fragment = format!(
                "#version 120\n/* DRAWBUFFERS:{destinations} */\nvoid main() {{ gl_FragData[0] = vec4(1.0); {} }}",
                if second_output {
                    "gl_FragData[1] = vec4(0.5);"
                } else {
                    ""
                }
            );
            let vertex_name = format!("{program}.vsh");
            let fragment_name = format!("{program}.fsh");
            let vertex = "#version 120\nvoid main() { gl_Position = ftransform(); }";
            let mut files = vec![
                (vertex_name.as_str(), vertex),
                (fragment_name.as_str(), fragment.as_str()),
            ];
            if program != "composite" {
                files.extend([
                    ("gbuffers_basic.vsh", vertex),
                    (
                        "gbuffers_basic.fsh",
                        "#version 120\nvoid main() { gl_FragColor = vec4(1.0); }",
                    ),
                ]);
            }
            let source = shaderpack::ShaderPack::new(shaderpack::PackSource::from_files(files));
            let pack = build(&source, "attachments", "", None, &gpu, &Default::default()).unwrap();
            if program != "composite" {
                assert!(
                    pack.draws
                        .iter()
                        .any(|draw| pack.programs[draw.program].name == program),
                    "{program} must produce a draw"
                );
            }
            let state = ViewState::new(&gpu.device, Arc::new(pack), [4, 4]);
            assert_eq!(
                state.is_ok(),
                accepted,
                "{program} {destinations}: {:?}",
                state.err()
            );
        }
    }
}

#[test]
fn shadow_colour_feedback_fails_loading() {
    let gpu = gpu();
    let vertex = "#version 120\nvoid main() { gl_Position = ftransform(); }";
    for (sample, accepted) in [("shadowcolor0", false), ("shadowcolor1", true)] {
        let fragment = format!(
            "#version 120\n/* DRAWBUFFERS:0 */\nuniform sampler2D {sample};\nvoid main() {{ gl_FragData[0] = texture2D({sample}, vec2(0.5)); }}"
        );
        let source = shaderpack::ShaderPack::new(shaderpack::PackSource::from_files([
            ("gbuffers_basic.vsh", vertex),
            (
                "gbuffers_basic.fsh",
                "#version 120\nvoid main() { gl_FragColor = vec4(1.0); }",
            ),
            ("shadow.vsh", vertex),
            ("shadow.fsh", fragment.as_str()),
        ]));
        let result = build(
            &source,
            "shadow feedback",
            "",
            None,
            &gpu,
            &Default::default(),
        );
        assert_eq!(
            result.is_ok(),
            accepted,
            "{sample}: {:?}",
            result.as_ref().err()
        );
        if !accepted {
            assert!(
                result
                    .err()
                    .unwrap()
                    .contains("samples shadowcolor0 while writing it")
            );
        }
    }
}

#[test]
fn after_deferred_effect_storage_alias_fails_admission() {
    let gpu = gpu();
    let vertex = "#version 430 compatibility\nvoid main() { gl_Position = ftransform(); }";
    for (program, properties) in [
        ("gbuffers_weather", ""),
        ("gbuffers_particles", "particles.ordering=AFTER"),
        (
            "gbuffers_particles_translucent",
            "particles.ordering=BEFORE",
        ),
    ] {
        for (sample, accepted) in [(false, false), (true, true)] {
            let fragment = format!(
                "#version 430 compatibility\n/* DRAWBUFFERS:0 */\nlayout(rgba8) writeonly uniform image2D colorimg0;\nuniform sampler2D colortex0;\nvoid main() {{ imageStore(colorimg0, ivec2(0), vec4(1.0)); gl_FragData[0] = {}; }}",
                if sample {
                    "texture(colortex0, vec2(0.5))"
                } else {
                    "vec4(0.5)"
                }
            );
            let vertex_name = format!("{program}.vsh");
            let fragment_name = format!("{program}.fsh");
            let source = shaderpack::ShaderPack::new(shaderpack::PackSource::from_files([
                ("shaders.properties", properties),
                ("gbuffers_basic.vsh", vertex),
                (
                    "gbuffers_basic.fsh",
                    "#version 430 compatibility\nvoid main() { gl_FragColor = vec4(1.0); }",
                ),
                ("deferred.vsh", vertex),
                (
                    "deferred.fsh",
                    "#version 430 compatibility\nvoid main() { gl_FragColor = vec4(1.0); }",
                ),
                (vertex_name.as_str(), vertex),
                (fragment_name.as_str(), fragment.as_str()),
            ]));
            let pack = build(&source, "effect alias", "", None, &gpu, &Default::default()).unwrap();
            assert!(
                pack.draws
                    .iter()
                    .any(|draw| pack.programs[draw.program].name == program)
            );
            assert_ne!(pack.frame.scene_alt & 1, pack.frame.translucent_alt & 1);
            let result = ViewState::new(&gpu.device, Arc::new(pack), [4, 4]);
            assert_eq!(
                result.is_ok(),
                accepted,
                "{program} samples={sample}: {:?}",
                result.err()
            );
        }
    }
}
