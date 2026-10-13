use super::super::crack_shape_from_template;
use super::*;

#[test]
fn bamboo_cracks_follow_columns_and_reuse_unchanged_geometry() {
    let data = assets::pinned_block_registry_bytes();
    let protocol = assets::registry_header_protocol(data).unwrap();
    let records: Vec<_> = assets::read_registry_for_protocol(data, protocol)
        .unwrap()
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:air" | "minecraft:stone" | "minecraft:bamboo"
            )
        })
        .enumerate()
        .map(|(id, mut record)| {
            record.sequential_id = id as u32;
            record
        })
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir_all(root.join("textures/blocks")).unwrap();
    std::fs::write(
        root.join("blocks.json"),
        r#"{"stone":{"textures":"test"},"bamboo":{"textures":"test"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"test":{"textures":"textures/blocks/test"}}}"#,
    )
    .unwrap();
    std::fs::write(root.join("textures/flipbook_textures.json"), "[]").unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([127, 127, 127, 255]))
        .save(root.join("textures/blocks/test.png"))
        .unwrap();
    let lights = vec![assets::LightProperties::default(); records.len()];
    let compiled = pack_compiler::compile_pack(root, &records, &lights).unwrap();
    let assets =
        Arc::new(assets::RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap());
    let bamboo = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:bamboo")
        .unwrap()
        .sequential_id;
    let mut shapes = CrackShapeCache::default();
    let shape = |cache: &mut CrackShapeCache, position| {
        let CrackShape::Quads(quads) = crack_shape(
            cache,
            &assets,
            assets::NetworkIdMode::Sequential,
            Some(bamboo),
            position,
            |_| None,
        ) else {
            panic!("bamboo model surface");
        };
        quads
    };
    let origin = shape(&mut shapes, [0, 2, 0]);
    let another_column = shape(&mut shapes, [1, 2, 0]);
    assert_ne!(
        origin, another_column,
        "runtime ID alone must not reuse another column's offsets"
    );
    let same_column = shape(&mut shapes, [0, 7, 0]);
    assert!(
        std::sync::Arc::ptr_eq(&origin, &same_column),
        "same column reuses geometry without rebuilding"
    );
    assert_eq!(shapes.shapes.len(), 2);
    let stone = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:stone")
        .unwrap()
        .sequential_id;
    for position in [[0; 3], [100, 20, -100]] {
        assert_eq!(
            crack_shape(
                &mut shapes,
                &assets,
                assets::NetworkIdMode::Sequential,
                Some(stone),
                position,
                |_| None,
            ),
            CrackShape::Cube
        );
    }
    assert_eq!(
        shapes.shapes.len(),
        3,
        "position-independent shapes share one entry"
    );
}

#[test]
fn custom_compound_cracks_follow_each_admitted_column_displacement() {
    let quad = assets::ModelQuad {
        positions: [[0, 0, 0], [256, 0, 0], [256, 256, 0], [0, 256, 0]],
        uvs: [[0, 0]; 4],
        material: 0,
        flags: 0,
    };
    let overlay = assets::BlockOverlay {
        visuals: vec![assets::BlockVisual {
            native_material: assets::native_material::NativeMaterialFacts::Unresolved,
            faces: [0; 6],
            flags: assets::BlockFlags::empty(),
            kind: assets::VisualKind::Model,
            support: assets::VisualSupport::VanillaFallback,
            contributor_role: assets::ContributorRole::Primary,
            model_template: 0,
            animation: assets::NO_ANIMATION,
            variant: 0,
            destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
        }],
        light_properties: vec![assets::LightProperties::default()],
        materials: vec![assets::Material {
            texture: assets::TextureRef::new(1, 0).unwrap(),
            animation: assets::NO_ANIMATION,
            ..assets::Material::unvaried()
        }],
        model_templates: vec![
            assets::ModelTemplate {
                quad_start: 0,
                quad_count: 1,
                flags: assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT,
            },
            assets::ModelTemplate {
                quad_start: 1,
                quad_count: 1,
                flags: 0,
            },
        ],
        model_quads: vec![quad; 2],
        model_random_offsets: vec![(1, block_transform::random_offset::BAMBOO)],
        hashes: vec![Some(0xdead_beef)],
        textures: vec![assets::TextureArray {
            layers: 1,
            mips: assets::build_texture_mip_chain(vec![255; 16 * 16 * 4].into(), 16).unwrap(),
        }],
        ..Default::default()
    };
    let assets = Arc::new(
        assets::RuntimeAssets::diagnostic()
            .with_block_overlay(1, &overlay)
            .unwrap(),
    );
    for (mode, id) in [
        (assets::NetworkIdMode::Sequential, 1),
        (assets::NetworkIdMode::Hashed, 0xdead_beef),
    ] {
        let mut cache = CrackShapeCache::default();
        let CrackShape::Quads(first) =
            crack_shape(&mut cache, &assets, mode, Some(id), [0, 2, 0], |_| None)
        else {
            panic!("compound model");
        };
        let CrackShape::Quads(second) =
            crack_shape(&mut cache, &assets, mode, Some(id), [1, 2, 0], |_| None)
        else {
            panic!("compound model");
        };
        assert_ne!(
            first[1].corners, second[1].corners,
            "different columns cannot reuse the first component displacement"
        );
        let CrackShape::Quads(same_column) =
            crack_shape(&mut cache, &assets, mode, Some(id), [1, 7, 0], |_| None)
        else {
            panic!("compound model");
        };
        assert!(
            std::sync::Arc::ptr_eq(&second, &same_column),
            "height changes reuse the same column shape"
        );
        assert_eq!(
            cache.shapes.len(),
            1,
            "continuous column displacement keeps bounded cache entries per runtime transform"
        );
    }
}

