use super::super::jobs::FACING_REST;
use super::*;
use std::sync::Arc;

#[test]
fn expansion_sources_must_share_carrier_revision_and_authored_bindings() {
    let selected =
        ChunkTextureAssets::with_revision(Arc::new(assets::RuntimeAssets::diagnostic()), 1);
    let newer = ChunkTextureAssets::with_revision(Arc::clone(selected.assets()), 2);
    let other = ChunkTextureAssets::with_revision(Arc::new(assets::RuntimeAssets::diagnostic()), 1);
    assert!(expansion_sources_match(
        &selected,
        &selected.clone(),
        selected.identity()
    ));
    assert!(!expansion_sources_match(
        &newer,
        &selected,
        newer.identity()
    ));
    assert!(!expansion_sources_match(
        &other,
        &selected,
        other.identity()
    ));
    assert!(!expansion_sources_match(
        &selected,
        &selected,
        newer.identity()
    ));
    let page = || assets::TextureArray {
        layers: 0,
        mips: Box::new([]),
    };
    let authored =
        selected.with_updated_enhanced(Some(Arc::new(render_scene::EnhancedTextureAssets {
            color_pages: [page(), page()],
            normal_pages: [page(), page()],
            mer_pages: [page(), page()],
            texture_refs: Box::new([]),
        })));
    assert!(!expansion_sources_match(
        &authored,
        &selected,
        selected.identity()
    ));
    assert!(expansion_sources_match(
        &authored,
        &authored.clone(),
        selected.identity()
    ));
}

#[test]
fn release_all_drops_retained_expansion_buffers() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::new() else {
        return;
    };
    let device = RenderDevice::from(gpu.device);
    let queue = RenderQueue::new(gpu.queue);
    let mut geometry = ShaderpackGeometry::new(&device, Some(false));
    geometry
        .expansion
        .write(&device, &queue, &[0, 0], [0; PARAM_WORDS], &[&[[0; 4]]]);
    assert!(geometry.expansion.jobs().is_some());
    assert!(geometry.expansion.params(0).is_some());
    geometry.release_all();
    assert!(geometry.expansion.jobs().is_none());
    assert!(geometry.expansion.params(0).is_none());
    assert!(geometry.inputs.is_none());
    assert!(geometry.views.is_empty() && geometry.shadows.is_empty());
}

/// Inputs with independent GPU buffer identities and one atlas generation.
fn expansion_inputs() -> ExpansionInputs {
    ExpansionInputs {
        materials: BufferId::new(),
        model_templates: BufferId::new(),
        biome_tints: BufferId::new(),
        biome_tint_table: ChunkBiomeTintIdentity::new(4, 5),
        atlas_generation: 1,
        attribute_key: AttributeKey {
            assets: 1,
            revision: 1,
            block_ids: None,
        },
        separate_ao: false,
        old_lighting: false,
    }
}

#[test]
fn palette_refresh_keeps_resident_faces_drawable_until_their_replacement() {
    let previous = expansion_inputs();
    let mut current = previous.clone();
    current.biome_tints = BufferId::new();
    let mut residency = Residency::new(4096, 4096, TerrainLayout::WIDE.stride());
    residency.follow_camera(DVec3::ZERO);
    let mut plan = SectionPlan {
        faces: 1,
        categories: [0..1, 1..1, 1..1],
        ..Default::default()
    };
    plan.facing[GeometryCategory::Solid.index()][FACING_REST] = 0..1;
    let entities = [
        Entity::from_raw_u32(1).unwrap(),
        Entity::from_raw_u32(2).unwrap(),
    ];
    for entity in entities {
        let placement = residency.place(entity, &plan).unwrap();
        let mut placed = plan.clone();
        placed.place(placement.faces.start);
        residency.commit(entity, &placement, &placed, [0; 3]);
    }
    let draws = |residency: &Residency| {
        ViewDraws::build(&DrawInputs::new(
            residency.sections.values(),
            None,
            residency.epoch,
            residency.generation,
            None,
        ))
        .commands
        .len()
    };
    assert_eq!(
        draws(&residency),
        entities.len(),
        "fixture faces draw before refresh"
    );
    current.refresh_residency(Some(&previous), false, &mut residency, entities);
    assert_eq!(draws(&residency), entities.len());
    assert_eq!(residency.undrawn_len(), 0);
    assert_eq!(residency.take_queue().len(), entities.len());
    let placement = residency.place(entities[0], &plan).unwrap();
    let mut placed = plan.clone();
    placed.place(placement.faces.start);
    residency.commit(entities[0], &placement, &placed, [0; 3]);
    assert_eq!(draws(&residency), entities.len());
    let mut changed_table = current.clone();
    changed_table.biome_tint_table = ChunkBiomeTintIdentity::new(4, 6);
    changed_table.refresh_residency(Some(&current), false, &mut residency, entities);
    assert_eq!(draws(&residency), 0, "new index assignments hide old faces");
    let mut changed_atlas = current.clone();
    changed_atlas.atlas_generation += 1;
    changed_atlas.refresh_residency(Some(&current), false, &mut residency, entities);
    assert_eq!(draws(&residency), 0, "atlas changes still hide stale faces");
    let mut palette = changed_atlas.clone();
    palette.biome_tints = BufferId::new();
    palette.refresh_residency(Some(&changed_atlas), false, &mut residency, entities);
    assert_eq!(
        draws(&residency),
        0,
        "palette changes cannot revive stale atlas faces"
    );
}
