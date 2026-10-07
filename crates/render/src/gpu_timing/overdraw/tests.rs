use super::*;
use bevy::render::render_resource::{
    CompareFunction, DepthBiasState, DepthStencilState, Face, FragmentState, StencilState,
};

#[test]
fn diagnostic_pipeline_preserves_geometry_and_alpha_entry() {
    let mut source = RenderPipelineDescriptor {
        fragment: Some(FragmentState {
            entry_point: Some("fragment".into()),
            shader_defs: vec!["EXISTING_MATERIAL_RULE".into()],
            targets: vec![Some(TextureFormat::Bgra8UnormSrgb.into())],
            ..Default::default()
        }),
        depth_stencil: Some(DepthStencilState {
            format: TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: CompareFunction::GreaterEqual,
            stencil: StencilState::default(),
            bias: DepthBiasState::default(),
        }),
        ..Default::default()
    };
    source.primitive.cull_mode = Some(Face::Back);
    source.vertex.entry_point = Some("vertex".into());
    let measured = descriptor(source.clone());
    let mut vertex = source.vertex.clone();
    vertex.shader_defs.push("OPAQUE_OVERDRAW".into());
    assert_eq!(measured.vertex, vertex);
    assert_eq!(measured.primitive, source.primitive);
    assert_eq!(measured.layout, source.layout);
    assert_eq!(measured.push_constant_ranges, source.push_constant_ranges);
    assert!(measured.depth_stencil.is_none());
    assert_eq!(measured.multisample.count, 1);
    let fragment = measured.fragment.unwrap();
    let original = source.fragment.unwrap();
    assert_eq!(fragment.shader, original.shader);
    assert_eq!(fragment.entry_point, original.entry_point);
    assert_eq!(fragment.shader_defs[0], original.shader_defs[0]);
    assert_eq!(fragment.shader_defs[1], "OPAQUE_OVERDRAW".into());
    let target = fragment.targets[0].as_ref().unwrap();
    assert_eq!(target.format, FORMAT);
    assert_eq!(target.write_mask, ColorWrites::RED);
    let blend = target.blend.unwrap();
    assert_eq!(blend.color.src_factor, BlendFactor::One);
    assert_eq!(blend.color.dst_factor, BlendFactor::One);
    assert_eq!(blend.color.operation, BlendOperation::Add);
    assert!(source.depth_stencil.unwrap().depth_write_enabled);
}

#[test]
fn layer_summary_excludes_padding_and_counts_each_covered_layer() {
    let row_bytes = 256;
    let mut bytes = vec![0xff; row_bytes * 4];
    for (row, values) in [[0x0000u16, 0x3c00, 0x4000], [0x4200, 0x4c00, 0x6800]]
        .into_iter()
        .enumerate()
    {
        for (column, value) in values.into_iter().enumerate() {
            let offset = (row + 1) * row_bytes + (column + 1) * 8;
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
    }
    let viewport = Viewport {
        physical_position: UVec2::ONE,
        physical_size: UVec2::new(3, 2),
        depth: 0.0..1.0,
    };
    let stats = readback::summarize(&bytes, row_bytes, &viewport);
    assert_eq!(stats.pixels, 6);
    assert_eq!(stats.covered, 5);
    assert_eq!(stats.layers, 1 + 2 + 3 + 16 + 2048);
    assert_eq!(stats.maximum, 2048);
    assert_eq!(stats.saturated, 1);
    assert_eq!(stats.histogram[0..4], [1, 1, 1, 1]);
    assert_eq!(stats.histogram[16], 2);
    assert_eq!(stats.histogram.iter().sum::<u64>(), 6);
}

#[test]
fn only_alpha_aware_terrain_variants_contribute_layers() {
    for stage in RuntimeStage::ALL {
        assert_eq!(
            counted(stage),
            [
                RuntimeStage::GpuTerrainSolid,
                RuntimeStage::GpuTerrainCutout,
                RuntimeStage::GpuTerrainModel,
            ]
            .contains(&stage)
        );
    }
}