/// Builds an admitted block overlay with one surface at the requested local height.
fn overlay_surface(height: i16) -> Arc<assets::RuntimeAssets> {
    let overlay = assets::BlockOverlay {
        visuals: vec![assets::BlockVisual {
            native_material: assets::native_material::NativeMaterialFacts::Unresolved,
            faces: [0; 6],
            flags: assets::BlockFlags::empty(),
            kind: assets::VisualKind::Model,
            support: assets::VisualSupport::VanillaFallback,
            contributor_role: assets::ContributorRole::Primary,
            model_template: 0,
            animation: assets::NO_ANIMATION,
            variant: 0,
            destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
        }],
        light_properties: vec![assets::LightProperties::default()],
        materials: vec![assets::Material {
            texture: assets::TextureRef::new(1, 0).unwrap(),
            animation: assets::NO_ANIMATION,
            ..assets::Material::unvaried()
        }],
        model_templates: vec![assets::ModelTemplate {
            quad_start: 0,
            quad_count: 1,
            flags: 0,
        }],
        model_quads: vec![assets::ModelQuad {
            positions: [
                [0, height, 0],
                [256, height, 0],
                [256, height, 256],
                [0, height, 256],
            ],
            uvs: [[0, 0]; 4],
            material: 0,
            flags: 0,
        }],
        hashes: vec![Some(0xdead_beef)],
        textures: vec![assets::TextureArray {
            layers: 1,
            mips: assets::build_texture_mip_chain(vec![255; 16 * 16 * 4].into(), 16).unwrap(),
        }],
        ..Default::default()
    };
    Arc::new(
        assets::RuntimeAssets::diagnostic()
            .with_block_overlay(1, &overlay)
            .unwrap(),
    )
}

#[test]
fn resource_reload_replaces_crack_surfaces_without_changing_block_identity() {
    let first_assets = overlay_surface(64);
    let second_assets = overlay_surface(192);
    for (mode, id) in [
        (assets::NetworkIdMode::Sequential, 1),
        (assets::NetworkIdMode::Hashed, 0xdead_beef),
    ] {
        let mut cache = CrackShapeCache::default();
        let first = crack_shape(&mut cache, &first_assets, mode, Some(id), [0; 3], |_| None);
        let repeated = crack_shape(&mut cache, &first_assets, mode, Some(id), [0; 3], |_| None);
        let (CrackShape::Quads(first_quads), CrackShape::Quads(repeated_quads)) =
            (&first, &repeated)
        else {
            panic!("admitted overlay surface");
        };
        assert!(Arc::ptr_eq(first_quads, repeated_quads));
        let replacement = crack_shape(&mut cache, &second_assets, mode, Some(id), [0; 3], |_| None);
        assert_ne!(first, replacement);
        assert_eq!(cache.shapes.len(), 1);
        assert_eq!(
            replacement,
            crack_shape(
                &mut CrackShapeCache::default(),
                &second_assets,
                mode,
                Some(id),
                [0; 3],
                |_| None,
            )
        );
    }
}

