use super::*;

/// Creates distinct geometry bytes for catalog identity and snapshot checks.
fn geometry(id: u32, count: usize) -> ActorRigGeometry {
    ActorRigGeometry::new(
        EntityRigId(id),
        vec![
            ActorRigVertex {
                position: [id as f32; 3],
                ..ActorRigVertex::default()
            };
            count
        ],
        vec![[0.0; 3]; 1],
    )
    .unwrap()
}

/// Every public span must resolve to the original geometry's exact bytes.
fn assert_spans(catalog: &GeometryCatalog) {
    for (id, geometry) in &catalog.geometries {
        let span = catalog.published_spans[catalog.indices[id] as usize];
        assert_eq!(
            bytemuck::cast_slice::<ActorRigVertex, u8>(catalog.vertices.span(span).unwrap()),
            bytemuck::cast_slice::<ActorRigVertex, u8>(&geometry.vertices)
        );
    }
}

/// Crossing the old 32-segment threshold preserves all prior payloads and addresses.
#[test]
fn registrations_keep_pages_and_addresses() {
    let mut catalog =
        GeometryCatalog::layout([(EntityRigId(1), geometry(1, 36))].into_iter().collect()).unwrap();
    for id in 2..80 {
        let before = catalog.vertices.clone();
        let spans = catalog.published_spans.clone();
        catalog
            .append(vec![geometry(id, 36)], u64::from(id))
            .unwrap();
        assert_eq!(catalog.vertices.epoch, before.epoch);
        for (old, new) in before.segments.iter().zip(catalog.vertices.segments.iter()) {
            assert!(Arc::ptr_eq(old, new));
        }
        assert_eq!(&catalog.published_spans[..spans.len()], spans.as_ref());
        assert_spans(&catalog);
    }
}

/// Replacing a same-sized page reuses its address and preserves older snapshots.
#[test]
fn replacement_reuses_vacant_addresses() {
    let mut catalog = GeometryCatalog::layout(
        (1..3)
            .map(|id| (EntityRigId(id), geometry(id, 36)))
            .collect(),
    )
    .unwrap();
    let old = catalog.vertices.clone();
    let old_spans = catalog.published_spans.clone();
    for revision in 1..80 {
        catalog.append(vec![geometry(1, 36)], revision).unwrap();
        assert_eq!(catalog.vertices.len(), old.len());
        assert_eq!(catalog.published_spans, old_spans);
        assert!(Arc::ptr_eq(&catalog.vertices.segments[1], &old.segments[1]));
        assert_eq!(
            old.span(old_spans[0]),
            Some(geometry(1, 36).vertices.as_ref())
        );
        assert_spans(&catalog);
    }
}

/// Fragmentation changes metadata only, while still admitting every catalog that fits live.
#[test]
fn capacity_relocation_keeps_immutable_payloads() {
    let quarter = MAX_ACTOR_RIG_VERTICES / 4;
    let mut catalog = GeometryCatalog::layout(
        [
            (EntityRigId(1), geometry(1, quarter * 2)),
            (EntityRigId(2), geometry(2, quarter)),
            (EntityRigId(3), geometry(3, quarter)),
        ]
        .into_iter()
        .collect(),
    )
    .unwrap();
    catalog.append(vec![geometry(1, quarter)], 2).unwrap();
    let before = catalog.vertices.clone();
    let old_spans = catalog.published_spans.clone();
    catalog.append(vec![geometry(3, quarter * 2)], 3).unwrap();
    assert_eq!(catalog.vertices.len(), MAX_ACTOR_RIG_VERTICES);
    assert_ne!(catalog.published_spans[1], old_spans[1]);
    for index in 0..2 {
        assert!(Arc::ptr_eq(
            &before.segments[index],
            &catalog.vertices.segments[index]
        ));
        assert_eq!(
            before.span(old_spans[index]),
            Some(before.segments[index].as_ref())
        );
    }
    assert_spans(&catalog);
    let spans = catalog.published_spans.clone();
    let vertices = catalog.vertices.clone();
    assert!(catalog.append(vec![geometry(4, 1)], 4).is_err());
    assert_eq!(catalog.published_spans, spans);
    assert_eq!(catalog.vertices, vertices);
    assert_eq!(catalog.revision, 3);
}

