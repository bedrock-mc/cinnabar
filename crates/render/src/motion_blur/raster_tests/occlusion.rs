//! A nearer opaque surface remains sharp while a visible moving actor exposes the background.

use super::*;

pub(super) struct Coverage {
    pub sample_mask: u64,
    pub occluder: Option<UVec4>,
}

impl Raster {
    /// Writes a nearby fixture surface into the same original depth used by both effect passes.
    pub(super) fn draw_occluder(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        samples: u32,
        bounds: UVec4,
    ) {
        let shader = self
            .gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("actor exposure occluder fixture"),
                source: wgpu::ShaderSource::Wgsl(
                    format!(
                        r#"{}
@fragment fn occluder() -> @builtin(frag_depth) f32 {{ return 0.5; }}
"#,
                        include_str!("../../motion_blur.wgsl")
                    )
                    .into(),
                ),
            });
        let pipeline = self
            .gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("actor exposure nearer depth"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("occluder"),
                    compilation_options: Default::default(),
                    targets: &[],
                }),
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("actor exposure nearer occluder"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_scissor_rect(bounds.x, bounds.y, bounds.z, bounds.w);
        pass.draw(0..3, 0..1);
    }
}

#[test]
fn swept_actor_exposure_preserves_nearer_occluders_and_background_colour() {
    let Some(raster) = Raster::new("swept actor nearer occlusion raster") else {
        return;
    };
    let actor = UVec4::new(60, 48, 8, 32);
    let occluder = UVec4::new(53, 44, 3, 40);
    let contains = |bounds: UVec4, x, y| {
        x >= bounds.x && x < bounds.x + bounds.z && y >= bounds.y && y < bounds.y + bounds.w
    };
    let input = image(|x, y| {
        if contains(occluder, x, y) {
            [0, 0, 255, 255]
        } else if contains(actor, x, y) {
            [255, 0, 0, 255]
        } else {
            [0, 128, 0, 255]
        }
    });
    let mut exposure = exposure(Mat4::IDENTITY);
    exposure.strength.x = 1.0;
    exposure.strength.y = 17.0;
    let z = projection().project_point3(Vec3::new(0.0, 0.0, -4.0)).z;
    for &samples in &raster.depth_samples {
        let output = raster.draw_with_coverage(
            &input,
            exposure,
            samples,
            z,
            Some(ActorMotion {
                velocity: [0.125, 0.0, 1.0, 1.0],
                bounds: actor,
            }),
            Coverage {
                sample_mask: !0,
                occluder: Some(occluder),
            },
        );
        for y in 40..88 {
            for x in 45..84 {
                let offset = ((y * SIDE + x) * 4) as usize;
                if contains(occluder, x, y) {
                    assert_eq!(
                        &output[offset..offset + 4],
                        &input[offset..offset + 4],
                        "actor exposure leaked through a nearer occluder at {samples}x MSAA"
                    );
                } else {
                    assert_eq!(
                        output[offset + 2],
                        0,
                        "nearer occluder colour leaked through a bilinear footprint at ({x},{y})/{samples}x MSAA"
                    );
                }
            }
        }
        let offset = ((SIDE / 2 * SIDE + actor.x + actor.z + 2) * 4) as usize;
        assert!(
            output[offset] > 8,
            "unobstructed background still needs exterior actor exposure"
        );
        assert!(
            output[offset + 1] > 0,
            "actor exposure must retain background colour"
        );
    }
}