/// Compiles two registered door classes with original fixture artwork and both wire identities.
fn door_fixture() -> &'static (Arc<assets::RuntimeAssets>, Vec<assets::RegistryRecord>) {
    static FIXTURE: std::sync::OnceLock<(Arc<assets::RuntimeAssets>, Vec<assets::RegistryRecord>)> =
        std::sync::OnceLock::new();
    FIXTURE.get_or_init(|| {
        let data = assets::pinned_block_registry_bytes();
        let protocol = assets::registry_header_protocol(data).unwrap();
        let registry = assets::read_registry_for_protocol(data, protocol).unwrap();
        let names = registry
            .iter()
            .filter(|record| record.model_family == assets::ModelFamily::Door)
            .map(|record| record.name.as_ref())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .take(2)
            .collect::<Vec<_>>();
        let blocks = names
            .iter()
            .map(|name| {
                (
                    name.strip_prefix("minecraft:").unwrap().to_owned(),
                    serde_json::json!({"textures":"test"}),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        let records = registry
            .iter()
            .filter(|record| {
                record.flags.contains(assets::BlockFlags::AIR)
                    || names.contains(&record.name.as_ref())
            })
            .cloned()
            .enumerate()
            .map(|(id, mut record)| {
                record.sequential_id = id as u32;
                record
            })
            .collect::<Vec<_>>();
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("textures/blocks")).unwrap();
        std::fs::write(
            root.join("blocks.json"),
            serde_json::to_vec(&blocks).unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join("textures/terrain_texture.json"),
            r#"{"texture_data":{"test":{"textures":"textures/blocks/test"}}}"#,
        )
        .unwrap();
        std::fs::write(root.join("textures/flipbook_textures.json"), "[]").unwrap();
        image::RgbaImage::from_pixel(16, 16, image::Rgba([127, 127, 127, 255]))
            .save(root.join("textures/blocks/test.png"))
            .unwrap();
        let lights = vec![assets::LightProperties::default(); records.len()];
        let compiled = pack_compiler::compile_pack(root, &records, &lights).unwrap();
        (
            Arc::new(
                assets::RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap(),
            ),
            records,
        )
    })
}

/// Finds a registered door state without replacing any of its independent stored selectors.
fn door_record<'a>(
    records: &'a [assets::RegistryRecord],
    name: &str,
    pose: assets::door::DoorPose,
) -> &'a assets::RegistryRecord {
    use assets::ModelStateField;
    records
        .iter()
        .find(|record| {
            record.name.as_ref() == name
                && record.model_state.get(ModelStateField::Orientation) == Some(pose.direction)
                && record.model_state.get(ModelStateField::Open) == Some(u32::from(pose.open))
                && record.model_state.get(ModelStateField::Hinge)
                    == Some(u32::from(pose.hinge_right))
                && record.model_state.get(ModelStateField::Flags)
                    == Some(u32::from(pose.upper) << 7)
        })
        .unwrap()
}

/// Returns the admitted registry identity for the requested wire mode.
fn wire_id(mode: assets::NetworkIdMode, record: &assets::RegistryRecord) -> u32 {
    match mode {
        assets::NetworkIdMode::Sequential => record.sequential_id,
        assets::NetworkIdMode::Hashed => record.network_hash,
    }
}

