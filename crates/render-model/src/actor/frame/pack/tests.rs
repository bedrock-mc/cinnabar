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

/// Combining updates matches sequential publication, including either range overflowing.
#[test]
fn combined_publication_matches_sequential_capacity_and_payloads() {
    let entity = crate::pack_rig_id(0);
    let equipment = crate::pack_equipment_rig_id(0);
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
            1 => (vec![geometry(entity, large)], Vec::new(), (Ok(()), Ok(()))),
            2 => (
                vec![geometry(entity, quarter + 3)],
                vec![geometry(equipment, large)],
                (Ok(()), Ok(())),
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
        assert_eq!(
            combined.staged.geometries, sequential.staged.geometries,
            "case {case}"
        );
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

/// Every resident rig's span must resolve to its own vertex bytes.
fn assert_spans(catalog: &GeometryCatalog) {
    for (id, geometry) in &catalog.geometries {
        let span = catalog.published_spans[catalog.indices[id] as usize];
        assert_eq!(
            bytemuck::cast_slice::<ActorRigVertex, u8>(catalog.vertices.span(span).unwrap()),
            bytemuck::cast_slice::<ActorRigVertex, u8>(&geometry.vertices)
        );
    }
}

/// Pack rig `index` with `count` vertices whose bytes differ from every other index.
fn pack_geometry(index: u32, count: usize) -> ActorRigGeometry {
    distinct(crate::pack_rig_id(index), index as f32 + 1.0, count)
}

/// Pack equipment rig `index`, with bytes distinct from every pack entity rig.
fn equipment_geometry(index: u32, count: usize) -> ActorRigGeometry {
    distinct(
        crate::pack_equipment_rig_id(index),
        -(index as f32) - 1.0,
        count,
    )
}

/// `count` vertices at `marker`, so geometries with different markers never share a page.
fn distinct(id: EntityRigId, marker: f32, count: usize) -> ActorRigGeometry {
    ActorRigGeometry::new(
        id,
        vec![
            ActorRigVertex {
                position: [marker; 3],
                ..ActorRigVertex::default()
            };
            count
        ],
        vec![[0.0; 3]],
    )
    .unwrap()
}

/// A submission of one unposed bone drawing `rig`.
fn draws(runtime_id: u64, rig: EntityRigId) -> ActorRigSubmission {
    let bone =
        RenderBoneTransform::from_model_space([0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0]).unwrap();
    ActorRigSubmission {
        material: ActorMaterial::default(),
        culling_bounds: Default::default(),
        input: ActorRigRenderInput {
            identity: ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id,
                spawn_revision: std::num::NonZeroU64::MIN,
                ingress_sequence: runtime_id,
                source_tick: Some(1),
                movement_revision: 1,
                pose_generation: PoseGeneration::new(1),
                layer: ACTOR_LAYER_BODY,
            },
            rig,
            previous_bones: std::iter::once(bone).collect(),
            current_bones: std::iter::once(bone).collect(),
            completed_tick: 1,
            reset_generation: 1,
        },
        world_from_actor: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 64.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        texture_layer: 0,
        route: ActorRigRoute::Compiled,
        tint: 0,
        uv_anim: crate::IDENTITY_UV_ANIM,
        light: 0,
        overlay_rgba8: 0,
    }
}

/// A server pack whose rigs together exceed the catalog still draws each spawned body.
#[test]
fn oversized_pack_range_draws_spawned_bodies() {
    let rigs = [
        crate::pack_rig_id(0),
        crate::pack_rig_id(1),
        crate::pack_rig_id(2),
    ];
    let mut builder = ActorRigFrameBuilder::new([geometry(EntityRigId(1), 3)]).unwrap();
    builder.catalog.maximum_vertices = builder.catalog.vertices.len() + 24;
    let pack = (0..rigs.len() as u32)
        .map(|index| pack_geometry(index, 12))
        .collect();
    assert_eq!(builder.replace_pack_geometries(pack), Ok(()));

    let frame = builder.build(0.5, None, [draws(1, rigs[0]), draws(2, rigs[1])]);
    assert_eq!(frame.instances.len(), 2);
    assert_eq!(frame.rejects.missing_geometry, 0);
    assert_spans(&builder.catalog);

    // A full catalog evicts staged rigs the frame does not draw, keeping retained GPU pages.
    let epoch = frame.geometry_vertices.epoch;
    let frame = builder.build(0.5, None, [draws(3, rigs[2])]);
    assert_eq!(frame.geometry_vertices.epoch, epoch);
    assert!(!builder.contains_geometry(rigs[0]));
    assert_eq!(frame.instances.len(), 1);
    assert_eq!(frame.rejects.missing_geometry, 0);
    assert!(builder.contains_geometry(rigs[2]));
    assert!(builder.contains_geometry(EntityRigId(1)));
    assert_spans(&builder.catalog);
}

/// A frame drawing more staged geometry than fits retries without changing the catalog.
#[test]
fn staged_overflow_within_one_frame_does_not_relayout_every_frame() {
    let rigs = [crate::pack_rig_id(0), crate::pack_rig_id(1)];
    let mut builder = ActorRigFrameBuilder::new([geometry(EntityRigId(1), 3)]).unwrap();
    builder.catalog.maximum_vertices = builder.catalog.vertices.len() + 12;
    let pack = (0..rigs.len() as u32)
        .map(|index| pack_geometry(index, 12))
        .collect();
    assert_eq!(builder.replace_pack_geometries(pack), Ok(()));
    let submissions = [draws(1, rigs[0]), draws(2, rigs[1])];

    let first = builder.build(0.5, None, submissions.clone());
    assert_eq!(first.instances.len(), 1);
    assert_eq!(first.rejects.missing_geometry, 1);
    let second = builder.build(0.5, None, submissions);
    assert_eq!(second.instances.len(), 1);
    assert_eq!(second.geometry_revision, first.geometry_revision);
    assert_eq!(
        second.geometry_vertices.epoch,
        first.geometry_vertices.epoch
    );
}

/// Replacing an overflowing range drops its staged rigs; a fitting range is resident at once.
#[test]
fn replacing_a_staged_range_discards_it() {
    let rig = crate::pack_rig_id(0);
    let mut builder = ActorRigFrameBuilder::new([geometry(EntityRigId(1), 3)]).unwrap();
    builder.catalog.maximum_vertices = builder.catalog.vertices.len() + 12;
    let oversized = vec![pack_geometry(0, 12), pack_geometry(1, 12)];
    assert_eq!(builder.replace_pack_geometries(oversized), Ok(()));
    assert!(!builder.staged.is_empty());

    assert_eq!(
        builder.replace_pack_geometries(vec![pack_geometry(0, 6)]),
        Ok(())
    );
    assert!(builder.staged.is_empty());
    assert!(builder.contains_geometry(rig));
    assert_eq!(builder.replace_pack_geometries(Vec::new()), Ok(()));
    assert!(!builder.contains_geometry(rig));
}

/// Admission uploads a bounded amount per frame but always progresses by at least one rig.
#[test]
fn staged_admission_is_bounded_per_frame() {
    let big = STAGED_ADMISSION_VERTICES_PER_FRAME / 2 + 6;
    let rigs = [crate::pack_rig_id(0), crate::pack_rig_id(1)];
    let mut builder = ActorRigFrameBuilder::new([geometry(EntityRigId(1), 3)]).unwrap();
    let resident = builder.catalog.vertices.len();
    builder.catalog.maximum_vertices = resident + big * 2 - 6;
    let pack = (0..rigs.len() as u32)
        .map(|index| pack_geometry(index, big))
        .collect();
    assert_eq!(builder.replace_pack_geometries(pack), Ok(()));
    builder.catalog.maximum_vertices = resident + big * 2;
    let submissions = [draws(1, rigs[0]), draws(2, rigs[1])];

    let first = builder.build(0.5, None, submissions.clone());
    assert_eq!(first.instances.len(), 1);
    let second = builder.build(0.5, None, submissions);
    assert_eq!(second.instances.len(), 2);
}

/// A larger device budget makes a staged pack wholly resident again.
#[test]
fn raising_the_budget_makes_a_staged_pack_resident() {
    let mut builder = ActorRigFrameBuilder::new([geometry(EntityRigId(1), 3)]).unwrap();
    let resident = builder.catalog.vertices.len();
    builder.catalog.maximum_vertices = resident + 12;
    let pack = vec![pack_geometry(0, 12), pack_geometry(1, 12)];
    let equipment = vec![equipment_geometry(0, 6)];
    assert_eq!(
        builder.replace_session_pack_geometries(pack, equipment),
        (Ok(()), Ok(()))
    );
    assert!(!builder.staged.is_empty());

    builder.set_vertex_budget(resident + 30);
    assert!(builder.staged.is_empty());
    for id in [
        crate::pack_rig_id(0),
        crate::pack_rig_id(1),
        crate::pack_equipment_rig_id(0),
    ] {
        assert!(builder.contains_geometry(id));
    }
}

/// Eviction spans both pack namespaces and drops the least recently drawn rig first.
#[test]
fn eviction_takes_the_least_recently_drawn_rig_of_either_namespace() {
    let (first, second) = (crate::pack_rig_id(0), crate::pack_rig_id(1));
    let held = crate::pack_equipment_rig_id(0);
    let mut builder = ActorRigFrameBuilder::new([geometry(EntityRigId(1), 3)]).unwrap();
    builder.catalog.maximum_vertices = builder.catalog.vertices.len() + 24;
    let (entities, equipment) = (
        vec![pack_geometry(0, 12), pack_geometry(1, 12)],
        vec![equipment_geometry(0, 12)],
    );
    assert_eq!(
        builder.replace_session_pack_geometries(entities, equipment),
        (Ok(()), Ok(()))
    );
    let _ = builder.build(0.5, None, [draws(1, first)]);
    let _ = builder.build(0.5, None, [draws(2, second)]);

    let frame = builder.build(0.5, None, [draws(3, held)]);
    assert_eq!(frame.instances.len(), 1);
    assert!(!builder.contains_geometry(first));
    assert!(builder.contains_geometry(second));
    assert!(builder.contains_geometry(held));
}

/// Only the submission that survives deduplication claims staged capacity.
#[test]
fn superseded_submissions_do_not_claim_staged_capacity() {
    let (stale, current) = (crate::pack_rig_id(0), crate::pack_rig_id(1));
    let mut builder = ActorRigFrameBuilder::new([geometry(EntityRigId(1), 3)]).unwrap();
    builder.catalog.maximum_vertices = builder.catalog.vertices.len() + 12;
    let pack = vec![pack_geometry(0, 12), pack_geometry(1, 12)];
    assert_eq!(builder.replace_pack_geometries(pack), Ok(()));
    let older = draws(1, stale);
    let mut newer = draws(1, current);
    newer.input.identity.ingress_sequence = 2;

    let frame = builder.build(0.5, None, [newer, older]);
    assert_eq!(frame.instances.len(), 1);
    assert_eq!(frame.manifest[0].rig, current);
    assert!(!builder.contains_geometry(stale));
}

/// A rig too large to ever fit does not keep smaller drawn rigs out.
#[test]
fn an_unfittable_rig_does_not_starve_smaller_ones() {
    let (large, small) = (crate::pack_rig_id(0), crate::pack_rig_id(1));
    let mut builder = ActorRigFrameBuilder::new([geometry(EntityRigId(1), 3)]).unwrap();
    builder.catalog.maximum_vertices = builder.catalog.vertices.len() + 12;
    let pack = vec![pack_geometry(0, 20), pack_geometry(1, 6)];
    assert_eq!(builder.replace_pack_geometries(pack), Ok(()));

    let frame = builder.build(0.5, None, [draws(1, large), draws(2, small)]);
    assert_eq!(frame.instances.len(), 1);
    assert_eq!(frame.rejects.missing_geometry, 1);
    assert!(builder.contains_geometry(small));
}

/// A rig whose vertices are already resident under another rig adds no storage.
#[test]
fn a_rig_sharing_a_resident_page_is_admitted_into_a_full_catalog() {
    let (first, twin) = (crate::pack_rig_id(0), crate::pack_rig_id(1));
    let mut builder = ActorRigFrameBuilder::new([geometry(EntityRigId(1), 3)]).unwrap();
    builder.catalog.maximum_vertices = builder.catalog.vertices.len() + 12;
    let pack = vec![
        pack_geometry(0, 12),
        distinct(twin, 1.0, 12),
        pack_geometry(2, 12),
    ];
    assert_eq!(builder.replace_pack_geometries(pack), Ok(()));
    let _ = builder.build(0.5, None, [draws(1, first)]);

    let frame = builder.build(0.5, None, [draws(1, first), draws(2, twin)]);
    assert_eq!(frame.instances.len(), 2);
    assert_eq!(frame.rejects.missing_geometry, 0);
}

/// Dynamic meshes reclaim old pack pages while retaining the most recently drawn rig.
#[test]
fn dynamic_geometry_reclaims_staged_capacity() {
    let (old, visible) = (crate::pack_rig_id(0), crate::pack_rig_id(1));
    let dynamic = EntityRigId(2);
    let mut builder = ActorRigFrameBuilder::new([geometry(EntityRigId(1), 3)]).unwrap();
    builder.catalog.maximum_vertices = builder.catalog.vertices.len() + 24;
    builder
        .replace_pack_geometries((0..3).map(|index| pack_geometry(index, 12)).collect())
        .unwrap();
    let _ = builder.build(0.5, None, [draws(1, old), draws(2, visible)]);
    let previous = builder.build(0.5, None, [draws(2, visible)]);

    assert_eq!(builder.insert_geometry(distinct(dynamic, 20.0, 12)), Ok(()));
    let frame = builder.build(0.5, None, [draws(2, visible), draws(3, dynamic)]);
    assert_eq!(frame.instances.len(), 2);
    assert_eq!(
        frame.geometry_vertices.epoch,
        previous.geometry_vertices.epoch
    );
    assert!(!builder.contains_geometry(old));
    assert_spans(&builder.catalog);

    // Replacing a dynamic mesh releases its old page before charging the new batch.
    assert_eq!(
        builder.insert_geometries(vec![geometry(dynamic, 6), geometry(EntityRigId(3), 6)]),
        Ok(())
    );
    assert!(builder.contains_geometry(visible));
    assert_spans(&builder.catalog);
    let revision = builder.catalog.revision;
    assert_eq!(
        builder.insert_geometry(geometry(EntityRigId(4), 40)),
        Err(ActorRigGeometryError::CatalogCapacity)
    );
    assert_eq!(builder.catalog.revision, revision);
    assert!(builder.contains_geometry(visible));
    assert!(builder.contains_geometry(dynamic));
}

/// Rejected poses neither claim fresh pack capacity nor protect a previously resident rig.
#[test]
fn invalid_staged_poses_cannot_starve_a_valid_draw() {
    let (invalid, valid) = (crate::pack_rig_id(0), crate::pack_rig_id(1));
    for resident in [false, true] {
        for case in 0..5 {
            let mut malformed = draws(1, invalid);
            let mut small = pack_geometry(0, 6);
            match case {
                0 => {
                    malformed.input.previous_bones = Arc::from([]);
                    malformed.input.current_bones = Arc::from([]);
                }
                1 => malformed.input.current_bones = Arc::from([]),
                2 => Arc::make_mut(&mut malformed.input.current_bones)[0].rotation[0] = f32::NAN,
                3 => {
                    let mut vertices = small.vertices.to_vec();
                    vertices[0].bone_index = 1;
                    small = ActorRigGeometry::new(invalid, vertices, vec![[0.0; 3]; 2]).unwrap();
                }
                _ => {
                    let bone = &mut Arc::make_mut(&mut malformed.input.current_bones)[0];
                    bone.translation_scale[3] = f32::MAX;
                    bone.axis_scale[0] = f32::MAX;
                }
            }
            let mut builder = ActorRigFrameBuilder::new([]).unwrap();
            builder.catalog.maximum_vertices = builder.catalog.vertices.len() + 12;
            builder
                .replace_pack_geometries(vec![small, pack_geometry(1, 12)])
                .unwrap();
            if resident {
                let mut initial = draws(1, invalid);
                initial.input.previous_bones = vec![initial.input.previous_bones[0]; 2].into();
                initial.input.current_bones = Arc::clone(&initial.input.previous_bones);
                assert_eq!(builder.build(0.5, None, [initial]).instances.len(), 1);
            }
            let frame = builder.build(0.5, None, [malformed, draws(2, valid)]);
            assert_eq!(frame.instances.len(), 1, "resident {resident}, case {case}");
            assert_eq!(
                frame.manifest[0].rig, valid,
                "resident {resident}, case {case}"
            );
            assert!(!builder.contains_geometry(invalid));
            assert_spans(&builder.catalog);
        }
    }
}

/// Vertex edits refresh the required bone count before either pack namespace is staged.
#[test]
fn staged_publication_revalidates_replaced_vertex_storage() {
    for equipment in [false, true] {
        for combined in [false, true] {
            let rig = if equipment {
                crate::pack_equipment_rig_id
            } else {
                crate::pack_rig_id
            };
            let mut edited = ActorRigGeometry::new(
                rig(0),
                vec![
                    ActorRigVertex {
                        bone_index: 1,
                        ..ActorRigVertex::default()
                    };
                    6
                ],
                vec![[0.0; 3]; 2],
            )
            .unwrap();
            edited.vertices = vec![ActorRigVertex::default(); 6].into();
            let pack = vec![edited, distinct(rig(1), 7.0, 12)];
            let mut builder = ActorRigFrameBuilder::new([]).unwrap();
            builder.catalog.maximum_vertices = builder.catalog.vertices.len() + 12;
            if combined {
                let (entities, worn) = if equipment {
                    (Vec::new(), pack)
                } else {
                    (pack, Vec::new())
                };
                assert_eq!(
                    builder.replace_session_pack_geometries(entities, worn),
                    (Ok(()), Ok(()))
                );
            } else if equipment {
                builder.replace_pack_equipment_geometries(pack).unwrap();
            } else {
                builder.replace_pack_geometries(pack).unwrap();
            }
            let frame = builder.build(0.5, None, [draws(1, rig(0))]);
            assert_eq!(
                frame.instances.len(),
                1,
                "equipment {equipment}, combined {combined}"
            );
            assert_eq!(frame.manifest[0].rig, rig(0));
            assert_spans(&builder.catalog);
        }
    }
}

/// Newly inserted pack rigs remain evictable and can return after another body needs space.
#[test]
fn inserted_pack_rigs_share_staged_residency() {
    for inserted in [crate::pack_rig_id(9), crate::pack_equipment_rig_id(9)] {
        let body = crate::pack_rig_id(0);
        let mut builder = ActorRigFrameBuilder::new([]).unwrap();
        builder.catalog.maximum_vertices = builder.catalog.vertices.len() + 12;
        builder
            .replace_pack_geometries(vec![pack_geometry(0, 12), pack_geometry(1, 12)])
            .unwrap();
        builder
            .insert_geometry(distinct(inserted, 20.0, 12))
            .unwrap();
        for (generation, rig) in [inserted, body, inserted].into_iter().enumerate() {
            let frame = builder.build(0.5, None, [draws(generation as u64 + 1, rig)]);
            assert_eq!(frame.instances.len(), 1, "rig {rig:?}");
            assert_eq!(frame.manifest[0].rig, rig);
            assert_spans(&builder.catalog);
        }
    }
}

/// Successful replacements survive eviction; rejected replacements leave both copies intact.
#[test]
fn inserted_pack_replacements_survive_eviction() {
    let replaced = crate::pack_rig_id(0);
    let other = crate::pack_rig_id(1);
    let mut builder = ActorRigFrameBuilder::new([]).unwrap();
    builder.catalog.maximum_vertices = builder.catalog.vertices.len() + 12;
    builder
        .replace_pack_geometries(vec![pack_geometry(0, 12), pack_geometry(1, 12)])
        .unwrap();
    let replacement = distinct(replaced, 20.0, 12);
    builder.insert_geometry(replacement.clone()).unwrap();
    assert_eq!(
        builder.insert_geometry(distinct(replaced, 30.0, 40)),
        Err(ActorRigGeometryError::CatalogCapacity)
    );
    let _ = builder.build(0.5, None, [draws(1, other)]);
    let frame = builder.build(0.5, None, [draws(2, replaced)]);
    assert_eq!(frame.instances.len(), 1);
    assert_eq!(builder.catalog.geometries[&replaced], replacement);
    assert_spans(&builder.catalog);
}

/// Repeated resident and cached-denial frames allocate nothing in staged admission.
#[test]
fn warm_staged_admission_reuses_drawn_storage() {
    for denied in [false, true] {
        let mut builder = ActorRigFrameBuilder::new([]).unwrap();
        builder.catalog.maximum_vertices = builder.catalog.vertices.len() + 12;
        builder
            .replace_pack_geometries(vec![
                pack_geometry(0, 6),
                pack_geometry(1, 6),
                pack_geometry(2, 40),
            ])
            .unwrap();
        let mut submissions = vec![
            draws(1, crate::pack_rig_id(1)),
            draws(2, crate::pack_rig_id(0)),
            draws(3, crate::pack_rig_id(1)),
        ];
        if denied {
            submissions.push(draws(4, crate::pack_rig_id(2)));
        }
        builder.admit_staged(submissions.iter(), 1);
        let before = test_alloc::thread_allocations();
        for generation in 2..12 {
            builder.admit_staged(submissions.iter(), generation);
        }
        assert_eq!(
            test_alloc::thread_allocations() - before,
            0,
            "denied {denied}"
        );
        assert!(builder.contains_geometry(crate::pack_rig_id(0)));
        assert!(builder.contains_geometry(crate::pack_rig_id(1)));
    }
}
