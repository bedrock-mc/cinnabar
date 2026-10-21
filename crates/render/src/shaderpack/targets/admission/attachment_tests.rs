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
            let source = shaderpack::ShaderPack::new(shaderpack::PackSource::from_files([
                (
                    vertex_name.as_str(),
                    "#version 120\nvoid main() { gl_Position = ftransform(); }",
                ),
                (fragment_name.as_str(), &fragment),
            ]));
            let pack = build(&source, "attachments", "", None, &gpu, &Default::default()).unwrap();
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
