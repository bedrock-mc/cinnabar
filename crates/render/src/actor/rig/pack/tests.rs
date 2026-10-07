use super::*;

/// Builds recognizable vertex bytes for namespace and capacity comparisons.
fn geometry(id: EntityRigId, count: usize) -> ActorRigGeometry {
    ActorRigGeometry::new(
        id,
        vec![
            ActorRigVertex {
                position: [id.0 as f32; 3],
                ..ActorRigVertex::default()
            };
            count
        ],
        vec![[0.0; 3]],
    )
    .unwrap()
}

/// Checks published addresses, vertex bytes and content identity independently of epochs.
fn assert_same_catalog(actual: &GeometryCatalog, expected: &GeometryCatalog) {
    assert_eq!(actual.geometries, expected.geometries);
    assert_eq!(actual.indices, expected.indices);
    assert_eq!(actual.published_spans, expected.published_spans);
    assert_eq!(actual.revision, expected.revision);
    assert_eq!(actual.vertices.len(), expected.vertices.len());
    for &span in actual.published_spans.iter() {
        assert_eq!(
            bytemuck::cast_slice::<ActorRigVertex, u8>(actual.vertices.span(span).unwrap()),
            bytemuck::cast_slice::<ActorRigVertex, u8>(expected.vertices.span(span).unwrap())
        );
    }
}

/// Combining updates retains sequential acceptance, including either capacity failure.
#[test]
fn combined_publication_matches_sequential_capacity_and_payloads() {
    let entity = render_model::pack_rig_id(0);
    let equipment = render_model::pack_equipment_rig_id(0);
    let pack_vertices = 240;
    let maximum_vertices = diagnostic_geometry().vertices.len() + pack_vertices;
    let quarter = pack_vertices / 4;
    let large = pack_vertices * 3 / 4;
    for case in 0..4 {
        let initial = [
            geometry(EntityRigId(1), 3),
            geometry(entity, quarter),
            geometry(equipment, quarter),
        ];
        let mut sequential = ActorRigFrameBuilder::new(initial.clone()).unwrap();
        let mut combined = ActorRigFrameBuilder::new(initial).unwrap();
        sequential.catalog.maximum_vertices = maximum_vertices;
        combined.catalog.maximum_vertices = maximum_vertices;
        let (entities, equipment_update, expected) = match case {
            0 => (
                vec![geometry(entity, 6), geometry(EntityRigId(1), 9)],
                vec![geometry(equipment, 9), geometry(entity, 12)],
                (Ok(()), Ok(())),
            ),
            1 => (
                vec![geometry(entity, large)],
                Vec::new(),
                (Err(ActorRigGeometryError::CatalogCapacity), Ok(())),
            ),
            2 => (
                vec![geometry(entity, quarter + 3)],
                vec![geometry(equipment, large)],
                (Ok(()), Err(ActorRigGeometryError::CatalogCapacity)),
            ),
            _ => (Vec::new(), Vec::new(), (Ok(()), Ok(()))),
        };
        let reference = (
            sequential.replace_pack_geometries(entities.clone()),
            sequential.replace_pack_equipment_geometries(equipment_update.clone()),
        );
        let result = combined.replace_session_pack_geometries(entities, equipment_update);
        assert_eq!(reference, expected, "case {case}");
        assert_eq!(result, reference, "case {case}");
        assert_same_catalog(&combined.catalog, &sequential.catalog);
    }
}

/// A session with neither namespace leaves existing GPU pages and their epoch untouched.
#[test]
fn empty_session_retains_catalog_identity() {
    let mut builder = ActorRigFrameBuilder::new([geometry(EntityRigId(1), 3)]).unwrap();
    let pages = builder.catalog.vertices.clone();
    let spans = builder.catalog.published_spans.clone();
    let revision = builder.catalog.revision;
    assert_eq!(
        builder.replace_session_pack_geometries(Vec::new(), Vec::new()),
        (Ok(()), Ok(()))
    );
    assert_eq!(builder.catalog.vertices.epoch, pages.epoch);
    assert_eq!(builder.catalog.revision, revision);
    assert!(Arc::ptr_eq(&builder.catalog.published_spans, &spans));
    assert!(Arc::ptr_eq(
        &builder.catalog.vertices.segments,
        &pages.segments
    ));
}
