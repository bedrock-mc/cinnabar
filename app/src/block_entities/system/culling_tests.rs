use render_model::{ChestModel, ChestPair, ChestVariant, Facing};

use super::*;

const GENERATION: u64 = 7;

/// A section key in the fixture dimension.
fn key(x: i32, y: i32, z: i32) -> SubChunkKey {
    SubChunkKey::new(0, x, y, z)
}

/// A current cave result for a camera in section (0, 4, 0) that reached only that section.
fn cave() -> CaveVisibilityCache {
    let mut cave = CaveVisibilityCache::default();
    cave.camera = Some(key(0, 4, 0));
    cave.graph_generation = Some(GENERATION);
    cave.visible = [key(0, 4, 0)].into_iter().collect();
    cave.initialized = true;
    cave
}

/// Bounded templates that exercise the shared admission path.
fn templates() -> [Template; 4] {
    [
        Template::Chest(ChestModel {
            variant: ChestVariant::Normal,
            facing: Facing::North,
            pair: ChestPair::Single,
            lid: 0.0,
        }),
        Template::EnchantTable,
        Template::Conduit {
            active: true,
            hunting: false,
        },
        Template::Static(render_model::BlockEntityKind::EndPortal),
    ]
}

/// Counts every fixture template at `block` through `culling`.
fn admit_all(
    culling: Option<&CaveCulling<'_, impl Fn(SubChunkKey) -> bool>>,
    clip_from_world: Option<Mat4>,
    block: [i32; 3],
) -> BlockEntityWork {
    let mut work = BlockEntityWork::default();
    for template in templates() {
        work.admit(culling, clip_from_world, &template, block);
    }
    work
}

/// Entities in sections the current cave graph never reached cost no model or text preparation.
#[test]
fn an_offscreen_set_prepares_none_of_its_candidates() {
    let cave = cave();
    let culling = CaveCulling::new(&cave, key(0, 4, 0), GENERATION, |_: SubChunkKey| true);
    assert_eq!(
        admit_all(Some(&culling), None, [56, 70, 56]),
        BlockEntityWork {
            candidates: 4,
            prepared: 0,
        }
    );
    // Within reach of the reached section, a model may draw into it.
    assert_eq!(admit_all(Some(&culling), None, [17, 70, 8]).prepared, 4);
    assert_eq!(admit_all(Some(&culling), None, [8, 81, 8]).prepared, 4);
}

/// Without a result for this camera section and graph, or with unloaded sections in reach,
/// every candidate is prepared.
#[test]
fn stale_or_unknown_results_prepare_every_candidate() {
    let cave = cave();
    let hidden = [56, 70, 56];
    let known = |_: SubChunkKey| true;
    let moved = CaveCulling::new(&cave, key(1, 4, 0), GENERATION, known);
    let regraphed = CaveCulling::new(&cave, key(0, 4, 0), GENERATION + 1, known);
    let unloaded = CaveCulling::new(&cave, key(0, 4, 0), GENERATION, |section: SubChunkKey| {
        section != key(3, 4, 3)
    });
    for culling in [&moved, &regraphed] {
        assert_eq!(admit_all(Some(culling), None, hidden).prepared, 4);
    }
    assert_eq!(admit_all(Some(&unloaded), None, hidden).prepared, 4);
    let unpublished = CaveVisibilityCache::default();
    let culling = CaveCulling::new(&unpublished, key(0, 4, 0), GENERATION, known);
    assert_eq!(admit_all(Some(&culling), None, hidden).prepared, 4);
    assert_eq!(
        admit_all(
            None::<&CaveCulling<'_, fn(SubChunkKey) -> bool>>,
            None,
            hidden
        )
        .prepared,
        4
    );
}

/// A beacon's beam crosses sections, so its own section never hides it.
#[test]
fn beacons_are_prepared_from_hidden_sections() {
    let cave = cave();
    let culling = CaveCulling::new(&cave, key(0, 4, 0), GENERATION, |_: SubChunkKey| true);
    let mut work = BlockEntityWork::default();
    assert!(work.admit(Some(&culling), None, &Template::Beacon, [56, 70, 56]));
    assert_eq!(
        work,
        BlockEntityWork {
            candidates: 1,
            prepared: 1,
        }
    );
}

