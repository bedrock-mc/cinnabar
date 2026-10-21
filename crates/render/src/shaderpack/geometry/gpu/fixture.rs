//! Ready, empty terrain geometry for render-host tests that need no chunk carriers.

use super::*;

impl ShaderpackGeometry {
    /// Creates ready, empty terrain for render-host tests without chunk carriers.
    pub(crate) fn ready_fixture(device: &RenderDevice) -> Self {
        let mut geometry = Self::new(device, Some(false));
        geometry.residency.follow_camera(DVec3::ZERO);
        geometry.inputs = Some(ExpansionInputs {
            materials: BufferId::new(),
            model_templates: BufferId::new(),
            biome_tints: BufferId::new(),
            biome_tint_table: ChunkBiomeTintIdentity::new(0, 0),
            atlas_generation: 0,
            attribute_key: AttributeKey {
                assets: 0,
                revision: 0,
                block_ids: None,
            },
            separate_ao: false,
            old_lighting: false,
        });
        geometry
    }
}
