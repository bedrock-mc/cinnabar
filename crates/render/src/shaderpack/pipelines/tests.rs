use super::*;

#[test]
fn declared_float32_storage_formats_survive_without_filtering_support() {
    for (declared, format) in [
        (BufferFormat::R32f, TextureFormat::R32Float),
        (BufferFormat::Rg32f, TextureFormat::Rg32Float),
        (BufferFormat::Rgba32f, TextureFormat::Rgba32Float),
    ] {
        assert_eq!(texture_format(declared, FormatSupport::default()), format);
    }
}

#[test]
fn every_pack_format_maps_to_a_renderable_format_of_the_same_kind() {
    let device_formats = [
        FormatSupport::default(),
        FormatSupport {
            float32_filterable: true,
            rg11b10_renderable: true,
            norm16: true,
        },
    ];
    for support in device_formats {
        for &format in BufferFormat::ALL {
            let mapped = texture_format(format, support);
            let name = format.name();
            let expected = if name.ends_with("UI") {
                Kind::Uint
            } else if name.ends_with('I') {
                Kind::Sint
            } else {
                Kind::Float
            };
            assert_eq!(format_kind(mapped), expected, "{name} → {mapped:?}");
            if expected == Kind::Float {
                assert!(
                    support.float32_filterable
                        || filterable(mapped)
                        || matches!(
                            format,
                            BufferFormat::R32f
                                | BufferFormat::Rg32f
                                | BufferFormat::Rgb32f
                                | BufferFormat::Rgba32f
                        ),
                    "{name} must stay filterable without float32 filtering"
                );
            }
            assert!(!mapped.is_srgb(), "pack buffers hold raw values");
        }
    }
}

#[test]
fn missing_device_features_widen_formats_without_losing_precision() {
    let none = FormatSupport::default();
    assert_eq!(
        texture_format(BufferFormat::R11fG11fB10f, none),
        TextureFormat::Rgba16Float
    );
    assert_eq!(
        texture_format(BufferFormat::Rgba16, none),
        TextureFormat::Rgba16Float
    );
    let float = FormatSupport {
        float32_filterable: true,
        ..none
    };
    assert_eq!(
        texture_format(BufferFormat::Rgba16, float),
        TextureFormat::Rgba32Float
    );
    let all = FormatSupport {
        float32_filterable: true,
        rg11b10_renderable: true,
        norm16: true,
    };
    assert_eq!(
        texture_format(BufferFormat::Rgba16, all),
        TextureFormat::Rgba16Unorm
    );
    assert_eq!(
        texture_format(BufferFormat::R11fG11fB10f, all),
        TextureFormat::Rg11b10Ufloat
    );
}