#[test]
fn selection_and_cracks_follow_the_paired_door_plane_for_both_halves() {
    use assets::door::DoorPose;
    let (assets, records) = door_fixture();
    let names = records
        .iter()
        .filter(|record| record.model_family == assets::ModelFamily::Door)
        .map(|record| record.name.as_ref())
        .collect::<std::collections::BTreeSet<_>>();
    for name in names {
        for direction in 0..4 {
            for open in [false, true] {
                for hinge_right in [false, true] {
                    let lower = DoorPose {
                        direction,
                        open,
                        hinge_right: !hinge_right,
                        upper: false,
                    };
                    let upper = DoorPose {
                        direction: (direction + 1) & 3,
                        open: !open,
                        hinge_right,
                        upper: true,
                    };
                    for mode in [
                        assets::NetworkIdMode::Sequential,
                        assets::NetworkIdMode::Hashed,
                    ] {
                        let mut cache = CrackShapeCache::default();
                        for (pose, neighbor, block, other_block) in [
                            (lower, upper, [-1, 15, -17], [-1, 16, -17]),
                            (upper, lower, [-1, 16, -17], [-1, 15, -17]),
                        ] {
                            let id = wire_id(mode, door_record(records, name, pose));
                            let other_id = wire_id(mode, door_record(records, name, neighbor));
                            let expected_id = wire_id(
                                mode,
                                door_record(
                                    records,
                                    name,
                                    DoorPose {
                                        direction,
                                        open,
                                        hinge_right,
                                        upper: pose.upper,
                                    },
                                ),
                            );
                            let expected_visual = assets.resolve(mode, expected_id);
                            let expected = crack_shape_from_template(
                                assets,
                                expected_visual.model_template().unwrap(),
                                expected_visual.variant(),
                                block,
                            )
                            .unwrap();
                            let neighbor_at = |position| {
                                assert_eq!(position, other_block);
                                Some(other_id)
                            };
                            assert_eq!(
                                selection_shape(assets, mode, id, block, neighbor_at),
                                expected,
                                "selection {name}/{pose:?}/{mode:?}"
                            );
                            assert_eq!(
                                crack_shape(&mut cache, assets, mode, Some(id), block, neighbor_at),
                                expected,
                                "cracks {name}/{pose:?}/{mode:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn neighbor_only_changes_replace_cached_door_cracks() {
    use assets::door::DoorPose;
    let (assets, records) = door_fixture();
    let name = records
        .iter()
        .find(|record| record.model_family == assets::ModelFamily::Door)
        .unwrap()
        .name
        .as_ref();
    let lower = DoorPose {
        direction: 0,
        open: true,
        hinge_right: false,
        upper: false,
    };
    let upper = DoorPose {
        direction: 2,
        open: false,
        hinge_right: false,
        upper: true,
    };
    for mode in [
        assets::NetworkIdMode::Sequential,
        assets::NetworkIdMode::Hashed,
    ] {
        for (pose, neighbor, changed_neighbor) in [
            (
                lower,
                upper,
                DoorPose {
                    hinge_right: true,
                    ..upper
                },
            ),
            (
                upper,
                lower,
                DoorPose {
                    direction: 2,
                    open: false,
                    ..lower
                },
            ),
        ] {
            let id = wire_id(mode, door_record(records, name, pose));
            let other_id = wire_id(mode, door_record(records, name, neighbor));
            let changed_id = wire_id(mode, door_record(records, name, changed_neighbor));
            let mut cache = CrackShapeCache::default();
            let first = crack_shape(&mut cache, assets, mode, Some(id), [0; 3], |_| {
                Some(other_id)
            });
            let changed = crack_shape(&mut cache, assets, mode, Some(id), [0; 3], |_| {
                Some(changed_id)
            });
            assert_ne!(first, changed, "only the neighbor's ID changed");
            assert_eq!(
                changed,
                selection_shape(assets, mode, id, [0; 3], |_| Some(changed_id))
            );
            let repeated = crack_shape(&mut cache, assets, mode, Some(id), [20, 20, 20], |_| {
                Some(changed_id)
            });
            let (CrackShape::Quads(changed), CrackShape::Quads(repeated)) = (&changed, repeated)
            else {
                panic!("door surfaces");
            };
            assert!(
                Arc::ptr_eq(changed, &repeated),
                "unchanged poses reuse geometry"
            );
            let missing = crack_shape(&mut cache, assets, mode, Some(id), [0; 3], |_| None);
            assert_eq!(missing, selection_shape(assets, mode, id, [0; 3], |_| None));
            assert_ne!(
                first, missing,
                "unloaded partners cannot retain the previous plane"
            );
            let other_class = records
                .iter()
                .find(|record| {
                    record.model_family == assets::ModelFamily::Door && record.name.as_ref() != name
                })
                .unwrap();
            assert_eq!(
                crack_shape(&mut cache, assets, mode, Some(id), [0; 3], |_| Some(
                    wire_id(mode, other_class)
                )),
                missing,
                "different door classes do not form a pair"
            );
            let restored = crack_shape(&mut cache, assets, mode, Some(id), [0; 3], |_| {
                Some(other_id)
            });
            assert_eq!(
                restored, first,
                "reloading the partner restores the original plane"
            );
            assert_eq!(
                cache.shapes.len(),
                3,
                "cache entries track poses instead of world positions"
            );
        }
    }
}

#[test]
fn door_crack_cache_tracks_neighbor_pose_without_changing_runtime_id() {
    let data = assets::pinned_block_registry_bytes();
    let protocol = assets::registry_header_protocol(data).unwrap();
    let records = assets::read_registry_for_protocol(data, protocol)
        .unwrap()
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:air" | "minecraft:warped_door"
            )
        })
        .enumerate()
        .map(|(index, mut record)| {
            record.sequential_id = index as u32;
            record
        })
        .collect::<Vec<_>>();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir_all(root.join("textures/blocks")).unwrap();
    std::fs::write(
        root.join("blocks.json"),
        r#"{"warped_door":{"textures":"test"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"test":{"textures":"textures/blocks/test"}}}"#,
    )
    .unwrap();
    std::fs::write(root.join("textures/flipbook_textures.json"), "[]").unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([127, 127, 127, 255]))
        .save(root.join("textures/blocks/test.png"))
        .unwrap();
    let lights = vec![assets::LightProperties::default(); records.len()];
    let compiled = pack_compiler::compile_pack(root, &records, &lights).unwrap();
    let world =
        Arc::new(assets::RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap());
    let state = |upper: bool, hinge: u32| {
        records
            .iter()
            .find(|record| {
                record.name.as_ref() == "minecraft:warped_door"
                    && record.model_state.get(assets::ModelStateField::Flags)
                        == Some(u32::from(upper) << 7)
                    && record.model_state.get(assets::ModelStateField::Orientation)
                        == Some(if upper { 3 } else { 0 })
                    && record.model_state.get(assets::ModelStateField::Open)
                        == Some(u32::from(!upper))
                    && record.model_state.get(assets::ModelStateField::Hinge) == Some(hinge)
            })
            .unwrap()
    };
    for mode in [
        assets::NetworkIdMode::Sequential,
        assets::NetworkIdMode::Hashed,
    ] {
        let id = |record: &assets::RegistryRecord| {
            if mode == assets::NetworkIdMode::Sequential {
                record.sequential_id
            } else {
                record.network_hash
            }
        };
        let lower = id(state(false, 0));
        let block = [0, 15, 0];
        let mut cache = CrackShapeCache::default();
        let mut first = None;
        let mut previous = None;
        for hinge in [1, 0, 1] {
            let neighbor = id(state(true, hinge));
            let template = world
                .model_template_at(mode, lower, block, |position| {
                    assert_eq!(position, [0, 16, 0]);
                    Some(neighbor)
                })
                .unwrap();
            let expected = crack_shape_from_template(&world, template, 0, block).unwrap();
            assert_eq!(
                crate::crack_shape_for_block(&world, mode, lower, block, |_| Some(neighbor)),
                Some(expected.clone()),
                "selection surfaces use the paired plane too"
            );
            let actual = cache.shape(&world, mode, Some(lower), block, Some(template));
            assert_eq!(
                actual, expected,
                "neighbor hinge must select the visible door plane"
            );
            if let Some(previous) = previous {
                assert_ne!(actual, previous);
            }
            if first.is_none() {
                first = Some(actual.clone());
            }
            previous = Some(actual);
        }
        let (CrackShape::Quads(first), Some(CrackShape::Quads(last))) = (first.unwrap(), previous)
        else {
            panic!("door surface");
        };
        assert!(
            Arc::ptr_eq(&first, &last),
            "returning to an unchanged pose reuses its surfaces"
        );
        assert_eq!(
            cache.shapes.len(),
            2,
            "cache stays bounded by poses rather than positions"
        );
        let CrackShape::Quads(empty) = cache.shape(&world, mode, Some(lower), block, None) else {
            panic!("unpaired door has no terrain surfaces");
        };
        assert!(empty.is_empty());
        assert_eq!(
            crate::crack_shape_for_block(&world, mode, lower, block, |_| None),
            Some(CrackShape::Quads(empty))
        );
    }
}

/// Resolves test cache surfaces through the same current-world asset contract.
fn crack_shape(
    cache: &mut CrackShapeCache,
    assets: &Arc<assets::RuntimeAssets>,
    mode: assets::NetworkIdMode,
    runtime_id: Option<u32>,
    block: [i32; 3],
    primary_at: impl FnOnce([i32; 3]) -> Option<u32>,
) -> CrackShape {
    let template = runtime_id.and_then(|id| assets.model_template_at(mode, id, block, primary_at));
    cache.shape(assets, mode, runtime_id, block, template)
}

/// Builds the model surfaces used by the selection consumer.
fn selection_shape(
    assets: &assets::RuntimeAssets,
    mode: assets::NetworkIdMode,
    runtime_id: u32,
    block: [i32; 3],
    primary_at: impl FnMut([i32; 3]) -> Option<u32>,
) -> CrackShape {
    crate::crack_shape_for_block(assets, mode, runtime_id, block, primary_at).unwrap_or_default()
}
