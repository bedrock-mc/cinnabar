//! The host's own full-screen passes around a pack (`blit.wgsl`): scene copies, presenting
//! and mip downsampling.

use super::plan::MAX_COLOR_BUFFERS;
use wgpu::TextureFormat;

/// Host passes around the pack's programs.
pub(crate) struct HostPipelines {
    pub scene_colour: wgpu::RenderPipeline,
    pub scene_depth: Option<wgpu::RenderPipeline>,
    /// Writes the scene depth into the comparison `depthtex*`.
    pub scene_depth_attachment: Option<wgpu::RenderPipeline>,
    /// `present` per view target format.
    pub present: Vec<(TextureFormat, wgpu::RenderPipeline)>,
    /// `downsample` per mipmapped buffer format.
    pub downsample: Vec<(TextureFormat, wgpu::RenderPipeline)>,
    pub depth_parameters: wgpu::Buffer,
    pub linear: wgpu::Sampler,
}

/// Builds the host passes for a pack's buffer and depth formats; `depth_targets` when the
/// pack has comparison `depthtex*` for the scene depth to fill, and
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
        present: [
            render_scene::SCENE_COLOR_FORMAT,
            render_scene::SCENE_HDR_FORMAT,
        ]
        .into_iter()
        .map(|f| (f, pipeline("present", &[target(f)], false)))
        .collect(),
        downsample: formats
            .into_iter()
            .map(|f| (f, pipeline("downsample", &[target(f)], false)))
            .collect(),
        depth_parameters: device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shader pack depth parameters"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        linear: device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shader pack host linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        }),
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
