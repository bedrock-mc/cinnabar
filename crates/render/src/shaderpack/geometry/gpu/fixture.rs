//! Ready, empty terrain geometry for render-host tests that need no chunk carriers.

use super::*;

impl ShaderpackGeometry {
    /// Creates one pending expansion and drawable camera and shadow lists without carriers.
    pub(crate) fn pending_fixture(device: &RenderDevice) -> Self {
        let mut geometry = Self::ready_fixture(device);
        let entity = Entity::PLACEHOLDER;
        let mut plan = SectionPlan {
            faces: 1,
            categories: [0..1, 1..1, 1..1],
            ..default()
        };
        plan.facing[GeometryCategory::Solid.index()][super::super::jobs::FACING_REST] = 0..1;
        let placement = geometry.residency.place(entity, &plan).unwrap();
        plan.place(placement.faces.start);
        geometry.residency.commit(entity, &placement, &plan, [0; 3]);
        let inputs = DrawInputs::new(
            geometry.residency.sections.values(),
            None,
            geometry.residency.epoch,
            geometry.residency.generation,
            None,
        );
        geometry.views.insert(entity, ViewDraws::build(&inputs));
        geometry.shadows.insert(entity, ViewDraws::build(&inputs));
        let layout = device.create_bind_group_layout("empty expansion fixture", &[]);
        let bindings = device.create_bind_group("empty expansion fixture", &layout, &[]);
        geometry.prepared.dispatches.push(Dispatch {
            kind: JobKind::Cube,
            sources: bindings.clone(),
            output: bindings,
            jobs: 1,
        });
        geometry
    }

    /// Counts expansion dispatches and camera and shadow commands awaiting submission.
    pub(crate) fn pending_work(&self) -> [usize; 3] {
        [
            self.prepared.dispatches.len(),
            self.views.values().map(|draws| draws.commands.len()).sum(),
            self.shadows
                .values()
                .map(|draws| draws.commands.len())
                .sum(),
        ]
    }

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
