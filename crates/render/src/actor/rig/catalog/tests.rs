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

#[test]
fn actor_catalog_admits_multiple_individually_bounded_models() {
    let count = render_model::MAX_ACTOR_RIG_VERTICES / 2 + 1;
    let models = [geometry(1, count), geometry(2, count)];
    let catalog =
        GeometryCatalog::layout(models.into_iter().map(|model| (model.id, model)).collect())
            .expect("multiple valid models must not share one model's vertex ceiling");
    assert_eq!(catalog.vertices.len(), count * 2);
    assert_spans(&catalog);
}

#[test]
fn duplicate_vertex_pages_share_storage_and_keep_rig_metadata() {
    let count = 6;
    let first = geometry(1, count);
    let second = ActorRigGeometry::new(
        EntityRigId(2),
        first.vertices.to_vec(),
        vec![[3.0, 4.0, 5.0]],
    )
    .unwrap();
    assert!(!Arc::ptr_eq(&first.vertices, &second.vertices));
    let catalog = GeometryCatalog::layout_with_limit(
        [(first.id, first), (second.id, second)]
            .into_iter()
            .collect(),
        count,
    )
    .expect("identical vertex pages consume one immutable storage span");
    assert_ne!(
        catalog.indices[&EntityRigId(1)],
        catalog.indices[&EntityRigId(2)]
    );
    assert_eq!(catalog.vertices.len(), count);
    assert_eq!(catalog.vertices.segments.len(), 1);
    assert_eq!(catalog.published_spans[0], catalog.published_spans[1]);
    assert_eq!(catalog.geometries[&EntityRigId(1)].bone_pivots[0], [0.0; 3]);
    assert_eq!(
        catalog.geometries[&EntityRigId(2)].bone_pivots[0],
        [3.0, 4.0, 5.0]
    );
    assert_spans(&catalog);
}

#[test]
fn duplicate_vertex_pages_append_with_existing_addresses_and_snapshots() {
    let count = 6;
    let first = geometry(1, count);
    let second = ActorRigGeometry::new(
        EntityRigId(2),
        first.vertices.to_vec(),
        first.bone_pivots.to_vec(),
    )
    .unwrap();
    let mut catalog =
        GeometryCatalog::layout_with_limit([(first.id, first)].into_iter().collect(), count)
            .unwrap();
    let before = catalog.vertices.clone();
    let original_span = catalog.published_spans[0];
    catalog
        .append(vec![second], 2)
        .expect("an appended alias shares its existing immutable vertex page");
    assert_eq!(catalog.vertices.len(), count);
    assert_eq!(catalog.vertices.segments.len(), 1);
    assert_eq!(catalog.published_spans[0], original_span);
    assert_eq!(catalog.published_spans[1], original_span);
    assert!(Arc::ptr_eq(
        &before.segments[0],
        &catalog.vertices.segments[0]
    ));
    assert_eq!(
        before.span(original_span),
        catalog.vertices.span(original_span)
    );
    assert_spans(&catalog);
}

#[test]
fn shared_page_replacement_preserves_alias_metadata_and_old_snapshot() {
    let first = geometry(1, 3);
    let second = ActorRigGeometry::new(
        EntityRigId(2),
        first.vertices.to_vec(),
        vec![[3.0, 4.0, 5.0]],
    )
    .unwrap();
    let mut catalog = GeometryCatalog::layout(
        [(first.id, first), (second.id, second)]
            .into_iter()
            .collect(),
    )
    .unwrap();
    let before = catalog.vertices.clone();
    let alias_span = catalog.published_spans[catalog.indices[&EntityRigId(2)] as usize];
    let replacement = geometry(3, 3);
    let vertices = Arc::clone(&replacement.vertices);
    let first = ActorRigGeometry::new(EntityRigId(1), vertices.to_vec(), vec![[0.0; 3]]).unwrap();
    catalog.append(vec![first, replacement], 2).unwrap();
    assert_eq!(catalog.vertices.segments.len(), 2);
    assert_eq!(catalog.vertices.len(), 6);
    assert_eq!(
        catalog.published_spans[catalog.indices[&EntityRigId(1)] as usize],
        catalog.published_spans[catalog.indices[&EntityRigId(3)] as usize]
    );
    assert_eq!(
        catalog.published_spans[catalog.indices[&EntityRigId(2)] as usize],
        alias_span
    );
    assert_eq!(before.span(alias_span), catalog.vertices.span(alias_span));
    assert_eq!(
        catalog.geometries[&EntityRigId(2)].bone_pivots[0],
        [3.0, 4.0, 5.0]
    );
    assert_spans(&catalog);
}

