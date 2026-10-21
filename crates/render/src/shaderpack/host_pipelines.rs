//! The host's own full-screen passes around a pack (`blit.wgsl`): scene copies, depth copies,
//! the Vanilla overlay, presenting and mip downsampling.

use super::plan::MAX_COLOR_BUFFERS;
use wgpu::TextureFormat;
use wgpu::util::DeviceExt;

/// Whether a mip source can use the host's linear filtering sampler on this device.
pub(super) fn downsample_filterable(device: &wgpu::Device, format: TextureFormat) -> bool {
    format.sample_type(None, Some(device.features()))
        == Some(wgpu::TextureSampleType::Float { filterable: true })
}

/// Host passes around the pack's programs.
pub(crate) struct HostPipelines {
    pub scene_colour: wgpu::RenderPipeline,
    pub scene_depth: Option<wgpu::RenderPipeline>,
    /// Writes scene depth into comparison textures and the geometry depth attachment.
    pub scene_depth_attachment: Option<wgpu::RenderPipeline>,
    /// Copies the geometry depth attachment into a float `depthtex`/`shadowtex`.
    pub depth_copy: wgpu::RenderPipeline,
    /// Overlays what the Vanilla scene draws in front of the pack's opaque geometry (actors,
    /// particles, clouds) onto `colortex0` and the depth attachment.
    pub vanilla_overlay: wgpu::RenderPipeline,
    /// `present` per view target format.
    pub present: Vec<(TextureFormat, wgpu::RenderPipeline)>,
    /// Copies pixels outside a camera viewport through the post-process target swap.
    pub preserve: Vec<(TextureFormat, wgpu::RenderPipeline)>,
    /// `downsample` per mipmapped buffer format.
    pub downsample: Vec<(TextureFormat, wgpu::RenderPipeline)>,
    pub linear: wgpu::Sampler,
    /// The geometry adapter's zero-stride constant stream, for host-built sky vertices.
    pub constant_stream: wgpu::Buffer,
}

/// Builds the host passes for a pack's buffer and depth formats; `depth_targets` when the
/// pack has a geometry depth or comparison `depthtex*` for the scene depth to fill, and
/// `float_depths` the float `depthtex*` copies it allocates (`targets::float_depths`).
pub(super) fn host_pipelines(
    device: &wgpu::Device,
    buffer_formats: &[Option<TextureFormat>; MAX_COLOR_BUFFERS as usize],
    depth_format: TextureFormat,
    depth_targets: bool,
    float_depths: [bool; 3],
    mipmap_formats: &[TextureFormat],
) -> HostPipelines {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("shader pack host passes"),
        source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
    });
    let pipeline = |entry: &str, targets: &[Option<wgpu::ColorTargetState>], depth: bool| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(entry),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("fullscreen"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: depth.then(|| wgpu::DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets,
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    let target = |format: TextureFormat| {
        Some(wgpu::ColorTargetState {
            format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })
    };
    let colour0 = buffer_formats[0].unwrap_or(TextureFormat::Rgba8Unorm);
    let mut formats: Vec<TextureFormat> = mipmap_formats.to_vec();
    formats.dedup();
    HostPipelines {
        scene_colour: pipeline("scene_colour", &[target(colour0)], false),
        scene_depth: depth_copy_targets(depth_format, float_depths)
            .map(|targets| pipeline("scene_depth", &targets, false)),
        scene_depth_attachment: depth_targets
            .then(|| pipeline("scene_depth_attachment", &[], true)),
        depth_copy: pipeline("depth_copy", &[target(depth_format)], false),
        vanilla_overlay: pipeline("vanilla_overlay", &[target(colour0)], true),
        present: [
            render_scene::SCENE_COLOR_FORMAT,
            render_scene::SCENE_HDR_FORMAT,
        ]
        .into_iter()
        .map(|f| (f, pipeline("present", &[target(f)], false)))
        .collect(),
        preserve: [
            render_scene::SCENE_COLOR_FORMAT,
            render_scene::SCENE_HDR_FORMAT,
        ]
        .into_iter()
        .map(|f| (f, pipeline("preserve", &[target(f)], false)))
        .collect(),
        downsample: formats
            .into_iter()
            .map(|f| {
                let entry = if downsample_filterable(device, f) {
                    "downsample"
                } else {
                    "downsample_unfilterable"
                };
                (f, pipeline(entry, &[target(f)], false))
            })
            .collect(),
        linear: device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shader pack host linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        }),
        constant_stream: {
            let words = super::geometry::CONSTANT_STREAM_WORDS;
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("shader pack sky constants"),
                contents: bytemuck::cast_slice(&words),
                usage: wgpu::BufferUsages::VERTEX,
            })
        },
    }
}

/// Colour targets needed to copy sampled scene depths, absent when no copies are requested.
fn depth_copy_targets(
    format: TextureFormat,
    allocated: [bool; 3],
) -> Option<[Option<wgpu::ColorTargetState>; 3]> {
    allocated.contains(&true).then(|| {
        allocated.map(|allocated| {
            allocated.then_some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_shader_validates_with_viewport_preservation() {
        let module = naga::front::wgsl::parse_str(include_str!("blit.wgsl"))
            .expect("host passes must parse");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("host passes must validate");
        assert!(module.entry_points.iter().any(|entry| {
            entry.name == "preserve" && entry.stage == naga::ShaderStage::Fragment
        }));
    }

    #[test]
    fn colour_only_packs_need_no_depth_copy_pipeline() {
        assert!(depth_copy_targets(TextureFormat::R32Float, [false; 3]).is_none());
        let targets = depth_copy_targets(TextureFormat::R32Float, [false, true, false]).unwrap();
        assert!(targets[0].is_none() && targets[2].is_none());
        assert_eq!(targets[1].as_ref().unwrap().format, TextureFormat::R32Float);
    }
    #[test]
    fn gpu_colour_only_host_creates_no_targetless_depth_pipeline() {
        let Some(gpu) = crate::gpu_snapshot::Gpu::new() else {
            return;
        };
        let host = host_pipelines(
            &gpu.device,
            &[None; MAX_COLOR_BUFFERS as usize],
            TextureFormat::R32Float,
            false,
            [false; 3],
            &[],
        );
        assert!(host.scene_depth.is_none());
    }
}
