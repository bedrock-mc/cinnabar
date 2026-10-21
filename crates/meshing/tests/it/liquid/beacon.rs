use super::*;

/// Resolves the active beacon identity without coupling the fixture to registry numbering.
pub(super) fn record() -> &'static assets::RegistryRecord {
    static RECORD: OnceLock<assets::RegistryRecord> = OnceLock::new();
    RECORD.get_or_init(|| {
        assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap()
        .into_iter()
        .find(|record| record.name.as_ref() == assets::facts::pack::PackBlock::Beacon.identifier())
        .expect("active beacon identity")
    })
}

#[test]
fn beacon_liquid_barrier_is_independent_of_visible_model_and_face_occlusion() {
    let beacon = record();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let beacon_id = match mode {
            NetworkIdMode::Sequential => beacon.sequential_id,
            NetworkIdMode::Hashed => beacon.network_hash,
        };
        let resolved = runtime_assets().resolve(mode, beacon_id);
        assert_eq!(resolved.kind(), VisualKind::Model);
        assert!(resolved.flags().is_empty());
        assert_eq!(
            runtime_assets().model_templates()[resolved.model_template().unwrap() as usize].flags,
            0
        );
        // Both identities use the same open model; only the admitted liquid fact differs.
        assert_eq!(resolved, runtime_assets().resolve(mode, UNCLASSIFIED_MODEL));
        for below in [false, true] {
            for (neighbour, barrier) in [(beacon_id, true), (UNCLASSIFIED_MODEL, false)] {
                let mut placements = Vec::new();
                for x in 7..=9 {
                    for z in 7..=9 {
                        placements.push((
                            if [x, z] == [9, 8] {
                                neighbour
                            } else {
                                WATER_SOURCE
                            },
                            [x, 8, z],
                        ));
                    }
                }
                if below {
                    placements.push((WATER_SOURCE, [9, 7, 8]));
                }
                let center = blocks(&placements);
                let mesh = mesh_sub_chunk_in_neighbourhood(
                    &BlockClassifier::new(AIR),
                    runtime_assets(),
                    mode,
                    &MeshNeighbourhood::new(&center),
                );
                let top = quad_at(&mesh, [8, 8, 8], Face::PositiveY);
                let source_height = LiquidLevel::from_variant(0).unwrap().height();
                if barrier {
                    assert_eq!(
                        top.heights(),
                        [source_height; 4],
                        "mode={mode:?} below={below}"
                    );
                    assert_eq!(top.flow_gradient(), [0, 0], "mode={mode:?} below={below}");
                    assert_eq!(top.material_id(), STILL);
                } else {
                    assert!(top.heights().iter().any(|&height| height < source_height));
                    if below {
                        assert_ne!(top.flow_gradient(), [0, 0]);
                    }
                }
                // A liquid barrier does not suppress the transparent contact face.
                quad_at(&mesh, [8, 8, 8], Face::PositiveX);
            }
        }
    }
}
