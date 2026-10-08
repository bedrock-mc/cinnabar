use crate::chunk::*;

pub(in crate::chunk) fn install_opaque_commands(render_app: &mut SubApp) {
    render_app
        .add_render_command::<Opaque3d, super::solid::DrawSolidChunkCommands>()
        .add_render_command::<Opaque3d, super::solid::DrawSolidChunkIndirectCommands>()
        .add_render_command::<Opaque3d, DrawChunkCommands>()
        .add_render_command::<Opaque3d, DrawChunkIndirectCommands>();
}

/// Identifies terrain draw functions without depending on their registration order.
pub(crate) fn timing_category(
    functions: &bevy::render::render_phase::DrawFunctionsInternal<Opaque3d>,
    draw: bevy::render::render_phase::DrawFunctionId,
) -> RuntimeStage {
    use crate::chunk::gpu_cull::DrawGpuCulledCommands;
    let categories = [
        (
            RuntimeStage::GpuTerrainSolid,
            [
                functions.get_id::<super::solid::DrawSolidChunkCommands>(),
                functions.get_id::<super::solid::DrawSolidChunkIndirectCommands>(),
                functions.get_id::<DrawGpuCulledCommands<0, false>>(),
                functions.get_id::<DrawGpuCulledCommands<0, true>>(),
            ],
        ),
        (
            RuntimeStage::GpuTerrainCutout,
            [
                functions.get_id::<DrawChunkCommands>(),
                functions.get_id::<DrawChunkIndirectCommands>(),
                functions.get_id::<DrawGpuCulledCommands<1, false>>(),
                functions.get_id::<DrawGpuCulledCommands<1, true>>(),
            ],
        ),
        (
            RuntimeStage::GpuTerrainModel,
            [
                functions.get_id::<DrawModelCommands>(),
                functions.get_id::<DrawModelIndirectCommands>(),
                functions.get_id::<DrawGpuCulledCommands<2, false>>(),
                functions.get_id::<DrawGpuCulledCommands<2, true>>(),
            ],
        ),
        (
            RuntimeStage::GpuTerrainDepthLiquid,
            [
                functions.get_id::<DrawDepthLiquidCommands>(),
                functions.get_id::<DrawDepthLiquidIndirectCommands>(),
                functions.get_id::<DrawGpuCulledCommands<3, false>>(),
                functions.get_id::<DrawGpuCulledCommands<3, true>>(),
            ],
        ),
    ];
    categories
        .into_iter()
        .find_map(|(stage, draws)| draws.contains(&Some(draw)).then_some(stage))
        .unwrap_or(RuntimeStage::GpuOpaqueOther)
}

/// The layer probe excludes GPU-cull streams whose late phase is outside the opaque node.
pub(crate) fn supports_layer_probe(
    functions: &bevy::render::render_phase::DrawFunctionsInternal<Opaque3d>,
    draw: bevy::render::render_phase::DrawFunctionId,
) -> bool {
    [
        functions.get_id::<super::solid::DrawSolidChunkCommands>(),
        functions.get_id::<super::solid::DrawSolidChunkIndirectCommands>(),
        functions.get_id::<DrawChunkCommands>(),
        functions.get_id::<DrawChunkIndirectCommands>(),
        functions.get_id::<DrawModelCommands>(),
        functions.get_id::<DrawModelIndirectCommands>(),
    ]
    .contains(&Some(draw))
}

#[cfg(test)]
mod timing_tests {
    use super::*;
    use bevy::render::render_phase::{Draw, DrawError, DrawFunctionId, DrawFunctionsInternal};

    struct NoDraw;

    impl Draw<Opaque3d> for NoDraw {
        fn draw<'w>(
            &mut self,
            _: &'w World,
            _: &mut TrackedRenderPass<'w>,
            _: Entity,
            _: &Opaque3d,
        ) -> Result<(), DrawError> {
            Ok(())
        }
    }

    /// Registers command identity without requiring the command's runtime resources.
    fn register<T: 'static>(functions: &mut DrawFunctionsInternal<Opaque3d>) -> DrawFunctionId {
        functions.add_with::<T, _>(NoDraw)
    }

    #[test]
    fn profiling_categories_cover_direct_indirect_and_gpu_culled_commands() {
        use crate::chunk::gpu_cull::DrawGpuCulledCommands;
        let functions = DrawFunctions::<Opaque3d>::default();
        let mut functions = functions.write();
        let solid = [
            register::<super::super::solid::DrawSolidChunkCommands>(&mut functions),
            register::<super::super::solid::DrawSolidChunkIndirectCommands>(&mut functions),
            register::<DrawGpuCulledCommands<0, false>>(&mut functions),
            register::<DrawGpuCulledCommands<0, true>>(&mut functions),
        ];
        let cutout = [
            register::<DrawChunkCommands>(&mut functions),
            register::<DrawChunkIndirectCommands>(&mut functions),
            register::<DrawGpuCulledCommands<1, false>>(&mut functions),
            register::<DrawGpuCulledCommands<1, true>>(&mut functions),
        ];
        let models = [
            register::<DrawModelCommands>(&mut functions),
            register::<DrawModelIndirectCommands>(&mut functions),
            register::<DrawGpuCulledCommands<2, false>>(&mut functions),
            register::<DrawGpuCulledCommands<2, true>>(&mut functions),
        ];
        let liquids = [
            register::<DrawDepthLiquidCommands>(&mut functions),
            register::<DrawDepthLiquidIndirectCommands>(&mut functions),
            register::<DrawGpuCulledCommands<3, false>>(&mut functions),
            register::<DrawGpuCulledCommands<3, true>>(&mut functions),
        ];
        for (draws, expected) in [
            (solid, RuntimeStage::GpuTerrainSolid),
            (cutout, RuntimeStage::GpuTerrainCutout),
            (models, RuntimeStage::GpuTerrainModel),
            (liquids, RuntimeStage::GpuTerrainDepthLiquid),
        ] {
            for (index, draw) in draws.into_iter().enumerate() {
                assert_eq!(timing_category(&functions, draw), expected);
                assert_eq!(
                    supports_layer_probe(&functions, draw),
                    index < 2 && expected != RuntimeStage::GpuTerrainDepthLiquid
                );
            }
        }
        let other = register::<NoDraw>(&mut functions);
        assert_eq!(
            timing_category(&functions, other),
            RuntimeStage::GpuOpaqueOther
        );
    }
}