#[test]
fn vertex_pages_preserve_exact_uv_bits_and_bone_indices() {
    let first = geometry(1, 3);
    let changed = |id, vertex: ActorRigVertex| {
        ActorRigGeometry::new(EntityRigId(id), vec![vertex; 3], vec![[0.0; 3]; 2]).unwrap()
    };
    let mut uv = first.vertices[0];
    uv.uv[0] = -0.0;
    let mut bone = first.vertices[0];
    bone.bone_index = 1;
    let geometries = [first, changed(2, uv), changed(3, bone)];
    let catalog = GeometryCatalog::layout(
        geometries
            .into_iter()
            .map(|geometry| (geometry.id, geometry))
            .collect(),
    )
    .unwrap();
    assert_eq!(catalog.vertices.len(), 9);
    assert_eq!(catalog.vertices.segments.len(), 3);
    assert_spans(&catalog);
}

#[test]
fn removing_a_shared_pack_range_keeps_the_remaining_route() {
    let pack = render_model::pack_rig_id(0);
    let first = geometry(1, 3);
    let alias =
        ActorRigGeometry::new(pack, first.vertices.to_vec(), vec![[3.0, 4.0, 5.0]]).unwrap();
    let mut builder = crate::ActorRigFrameBuilder::new([first, alias]).unwrap();
    let with_alias = builder.catalog.vertices.len();
    builder.replace_pack_geometries(Vec::new()).unwrap();
    assert!(!builder.contains_geometry(pack));
    assert!(builder.contains_geometry(EntityRigId(1)));
    assert_eq!(builder.catalog.vertices.len(), with_alias);
    assert_spans(&builder.catalog);
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
    let maximum_vertices = 96;
    let quarter = maximum_vertices / 4;
    let mut catalog = GeometryCatalog::layout_with_limit(
        [
            (EntityRigId(1), geometry(1, quarter * 2)),
            (EntityRigId(2), geometry(2, quarter)),
            (EntityRigId(3), geometry(3, quarter)),
        ]
        .into_iter()
        .collect(),
        maximum_vertices,
    )
    .unwrap();
    catalog.append(vec![geometry(1, quarter)], 2).unwrap();
    let before = catalog.vertices.clone();
    let old_spans = catalog.published_spans.clone();
    catalog.append(vec![geometry(3, quarter * 2)], 3).unwrap();
    assert_eq!(catalog.vertices.len(), maximum_vertices);
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

/// Initial revisions depend on page contents and routes, independent of allocation identity.
#[test]
fn initial_revision_tracks_content_and_routes() {
    let layout = |models: Vec<ActorRigGeometry>| {
        GeometryCatalog::layout(models.into_iter().map(|model| (model.id, model)).collect())
            .unwrap()
    };
    let original = layout(vec![geometry(1, 3), geometry(2, 6)]);
    let identical = layout(vec![geometry(1, 3), geometry(2, 6)]);
    assert_eq!(original.revision, identical.revision);
    let changed = layout(vec![geometry(1, 3), geometry(3, 6)]);
    assert_ne!(original.revision, changed.revision);
    let mut first = geometry(1, 3);
    let mut second = geometry(2, 6);
    std::mem::swap(&mut first.id, &mut second.id);
    let rerouted = layout(vec![first, second]);
    assert_ne!(original.revision, rerouted.revision);
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

    let pack = render_model::pack_rig_id(0);
    let equipment = render_model::pack_equipment_rig_id(0);
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

thread_local! {
    static LOOKUP_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts one explicit candidate inspection without changing production code.
pub(super) fn record_probe() {
    LOOKUP_PROBES.with(|count| count.set(count.get() + 1));
}

/// Starts an independent work sample on this test thread.
fn reset_probes() {
    LOOKUP_PROBES.with(|count| count.set(0));
}

/// Returns deterministic lookup work rather than elapsed time.
fn probes() -> usize {
    LOOKUP_PROBES.with(std::cell::Cell::get)
}

#[test]
fn bounded_placement_work_for_a_unique_geometry_burst() {
    let retained = render_model::MAX_RENDERED_PLAYERS;
    let added = retained * 2;
    let mut catalog = GeometryCatalog::layout(
        (1..=retained as u32)
            .map(|id| (EntityRigId(id), geometry(id, 3)))
            .collect(),
    )
    .unwrap();
    let before = catalog.vertices.clone();
    reset_probes();
    catalog
        .append(
            (retained + 1..=retained + added)
                .map(|id| geometry(id as u32, 3))
                .collect(),
            2,
        )
        .unwrap();
    assert!(
        probes() <= (retained + added) * 4,
        "{} range candidates for {added} new pages",
        probes()
    );
    assert_eq!(
        &catalog.vertices.offsets[..retained],
        before.offsets.as_ref()
    );
    assert_spans(&catalog);
}

thread_local! {
    static HASHED_VERTEX_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts only the vertex-byte hashing performed during catalog publication.
pub(super) fn record_vertex_hash(bytes: usize) {
    HASHED_VERTEX_BYTES.with(|count| count.set(count.get() + bytes));
}

/// A completed crowd may arrive in one tick, but neither catalog should rehash its meshes.
#[test]
fn prepared_crowd_catalogs_reuse_vertex_fingerprints() {
    let models: Vec<_> = (1..=render_model::MAX_RENDERED_PLAYERS as u32)
        .map(|id| geometry(id, 64))
        .collect();
    HASHED_VERTEX_BYTES.with(|count| count.set(0));
    for _ in 0..2 {
        let initial = GeometryCatalog::layout(
            models
                .iter()
                .cloned()
                .map(|model| (model.id, model))
                .collect(),
        )
        .unwrap();
        assert_eq!(initial.vertices.segments.len(), models.len());
        let mut catalog = GeometryCatalog::layout(BTreeMap::new()).unwrap();
        catalog.append(models.clone(), 1).unwrap();
        assert_eq!(catalog.vertices.segments.len(), models.len());
        assert_spans(&catalog);
    }
    assert_eq!(HASHED_VERTEX_BYTES.with(std::cell::Cell::get), 0);
}

/// Catalog storage accounting remains valid after transferring prepared aliases to one page.
#[test]
fn prepared_aliases_release_duplicate_source_allocations() {
    let first = geometry(1, 64);
    let second =
        ActorRigGeometry::new(EntityRigId(2), first.vertices.to_vec(), vec![[1.0; 3]]).unwrap();
    let duplicate = Arc::clone(&second.vertices);
    let mut catalog =
        GeometryCatalog::layout([(first.id, first), (second.id, second)].into()).unwrap();
    assert_eq!(
        Arc::strong_count(&duplicate),
        1,
        "only the external test witness retains duplicate bytes"
    );
    drop(duplicate);
    let first = &catalog.geometries[&EntityRigId(1)].vertices;
    let second = &catalog.geometries[&EntityRigId(2)].vertices;
    assert!(Arc::ptr_eq(first, second));
    assert_eq!(catalog.geometries[&EntityRigId(2)].bone_pivots[0], [1.0; 3]);
    HASHED_VERTEX_BYTES.with(|count| count.set(0));
    catalog.append(vec![geometry(3, 64)], 2).unwrap();
    assert_eq!(HASHED_VERTEX_BYTES.with(std::cell::Cell::get), 0);
    assert_spans(&catalog);
}
