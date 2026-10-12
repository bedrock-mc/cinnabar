use render_model::{ChestModel, ChestPair, ChestVariant, Facing};

use super::*;

const GENERATION: u64 = 7;

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
    block: [i32; 3],
) -> BlockEntityWork {
    let mut work = BlockEntityWork::default();
    for template in templates() {
        work.admit(culling, &template, block);
    }
    work
}

/// Entities in sections the current cave graph never reached cost no model or text preparation.
#[test]
fn an_offscreen_set_prepares_none_of_its_candidates() {
    let cave = cave();
    let culling = CaveCulling::new(&cave, key(0, 4, 0), GENERATION, |_: SubChunkKey| true);
    assert_eq!(
        admit_all(Some(&culling), [56, 70, 56]),
        BlockEntityWork {
            candidates: 4,
            prepared: 0,
        }
    );
    // Within reach of the reached section, a model may draw into it.
    assert_eq!(admit_all(Some(&culling), [17, 70, 8]).prepared, 4);
    assert_eq!(admit_all(Some(&culling), [8, 81, 8]).prepared, 4);
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
        assert_eq!(admit_all(Some(culling), hidden).prepared, 4);
    }
    assert_eq!(admit_all(Some(&unloaded), hidden).prepared, 4);
    let unpublished = CaveVisibilityCache::default();
    let culling = CaveCulling::new(&unpublished, key(0, 4, 0), GENERATION, known);
    assert_eq!(admit_all(Some(&culling), hidden).prepared, 4);
    assert_eq!(
        admit_all(None::<&CaveCulling<'_, fn(SubChunkKey) -> bool>>, hidden).prepared,
        4
    );
}

/// A beacon's beam crosses sections, so its own section never hides it.
#[test]
fn beacons_are_prepared_from_hidden_sections() {
    let cave = cave();
    let culling = CaveCulling::new(&cave, key(0, 4, 0), GENERATION, |_: SubChunkKey| true);
    let mut work = BlockEntityWork::default();
    assert!(work.admit(Some(&culling), &Template::Beacon, [56, 70, 56]));
    assert_eq!(
        work,
        BlockEntityWork {
            candidates: 1,
            prepared: 1,
        }
    );
}