#[test]
fn quad_buffer_places_each_supplied_attribute_at_its_fixed_location() {
    let bytes = quad_vertices();
    let stride = (QUAD_SLOTS * QUAD_ATTRIBUTE_BYTES) as usize;
    assert_eq!(bytes.len(), stride * 4);
    let read = |vertex: usize, attribute: VertexAttribute| -> [f32; 4] {
        let start = vertex * stride
            + attribute.location().unwrap() as usize * QUAD_ATTRIBUTE_BYTES as usize;
        std::array::from_fn(|i| {
            f32::from_le_bytes(bytes[start + i * 4..start + i * 4 + 4].try_into().unwrap())
        })
    };
    assert_eq!(read(3, VertexAttribute::Position), [1.0, 1.0, 0.0, 1.0]);
    assert_eq!(read(1, VertexAttribute::TexCoord), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(read(2, VertexAttribute::Color), [1.0; 4]);
    assert_eq!(read(0, VertexAttribute::BlockId), [0.0; 4]);
}

#[test]
fn colour_targets_require_matching_component_kinds_and_skip_integer_blending() {
    let float4 = ValueType::Vector {
        size: 4,
        scalar: ScalarType::Float,
    };
    let uint4 = ValueType::Vector {
        size: 4,
        scalar: ScalarType::Uint,
    };
    let additive = blend_state(BlendMode::Factors {
        src_color: PackFactor::One,
        dst_color: PackFactor::One,
        src_alpha: PackFactor::One,
        dst_alpha: PackFactor::Zero,
    });
    assert!(color_target(TextureFormat::R32Uint, float4, None).is_none());
    let target = color_target(TextureFormat::Rgba16Float, float4, additive).unwrap();
    assert_eq!(target.blend, additive);
    let integer = color_target(TextureFormat::Rgba8Uint, uint4, additive).unwrap();
    assert_eq!(integer.blend, None);
    assert_eq!(blend_state(BlendMode::Off), None);
}

#[test]
fn float32_textures_filter_with_the_feature_and_depth_reads_never_filter() {
    let integer = class_sampler(SamplerClass::RepeatNonFiltering);
    assert_eq!(integer.address_mode_u, wgpu::AddressMode::Repeat);
    assert_eq!(integer.mipmap_filter, wgpu::MipmapFilterMode::Nearest);
    let atlas = class_sampler(SamplerClass::RepeatNearest);
    assert_eq!(atlas.min_filter, wgpu::FilterMode::Nearest);
    assert_eq!(atlas.mipmap_filter, wgpu::MipmapFilterMode::Linear);
    let with = FormatSupport {
        float32_filterable: true,
        ..FormatSupport::default()
    };
    for format in [TextureFormat::R32Float, TextureFormat::Rgba32Float] {
        assert_eq!(
            texture_sample_type(format, with),
            TextureSampleType::Float { filterable: true }
        );
        assert_eq!(
            texture_sample_type(format, FormatSupport::default()),
            TextureSampleType::Float { filterable: false }
        );
    }
    let sampler = |binding, comparison| SamplerBinding {
        name: String::new(),
        group: shaderpack::TEXTURE_GROUP,
        binding,
        comparison,
        stages: StageMask::default(),
    };
    assert_eq!(
        sampler_binding_type(&sampler(SamplerClass::ClampNearest.binding(), false)),
        SamplerBindingType::NonFiltering
    );
    assert_eq!(
        sampler_binding_type(&sampler(SamplerClass::ClampLinear.binding(), false)),
        SamplerBindingType::Filtering
    );
    assert_eq!(
        sampler_binding_type(&sampler(SamplerClass::RepeatNearest.binding(), false)),
        SamplerBindingType::Filtering
    );
    assert_eq!(
        sampler_binding_type(&sampler(SamplerClass::RepeatNonFiltering.binding(), false)),
        SamplerBindingType::NonFiltering
    );
    assert_eq!(
        sampler_binding_type(&sampler(shaderpack::SHADOW_SAMPLER_BINDING, true)),
        SamplerBindingType::Comparison
    );
}
#[test]
fn norm16_feature_alone_does_not_select_an_unrenderable_target() {
    let support = FormatSupport::from_features(Features::TEXTURE_FORMAT_16BIT_NORM);
    for format in [BufferFormat::R16, BufferFormat::Rg16, BufferFormat::Rgba16] {
        assert!(
            texture_format(format, support)
                .guaranteed_format_features(Features::TEXTURE_FORMAT_16BIT_NORM)
                .allowed_usages
                .contains(TextureUsages::RENDER_ATTACHMENT)
        );
    }
}
#[test]
fn norm16_targets_require_effective_adapter_attachment_usages() {
    let features =
        Features::TEXTURE_FORMAT_16BIT_NORM | Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
    let allowed = |format: TextureFormat| {
        let mut support = format.guaranteed_format_features(features);
        support.allowed_usages |= TextureUsages::RENDER_ATTACHMENT;
        support
    };
    let exact = FormatSupport::for_device(features, allowed);
    assert_eq!(
        texture_format(BufferFormat::R16, exact),
        TextureFormat::R16Unorm
    );
    let limited = FormatSupport::for_device(features, |format| {
        let mut support = allowed(format);
        if format == TextureFormat::Rg16Unorm {
            support
                .allowed_usages
                .remove(TextureUsages::RENDER_ATTACHMENT);
        }
        support
    });
    assert_ne!(
        texture_format(BufferFormat::Rg16, limited),
        TextureFormat::Rg16Unorm
    );
    let guaranteed = FormatSupport::for_device(Features::TEXTURE_FORMAT_16BIT_NORM, allowed);
    assert!(!guaranteed.norm16);
    let missing_feature = effective_format_features(
        Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        TextureFormat::R16Unorm,
        allowed,
    );
    assert!(missing_feature.allowed_usages.is_empty());
}

#[test]
fn unsupported_reflected_interfaces_fail_layout_admission() {
    assert_eq!(
        view_dimension(PackDimension::D2).unwrap(),
        TextureViewDimension::D2
    );
    assert!(view_dimension(PackDimension::D1Array).is_err());
    let mut reflection = ProgramReflection {
        kind: shaderpack::ProgramKind::Render,
        entry_points: vec![],
        vertex_inputs: vec![],
        uniforms: None,
        uniform_buffers: vec![],
        storage_buffers: vec![],
        textures: vec![],
        samplers: vec![],
        storage_images: vec![],
        color_outputs: vec![],
        writes_depth: false,
        compute: None,
    };
    reflection.uniforms = Some(shaderpack::UniformBlock {
        name: "host".into(),
        group: 0,
        binding: 0,
        size: 16,
        members: vec![shaderpack::UniformMember {
            name: "viewWidth".into(),
            offset: 0,
            ty: ValueType::Scalar(ScalarType::Int64),
            array: None,
        }],
        stages: StageMask::default(),
    });
    assert!(
        layout_entries(&reflection, &[])
            .unwrap_err()
            .contains("viewWidth")
    );
    reflection.uniforms.as_mut().unwrap().members[0].ty = ValueType::Scalar(ScalarType::Int);
    assert!(layout_entries(&reflection, &[]).is_ok());
}

#[test]
fn transparent_colour_targets_preserve_the_draw_blend_without_overrides() {
    let reflection = ProgramReflection {
        kind: shaderpack::ProgramKind::Render,
        entry_points: vec![],
        vertex_inputs: vec![],
        uniforms: None,
        uniform_buffers: vec![],
        storage_buffers: vec![],
        textures: vec![],
        samplers: vec![],
        storage_images: vec![],
        color_outputs: vec![shaderpack::ColorOutput {
            location: 0,
            name: "color".into(),
            format: BufferFormat::Rgba8,
            buffer: Some(0),
            ty: ValueType::Vector {
                size: 4,
                scalar: ScalarType::Float,
            },
        }],
        writes_depth: false,
        compute: None,
    };
    let mut formats = [None; MAX_COLOR_BUFFERS as usize];
    formats[0] = Some(TextureFormat::Rgba8Unorm);
    let targets = color_targets(
        ProgramGroup::Gbuffers,
        "gbuffers_water",
        &reflection,
        &formats,
        &[],
        Some(BlendState::ALPHA_BLENDING),
    );
    assert_eq!(
        targets[0].as_ref().unwrap().blend,
        Some(BlendState::ALPHA_BLENDING)
    );
}

#[test]
fn colour_target_overrides_replace_the_draw_blend_in_precedence_order() {
    let reflection = ProgramReflection {
        kind: shaderpack::ProgramKind::Render,
        entry_points: vec![],
        vertex_inputs: vec![],
        uniforms: None,
        uniform_buffers: vec![],
        storage_buffers: vec![],
        textures: vec![],
        samplers: vec![],
        storage_images: vec![],
        writes_depth: false,
        compute: None,
        color_outputs: (0..2)
            .map(|index| shaderpack::ColorOutput {
                location: index,
                name: format!("color{index}"),
                buffer: Some(index),
                format: BufferFormat::Rgba8,
                ty: ValueType::Vector {
                    size: 4,
                    scalar: ScalarType::Float,
                },
            })
            .collect(),
    };
    let formats = [Some(TextureFormat::Rgba8Unorm); MAX_COLOR_BUFFERS as usize];
    let rules = [
        BlendRule {
            program: "gbuffers_water".into(),
            buffer: None,
            mode: BlendMode::Off,
        },
        BlendRule {
            program: "gbuffers_water".into(),
            buffer: Some("gdepth".into()),
            mode: BlendMode::Factors {
                src_color: PackFactor::One,
                dst_color: PackFactor::One,
                src_alpha: PackFactor::One,
                dst_alpha: PackFactor::Zero,
            },
        },
    ];
    let targets = color_targets(
        ProgramGroup::Gbuffers,
        "gbuffers_water",
        &reflection,
        &formats,
        &rules,
        Some(BlendState::ALPHA_BLENDING),
    );
    assert_eq!(targets[0].as_ref().unwrap().blend, None);
    assert_eq!(
        targets[1].as_ref().unwrap().blend,
        blend_state(rules[1].mode)
    );
    let opaque = color_targets(
        ProgramGroup::Gbuffers,
        "gbuffers_terrain",
        &reflection,
        &formats,
        &[],
        None,
    );
    assert!(opaque.iter().flatten().all(|target| target.blend.is_none()));
}