/// Installed skull geometry can extend beyond the authored block reach.
#[test]
fn unbounded_pack_geometry_is_prepared_even_in_hidden_sections() {
    let cave = cave();
    let culling = CaveCulling::new(&cave, key(0, 4, 0), GENERATION, |_: SubChunkKey| true);
    let template = Template::Static(render_model::BlockEntityKind::Skull(
        render_model::SkullModel {
            kind: render_model::SkullKind::Dragon,
            mount: render_model::SkullMount::Floor {
                rotation_degrees: 0.0,
            },
        },
    ));
    let mut work = BlockEntityWork::default();
    assert!(work.admit(Some(&culling), None, &template, [56, 70, 56]));
    assert_eq!(work.prepared, 1);
}

/// Counts the fixture templates with only camera visibility deciding admission.
fn view_work(matrix: Option<Mat4>, block: [i32; 3]) -> BlockEntityWork {
    admit_all(
        None::<&CaveCulling<'_, fn(SubChunkKey) -> bool>>,
        matrix,
        block,
    )
}

/// The camera looks down negative Z with a reverse-depth near plane.
fn projection() -> Mat4 {
    glam::camera::rh::proj::directx::perspective_infinite_reverse(
        std::f32::consts::FRAC_PI_2,
        1.0,
        0.1,
    )
}

#[test]
fn behind_camera_candidates_skip_preparation_and_turning_admits_them() {
    let matrix = projection();
    let block = [0, 0, 20];
    assert_eq!(
        view_work(Some(matrix), block),
        BlockEntityWork {
            candidates: templates().len() as u32,
            prepared: 0,
        }
    );
    let turned = matrix * Mat4::from_rotation_y(std::f32::consts::PI);
    assert_eq!(view_work(Some(turned), block).prepared, 4);
    assert_eq!(view_work(Some(matrix), block).prepared, 0);
}

#[test]
fn intersecting_bounds_and_near_plane_crossings_remain_admitted() {
    let reach = BLOCK_ENTITY_REACH_BLOCKS;
    // The origin is behind the eye, but its bound crosses the near plane.
    for block in [[0, 0, 0], [0, 0, reach - 1], [0, 0, -reach]] {
        assert_eq!(view_work(Some(projection()), block).prepared, 4);
    }
    // At z=-20 the right view plane is x=20: the origin can be outside it.
    assert_eq!(view_work(Some(projection()), [21, 0, -20]).prepared, 4);
    assert_eq!(view_work(Some(projection()), [30, 0, -20]).prepared, 0);
}

#[test]
fn missing_or_invalid_camera_transforms_fail_open() {
    let block = [0, 0, 20];
    assert_eq!(view_work(None, block).prepared, 4);
    for matrix in [
        Mat4::ZERO,
        Mat4::from_cols_array(&[f32::NAN; 16]),
        Mat4::from_cols_array(&[f32::INFINITY; 16]),
    ] {
        assert_eq!(view_work(Some(matrix), block).prepared, 4);
    }
}

#[test]
fn extended_effects_and_unknown_item_bounds_fail_open_behind_the_camera() {
    use super::super::super::describe::{HeldItem, PlantBlock};
    use render_model::{BlockEntityKind, CrystalBeamModel, DragonDeathModel, ItemFrameModel};
    use std::sync::Arc;

    let item = HeldItem {
        identifier: Arc::from("minecraft:apple"),
        metadata: 0,
    };
    let templates = [
        Template::Beacon,
        Template::Static(BlockEntityKind::CrystalBeam(CrystalBeamModel {
            target: [0.0, 0.0, -20.0],
            crystal: [0.0, 0.0, 20.0],
            age_ticks: 1.0,
        })),
        Template::Static(BlockEntityKind::DragonDeath(DragonDeathModel {
            center: [0.0, 0.0, 20.0],
            death_ticks: 60,
            partial_tick: 0.5,
            seed: 17,
            duration_ticks: 200.0,
        })),
        Template::FlowerPot {
            plant: PlantBlock {
                identifier: Arc::from("minecraft:dandelion"),
                states: serde_json::Map::new(),
            },
        },
        Template::ItemFrame {
            model: ItemFrameModel {
                glow: false,
                outward: 2,
                full_face: false,
                map: None,
            },
            item: Some(item.clone()),
            rotation_degrees: 0.0,
            map_id: None,
        },
        Template::Campfire {
            yaw_degrees: 0.0,
            items: [Some(item), None, None, None],
        },
    ];
    let mut work = BlockEntityWork::default();
    for template in &templates {
        assert!(bounds(template, [0, 0, 20]).is_none());
        assert!(work.admit(
            None::<&CaveCulling<'_, fn(SubChunkKey) -> bool>>,
            Some(projection()),
            template,
            [0, 0, 20],
        ));
    }
    assert_eq!(work.prepared, templates.len() as u32);
}
