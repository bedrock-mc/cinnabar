//! Selection tests: the measure, walks over synthetic explored trees, and the swap from
//! one selection to the next.

mod shown;
mod walk;

use crate::{
    geometry::{Octant, SectionPos},
    section::{ChildMask, ChildState},
    select::{
        ROOT_LEVEL, View,
        resident::ResidentColumns,
        tree::{ExploredTree, Node},
        walk::{Selector, Walk},
    },
};

#[test]
fn each_finer_quality_halves_the_edge_threshold() {
    use render_api::DistantTerrainQuality;
    for pair in DistantTerrainQuality::ALL.windows(2) {
        assert_eq!(
            super::edge_threshold_px(pair[1]) * 2.0,
            super::edge_threshold_px(pair[0]),
            "{pair:?}"
        );
    }
    assert_eq!(
        view([0.0; 3], 0.0).threshold_px,
        super::edge_threshold_px(DistantTerrainQuality::default()),
        "the fixtures select at the default quality"
    );
}

/// Pixels per unit of `tan(angle)` of a 1080-pixel-tall view with a 70° vertical field of
/// view.
const FOCAL: f64 = 771.0;

fn pos(level: u8, x: i32, y: i32, z: i32) -> SectionPos {
    SectionPos::new(level, x, y, z).expect("position")
}

/// A view from `camera` at medium quality reaching `distance` blocks.
fn view(camera: [f64; 3], distance: f64) -> View {
    View {
        camera,
        focal_px: FOCAL,
        threshold_px: super::edge_threshold_px(render_api::DistantTerrainQuality::Medium),
        distance_blocks: distance,
        min_block_y: -64,
        max_block_y: 320,
    }
}

/// A tree of flat terrain filling section Y 0 at every level under the root sections at
/// `x` and `z` in `roots`: every section there is stored, with its four lower children
/// non-empty and its upper ones empty.
fn flat_tree(roots: std::ops::Range<i32>) -> ExploredTree {
    let mut tree = ExploredTree::default();
    let mut stack: Vec<SectionPos> = roots
        .clone()
        .flat_map(|x| roots.clone().map(move |z| pos(ROOT_LEVEL, x, 0, z)))
        .collect();
    while let Some(section) = stack.pop() {
        if section.level() == 0 {
            tree.insert(section.key(), Node::Present(ChildMask::UNKNOWN));
            continue;
        }
        let mut mask = ChildMask::UNKNOWN;
        for octant in Octant::ALL {
            let state = if octant.y() == 0 {
                stack.push(section.child(octant).expect("child"));
                ChildState::NonEmpty
            } else {
                ChildState::Empty
            };
            mask.set(octant, state);
        }
        tree.insert(section.key(), Node::Present(mask));
    }
    tree
}

/// Walks until every position the walk asks for is known, marking positions `tree` does
/// not hold missing.
fn walk_known(
    selector: &mut Selector,
    view: &View,
    tree: &mut ExploredTree,
    resident: &ResidentColumns,
) -> Walk {
    let mut walk = Walk::default();
    for _ in 0..8 {
        selector.walk(view, tree, resident, &mut walk);
        if walk.unknown.is_empty() {
            return walk;
        }
        for unknown in &walk.unknown {
            tree.insert(unknown.key(), Node::Missing);
        }
    }
    panic!("the walk kept asking for unknown sections");
}
