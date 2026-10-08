use super::*;
use bevy::render::render_resource::DepthBiasState;

pub(super) struct UiPipelineSpecializer;

#[derive(Resource)]
pub(super) struct UiPipeline {
    pub(super) variants: Variants<RenderPipeline, UiPipelineSpecializer>,
    pub(super) bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for UiPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = ui_bind_group_layout();
        let descriptor = ui_pipeline_descriptor(bind_group_layout.clone());
        Self {
            variants: Variants::new(UiPipelineSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

/// Declares the shared viewport, texture-page and sampler bindings.
pub(crate) fn ui_bind_group_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "shared UI bind group layout",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                // The fragment stage reads the glint clock.
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<UiViewportUniform>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 4,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(16),
                },
                count: None,
            },
        ],
    )
}

/// The premultiplied-alpha blend state shared by every UI quad except the
/// crosshair.
pub(crate) fn ui_alpha_blend_state() -> BlendState {
    let blend = BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    };
    BlendState {
        color: blend,
        alpha: blend,
    }
}

/// The classic crosshair invert: color = src*(1-dst) + dst*(1-src), so the
/// white cross reads against any background; alpha passes the source through.
pub(crate) fn ui_invert_blend_state() -> BlendState {
    BlendState {
        color: BlendComponent {
            src_factor: BlendFactor::OneMinusDst,
            dst_factor: BlendFactor::OneMinusSrc,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::Zero,
            operation: BlendOperation::Add,
        },
    }
}

/// Builds the retained UI shader pipeline before per-view specialization.
pub(crate) fn ui_pipeline_descriptor(
    bind_group_layout: BindGroupLayoutDescriptor,
) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("shared retained UI overlay pipeline".into()),
        layout: vec![bind_group_layout],
        vertex: VertexState {
            shader: UI_SHADER_HANDLE,
            entry_point: Some("ui_vertex".into()),
            buffers: vec![VertexBufferLayout {
                array_stride: size_of::<UiRenderVertex>() as u64,
                step_mode: VertexStepMode::Vertex,
                attributes: vec![
                    VertexAttribute {
                        format: VertexFormat::Float32x4,
                        offset: std::mem::offset_of!(UiRenderVertex, position) as u64,
                        shader_location: 0,
                    },
                    VertexAttribute {
                        format: VertexFormat::Float32x2,
                        offset: std::mem::offset_of!(UiRenderVertex, uv) as u64,
                        shader_location: 1,
                    },
                    VertexAttribute {
                        format: VertexFormat::Unorm8x4,
                        offset: std::mem::offset_of!(UiRenderVertex, color) as u64,
                        shader_location: 2,
                    },
                    VertexAttribute {
                        format: VertexFormat::Uint32,
                        offset: std::mem::offset_of!(UiRenderVertex, style_flags) as u64,
                        shader_location: 3,
                    },
                    VertexAttribute {
                        format: VertexFormat::Float32,
                        offset: std::mem::offset_of!(UiRenderVertex, alpha_cutoff) as u64,
                        shader_location: 4,
                    },
                    VertexAttribute {
                        format: VertexFormat::Float32,
                        offset: std::mem::offset_of!(UiRenderVertex, model_light) as u64,
                        shader_location: 5,
                    },
                    VertexAttribute {
                        format: VertexFormat::Float32x4,
                        offset: std::mem::offset_of!(UiRenderVertex, overlay_color) as u64,
                        shader_location: 6,
                    },
                ],
            }],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: UI_SHADER_HANDLE,
            entry_point: Some("ui_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::bevy_default(),
                blend: Some(ui_alpha_blend_state()),
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        depth_stencil: None,
        ..default()
    }
}

// Vanilla environmental text: native constant bias -32.
// The same override zeros slope/clamp. Native LessEqual uses standard Z;
// our GreaterEqual reverse-Z comparison reverses the bias sign to retain the toward-eye shift.
pub(super) const NATIVE_ENVIRONMENTAL_TEXT_DEPTH_BIAS: i32 =
    -render_model::NAMETAG_TEXT_REVERSE_Z_BIAS;

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
pub(super) struct UiPipelineKey {
    pub(super) msaa: Msaa,
    pub(super) hdr: bool,
    pub(super) invert_blend: bool,
    pub(super) layer: bool,
    pub(super) depth_test: bool,
    pub(super) depth_write: bool,
    pub(super) isolated_depth: bool,
}