/// Measures registration at the former segment-count cliff with a lobby-sized catalog.
#[test]
#[ignore = "benchmark"]
fn catalog_registration_threshold_bench() {
    let geometry = |id| {
        ActorRigGeometry::new(
            EntityRigId(id),
            vec![ActorRigVertex::default(); 1440],
            vec![[0.0; 3]; 4],
        )
        .unwrap()
    };
    let mut elapsed = std::time::Duration::ZERO;
    let mut copied_vertices = 0;
    for _ in 0..50 {
        let mut catalog =
            GeometryCatalog::layout((0..224).map(|id| (EntityRigId(id), geometry(id))).collect())
                .unwrap();
        for id in 224..255 {
            catalog.append(vec![geometry(id)], 1).unwrap();
        }
        let before = catalog.vertices.clone();
        let added = geometry(255);
        let start = std::time::Instant::now();
        catalog.append(vec![added], 2).unwrap();
        elapsed += start.elapsed();
        copied_vertices += catalog
            .vertices
            .segments
            .iter()
            .filter(|segment| !before.segments.iter().any(|old| Arc::ptr_eq(old, segment)))
            .map(|segment| segment.len())
            .sum::<usize>();
        std::hint::black_box(&catalog);
    }
    eprintln!(
        "CATALOG_THRESHOLD mean_us={:.3} fresh_vertex_bytes={}",
        elapsed.as_secs_f64() * 1e6 / 50.0,
        copied_vertices * std::mem::size_of::<ActorRigVertex>() / 50
    );
}

/// A paged initial catalog keeps the previous contiguous-layout content revision.
#[test]
fn initial_revision_matches_contiguous_content() {
    let catalog = GeometryCatalog::layout(
        (1..4)
            .map(|id| (EntityRigId(id), geometry(id, id as usize * 3)))
            .collect(),
    )
    .unwrap();
    let vertices: Vec<_> = catalog
        .geometries
        .values()
        .flat_map(|geometry| geometry.vertices.iter().copied())
        .collect();
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytemuck::cast_slice::<ActorRigVertex, u8>(&vertices)
        .iter()
        .chain(bytemuck::cast_slice::<ActorRigGeometrySpan, u8>(
            &catalog.published_spans,
        ))
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    assert_eq!(catalog.revision, hash.max(1));
}

/// Empty ranges retain GPU page identity, while clearing populated ranges still removes them.
#[test]
fn empty_pack_ranges_preserve_pages_and_populated_ranges_are_removed() {
    let mut builder = crate::ActorRigFrameBuilder::new([geometry(1, 3)]).unwrap();
    let before = builder.catalog.vertices.clone();
    let spans = builder.catalog.published_spans.clone();
    let revision = builder.catalog.revision;
    builder.replace_pack_geometries(Vec::new()).unwrap();
    builder
        .replace_pack_equipment_geometries(Vec::new())
        .unwrap();
    assert_eq!(builder.catalog.vertices.epoch, before.epoch);
    assert!(Arc::ptr_eq(&builder.catalog.published_spans, &spans));
    assert_eq!(builder.catalog.revision, revision);
    for (old, new) in before
        .segments
        .iter()
        .zip(builder.catalog.vertices.segments.iter())
    {
        assert!(Arc::ptr_eq(old, new));
    }

    let pack = crate::pack_rig_id(0);
    let equipment = crate::pack_equipment_rig_id(0);
    builder
        .replace_pack_geometries(vec![geometry(pack.0, 6)])
        .unwrap();
    let with_pack = builder.catalog.vertices.clone();
    builder
        .replace_pack_equipment_geometries(Vec::new())
        .unwrap();
    assert_eq!(builder.catalog.vertices.epoch, with_pack.epoch);
    assert_spans(&builder.catalog);

    builder
        .replace_pack_equipment_geometries(vec![geometry(equipment.0, 9)])
        .unwrap();
    assert!(builder.contains_geometry(equipment));
    builder
        .replace_pack_equipment_geometries(Vec::new())
        .unwrap();
    assert!(!builder.contains_geometry(equipment));
    assert!(builder.contains_geometry(pack));
    builder.replace_pack_geometries(Vec::new()).unwrap();
    assert!(!builder.contains_geometry(pack));
    assert!(builder.contains_geometry(EntityRigId(1)));
    assert_spans(&builder.catalog);
    assert_eq!(builder.catalog.revision, revision);
}
