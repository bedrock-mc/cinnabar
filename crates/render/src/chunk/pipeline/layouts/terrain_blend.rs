use crate::chunk::*;

/// Vanilla terrain_blend adds blending, not DisableDepthWrite. The exact-current
/// material parser enables depth writes when that state is absent;
/// the installed 1.26.51 terrain.material independently preserves this contract.
/// Bedrock's conventional LessEqual depth maps to GreaterEqual in reverse-Z.
pub(super) fn apply(descriptor: &mut RenderPipelineDescriptor) {
    let target = descriptor
        .fragment
        .as_mut()
        .expect("terrain fragment")
        .targets[0]
        .as_mut()
        .expect("terrain colour target");
    target.blend = Some(BlendState::ALPHA_BLENDING);
    // Inherited terrain_base DisableAlphaWrite: the same native parser maps
    // state bit 0x10 to an RGB-only colour mask.
    target.write_mask = ColorWrites::RED | ColorWrites::GREEN | ColorWrites::BLUE;
    descriptor
        .fragment
        .as_mut()
        .expect("terrain fragment")
        .shader_defs
        .push("NATIVE_GAMMA_BLEND".into());
    let depth = descriptor
        .depth_stencil
        .as_mut()
        .expect("terrain depth state");
    depth.depth_write_enabled = true;
    depth.depth_compare = CompareFunction::GreaterEqual;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_terrain_keeps_native_depth_writes() {
        let mut descriptor = RenderPipelineDescriptor {
            fragment: Some(FragmentState {
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::bevy_default(),
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: CompareFunction::Always,
                stencil: default(),
                bias: default(),
            }),
            ..default()
        };
        apply(&mut descriptor);
        let depth = descriptor.depth_stencil.unwrap();
        assert!(depth.depth_write_enabled);
        assert_eq!(depth.depth_compare, CompareFunction::GreaterEqual);
        let target = descriptor.fragment.unwrap().targets[0].clone().unwrap();
        assert_eq!(target.blend, Some(BlendState::ALPHA_BLENDING));
        assert_eq!(
            target.write_mask,
            ColorWrites::RED | ColorWrites::GREEN | ColorWrites::BLUE
        );
    }
}