impl Specializer<RenderPipeline> for UiPipelineSpecializer {
    type Key = UiPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        descriptor.multisample.count = if key.layer { 1 } else { key.msaa.samples() };
        descriptor.fragment.as_mut().unwrap().entry_point = Some(
            if !key.layer && !key.invert_blend {
                "ui_world_fragment"
            } else {
                "ui_fragment"
            }
            .into(),
        );
        descriptor.depth_stencil =
            (key.depth_test || key.depth_write).then_some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: key.depth_write,
                depth_compare: if key.depth_test {
                    CompareFunction::GreaterEqual
                } else {
                    CompareFunction::Always
                },
                stencil: default(),
                // Depth-tested, depth-writing projected UI is the native environmental-text
                // mode. Plates are read-only; ordinary text uses Always; HUD has no depth state.
                bias: DepthBiasState {
                    constant: if key.depth_test && key.depth_write && !key.isolated_depth {
                        -NATIVE_ENVIRONMENTAL_TEXT_DEPTH_BIAS
                    } else {
                        0
                    },
                    ..default()
                },
            });
        let target = descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap();
        target.format = if key.layer {
            composite::UI_LAYER_FORMAT
        } else if key.hdr {
            ViewTarget::TEXTURE_FORMAT_HDR
        } else {
            TextureFormat::bevy_default()
        };
        target.blend = Some(if key.invert_blend {
            ui_invert_blend_state()
        } else {
            ui_alpha_blend_state()
        });
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hud_layer_and_projected_world_use_distinct_color_targets() {
        for layer in [true, false] {
            let mut descriptor = ui_pipeline_descriptor(ui_bind_group_layout());
            UiPipelineSpecializer
                .specialize(
                    UiPipelineKey {
                        msaa: Msaa::Sample4,
                        hdr: true,
                        invert_blend: false,
                        layer,
                        depth_test: !layer,
                        depth_write: !layer,
                        isolated_depth: false,
                    },
                    &mut descriptor,
                )
                .unwrap();
            let fragment = descriptor.fragment.unwrap();
            assert_eq!(
                fragment.entry_point.as_deref(),
                Some(if layer {
                    "ui_fragment"
                } else {
                    "ui_world_fragment"
                })
            );
            assert_eq!(
                fragment.targets[0].as_ref().unwrap().format,
                if layer {
                    composite::UI_LAYER_FORMAT
                } else {
                    ViewTarget::TEXTURE_FORMAT_HDR
                }
            );
            assert_eq!(
                descriptor.multisample.count,
                if layer { 1 } else { Msaa::Sample4.samples() }
            );
            assert_eq!(descriptor.depth_stencil.is_some(), !layer);
        }
    }

    #[test]
    fn ui_model_depth_never_inherits_environmental_text_bias() {
        for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
            for (depth_test, depth_write) in [(true, false), (false, true), (true, true)] {
                let mut descriptor = ui_pipeline_descriptor(ui_bind_group_layout());
                UiPipelineSpecializer
                    .specialize(
                        UiPipelineKey {
                            msaa,
                            hdr: false,
                            invert_blend: false,
                            layer: true,
                            depth_test,
                            depth_write,
                            isolated_depth: true,
                        },
                        &mut descriptor,
                    )
                    .unwrap();
                let depth = descriptor.depth_stencil.unwrap();
                assert_eq!(depth.depth_write_enabled, depth_write);
                assert_eq!(depth.bias.constant, 0);
                assert_eq!(depth.bias.slope_scale, 0.0);
                assert_eq!(descriptor.multisample.count, 1);
                assert_eq!(
                    descriptor.fragment.unwrap().targets[0]
                        .as_ref()
                        .unwrap()
                        .format,
                    composite::UI_LAYER_FORMAT
                );
            }
        }
    }
}
