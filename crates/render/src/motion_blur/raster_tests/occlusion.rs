//! A nearer opaque surface remains sharp while a visible moving actor exposes the background.

use super::*;

pub(super) const NEAR_DEPTH: f32 = 0.5;

pub(super) struct Coverage {
    pub sample_mask: u64,
    pub occluder: Option<UVec4>,
    pub occluder_mask: u64,
    pub occluder_depth: f32,
    pub depth_slope: f32,
}

impl Raster {
    /// Writes a masked fixture surface at the requested original scene depth.
    pub(super) fn draw_occluder(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        samples: u32,
        bounds: UVec4,
        mask: u64,
        z: f32,
    ) {
        let shader = self
            .gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("actor exposure occluder fixture"),
                source: wgpu::ShaderSource::Wgsl(
                    format!(
                        r#"{}
struct Occluder {{ @builtin(frag_depth) depth: f32, @builtin(sample_mask) coverage: u32 }}
@fragment fn occluder() -> Occluder {{ return Occluder({z}, {}u); }}
"#,
                        super::super::shader_source(include_str!("../../motion_blur.wgsl"), false),
                        mask as u32,
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
                    mask,
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
                depth: None,
            }),
            Coverage {
                sample_mask: !0,
                occluder: Some(occluder),
                occluder_mask: !0,
                occluder_depth: NEAR_DEPTH,
                depth_slope: 0.0,
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

#[test]
fn mixed_msaa_actor_sources_do_not_spread_stationary_occluder_colour() {
    let Some(raster) = Raster::new("mixed actor and nearer occluder source raster") else {
        return;
    };
    let bounds = UVec4::new(60, 48, 8, 32);
    let mut uniform = exposure(Mat4::IDENTITY);
    uniform.strength.x = 1.0;
    uniform.strength.y = 17.0;
    let z = projection().project_point3(Vec3::new(0.0, 0.0, -4.0)).z;
    for &samples in raster.depth_samples.iter().filter(|&&samples| samples > 1) {
        let mask = 1 << (samples - 1);
        let red = (255.0 / samples as f32).round() as u8;
        let background_green = (128.0 * (1.0 - 1.0 / samples as f32)).round() as u8;
        let input = |nearer| {
            image(|x, y| {
                if x >= bounds.x
                    && x < bounds.x + bounds.z
                    && y >= bounds.y
                    && y < bounds.y + bounds.w
                {
                    if nearer {
                        [red, 0, 255 - red, 255]
                    } else {
                        [red, background_green, 0, 255]
                    }
                } else {
                    [0, 128, 0, 255]
                }
            })
        };
        let draw = |nearer, occluder_depth| {
            raster.draw_with_coverage(
                &input(nearer),
                uniform,
                samples,
                0.0,
                Some(ActorMotion {
                    velocity: [0.125, 0.0, 1.0, 1.0],
                    bounds,
                    depth: Some(z),
                }),
                Coverage {
                    sample_mask: mask,
                    occluder: Some(bounds),
                    occluder_mask: !mask,
                    occluder_depth,
                    depth_slope: 0.0,
                },
            )
        };
        let exterior = ((SIDE / 2 * SIDE + bounds.x - 3) * 4) as usize;
        for background_depth in [0.0, z] {
            let clear = draw(false, background_depth);
            assert!(
                clear[exterior] > 1,
                "partial actor with background depth{background_depth} must expose the background"
            );
        }
        let mixed = draw(true, NEAR_DEPTH);
        let mixed_input = input(true);
        for y in bounds.y..bounds.y + bounds.w {
            for x in bounds.x..bounds.x + bounds.z {
                let offset = ((y * SIDE + x) * 4) as usize;
                assert_eq!(
                    &mixed[offset..offset + 4],
                    &mixed_input[offset..offset + 4],
                    "nearer non-actor samples faded inside mixed source at ({x},{y})/{samples}x MSAA"
                );
            }
        }
        for x in bounds.x - 8..bounds.x + bounds.z + 8 {
            if x >= bounds.x && x < bounds.x + bounds.z {
                continue;
            }
            let offset = ((SIDE / 2 * SIDE + x) * 4) as usize;
            assert_eq!(
                mixed[offset + 2],
                0,
                "resolved nearer non-actor colour spread to x={x}/{samples}x MSAA"
            );
        }
    }
}

#[test]
fn actor_exposure_respects_full_and_partial_destination_depth_coverage() {
    let Some(raster) = Raster::new("actor source and destination depth coverage raster") else {
        return;
    };
    let actor = UVec4::new(60, 48, 8, 32);
    let receiver = UVec4::new(actor.x - 6, actor.y, 3, actor.w);
    let contains = |bounds: UVec4, x, y| {
        x >= bounds.x && x < bounds.x + bounds.z && y >= bounds.y && y < bounds.y + bounds.w
    };
    let z = projection().project_point3(Vec3::new(0.0, 0.0, -4.0)).z;
    let mut uniform = exposure(Mat4::IDENTITY);
    uniform.strength.x = 1.0;
    uniform.strength.y = 17.0;
    for &samples in &raster.depth_samples {
        let last_sample = 1 << (samples - 1);
        for partial_source in [false, true] {
            if partial_source && samples == 1 {
                continue;
            }
            let source_fraction = if partial_source {
                1.0 / samples as f32
            } else {
                1.0
            };
            for partial_destination in [false, true] {
                if partial_destination && samples == 1 {
                    continue;
                }
                let destination_fraction = if partial_destination {
                    1.0 / samples as f32
                } else {
                    1.0
                };
                for receiver_depth in [0.0, z, z + 0.000001, NEAR_DEPTH] {
                    let nearer = receiver_depth > z;
                    let input = image(|x, y| {
                        if contains(actor, x, y) {
                            [
                                (255.0 * source_fraction).round() as u8,
                                (128.0 * (1.0 - source_fraction)).round() as u8,
                                0,
                                255,
                            ]
                        } else if nearer && contains(receiver, x, y) {
                            [
                                0,
                                (128.0 * (1.0 - destination_fraction)).round() as u8,
                                (255.0 * destination_fraction).round() as u8,
                                255,
                            ]
                        } else {
                            [0, 128, 0, 255]
                        }
                    });
                    let output = raster.draw_with_coverage(
                        &input,
                        uniform,
                        samples,
                        0.0,
                        Some(ActorMotion {
                            velocity: [0.125, 0.0, 1.0, 1.0],
                            bounds: actor,
                            depth: Some(z),
                        }),
                        Coverage {
                            sample_mask: if partial_source { last_sample } else { !0 },
                            occluder: Some(receiver),
                            occluder_mask: if partial_destination { last_sample } else { !0 },
                            occluder_depth: receiver_depth,
                            depth_slope: 0.0,
                        },
                    );
                    for y in receiver.y..receiver.y + receiver.w {
                        for x in receiver.x..receiver.x + receiver.z {
                            let offset = ((y * SIDE + x) * 4) as usize;
                            if nearer {
                                assert_eq!(
                                    &output[offset..offset + 4],
                                    &input[offset..offset + 4],
                                    "nearer receiver changed: depth{receiver_depth}, sourcepartial={partial_source}, destinationpartial={partial_destination}, ({x},{y})/{samples}x MSAA"
                                );
                            } else {
                                assert!(
                                    output[offset] > 1,
                                    "equal or farther receiver lost actor exposure: depth{receiver_depth}, sourcepartial={partial_source}, destinationpartial={partial_destination}, ({x},{y})/{samples}x MSAA"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn world_camera_blur_is_independent_of_distant_stationary_actor_availability() {
    let Some(raster) = Raster::new("world exposure actor availability raster") else {
        return;
    };
    let terrain = UVec4::new(80, 0, SIDE - 80, SIDE);
    let input = image(|x, _| {
        if x >= terrain.x {
            [0, 0, 255, 255]
        } else {
            [0, 128, 0, 255]
        }
    });
    let uniform = exposure(Mat4::from_translation(Vec3::new(0.5, 0.0, 0.0)));
    let z = projection().project_point3(Vec3::new(0.0, 0.0, -4.0)).z;
    for &samples in &raster.depth_samples {
        let draw = |actor| {
            raster.draw_with_coverage(
                &input,
                uniform,
                samples,
                z,
                actor,
                Coverage {
                    sample_mask: !0,
                    occluder: Some(terrain),
                    occluder_mask: !0,
                    occluder_depth: NEAR_DEPTH,
                    depth_slope: 0.0,
                },
            )
        };
        let world = draw(None);
        assert_ne!(
            world, input,
            "world camera movement must retain terrain exposure"
        );
        let actor = draw(Some(ActorMotion {
            velocity: [0.0, 0.0, 1.0, 1.0],
            bounds: UVec4::new(8, 8, 2, 2),
            depth: None,
        }));
        assert_eq!(
            actor, world,
            "an unrelated stationary actor changed world blur at {samples}x MSAA"
        );
    }
}
