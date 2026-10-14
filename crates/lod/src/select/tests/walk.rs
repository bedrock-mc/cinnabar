//! The measure and walks: monotone refinement, hysteresis, the resident-column rule,
//! balance and unread children.

use super::*;
use crate::{
    geometry::{Face, MAX_LEVEL, SectionKey, section_blocks},
    select::{KeySet, projected_area, resident::ChunkRect},
};

/// A small deterministic generator for sample cameras and octants.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// A value in `-range..range`.
    fn signed(&mut self, range: f64) -> f64 {
        (self.next() as f64 / (1_u64 << 31) as f64 * 2.0 - 1.0) * range
    }
}

#[test]
fn projected_area_is_monotone_on_every_root_to_leaf_path() {
    let mut random = Lcg(7);
    for _ in 0..20_000 {
        let camera = [
            random.signed(5_000.0),
            random.signed(400.0),
            random.signed(5_000.0),
        ];
        let mut section = pos(MAX_LEVEL, (random.next() % 6) as i32 - 3, 0, -1);
        let mut area = projected_area(camera, FOCAL, section);
        while let Some(child) = section.child(Octant::ALL[(random.next() % 8) as usize]) {
            let child_area = projected_area(camera, FOCAL, child);
            assert!(
                child_area <= area,
                "{child:?} {child_area} > parent {section:?} {area} from {camera:?}"
            );
            // Parent bounds contain their children.
            let (parent_min, child_min) = (section.min_block(), child.min_block());
            let (parent_edge, child_edge) = (
                section_blocks(section.level()),
                section_blocks(child.level()),
            );
            for axis in 0..3 {
                assert!(parent_min[axis] <= child_min[axis]);
                assert!(child_min[axis] + child_edge <= parent_min[axis] + parent_edge);
            }
            section = child;
            area = child_area;
        }
    }
}

/// The camera X, at the root's height and Z centre, where the root's area equals `area`.
fn camera_x_at_area(root: SectionPos, area: f64) -> f64 {
    let edge = section_blocks(root.level()) as f64;
    let centre = [edge / 2.0, edge / 2.0];
    let at = |x: f64| projected_area([x, centre[0], centre[1]], FOCAL, root);
    let (mut near, mut far) = (edge * 2.0, edge * 1_000.0);
    for _ in 0..100 {
        let mid = (near + far) / 2.0;
        if at(mid) > area {
            near = mid;
        } else {
            far = mid;
        }
    }
    far
}

#[test]
fn sections_refine_above_the_threshold_and_coarsen_only_well_below_it() {
    let mut tree = flat_tree(0..1);
    let root = pos(ROOT_LEVEL, 0, 0, 0);
    let resident = ResidentColumns::default();
    let template = view([0.0; 3], 6_000.0);
    let refine_x = camera_x_at_area(root, template.refine_area());
    let coarsen_x = camera_x_at_area(root, template.coarsen_area());
    assert!(coarsen_x > refine_x);
    let at = |x: f64| view([x, 256.0, 256.0], 6_000.0);
    let root_selected = |selector: &mut Selector, x: f64, tree: &mut ExploredTree| {
        walk_known(selector, &at(x), tree, &resident)
            .selected
            .contains(&root)
    };
    let between = (refine_x + coarsen_x) / 2.0;
    let mut selector = Selector::default();
    assert!(root_selected(&mut selector, coarsen_x * 1.1, &mut tree));
    assert!(root_selected(&mut selector, between, &mut tree), "not yet");
    assert!(!root_selected(&mut selector, refine_x * 0.9, &mut tree));
    assert!(
        !root_selected(&mut selector, between, &mut tree),
        "a refined root stays refined until well below the threshold"
    );
    assert!(root_selected(&mut selector, coarsen_x * 1.1, &mut tree));
    selector.reset();
    assert!(!root_selected(&mut selector, refine_x * 0.9, &mut tree));
    selector.reset();
    assert!(
        root_selected(&mut selector, between, &mut tree),
        "a reset forgets refinement"
    );
}

/// True when no selected section has a selected ancestor.
fn disjoint(selected: &KeySet) -> bool {
    selected.iter().all(|key| {
        std::iter::successors(key.pos().parent(), |pos| pos.parent())
            .all(|ancestor| !selected.contains(&ancestor.key()))
    })
}

/// True when every selected section's face neighbours are at most one level coarser; as
/// it holds for every section, they are at most one level finer too.
fn balanced(selected: &KeySet) -> bool {
    selected.iter().all(|key| {
        let pos = key.pos();
        Face::ALL.into_iter().all(|face| {
            let Some(across) = pos.neighbour(face) else {
                return true;
            };
            (2..=MAX_LEVEL)
                .map_while(|up| across.ancestor(up))
                .all(|coarse| !selected.contains(&coarse.key()))
        })
    })
}

#[test]
fn resident_columns_force_level_zero_and_neighbours_balance_around_them() {
    let mut tree = flat_tree(0..4);
    // A 5×5-chunk patch of resident columns inside root (1, 1), off the level-0 grid, from
    // a camera so high above that the measure alone keeps every root whole.
    let patch: Vec<[i32; 2]> = (41..46)
        .flat_map(|x| (41..46).map(move |z| [x, z]))
        .collect();
    let resident = ResidentColumns::new(patch.iter().copied());
    let far = view([1024.0, 60_000.0, 1024.0], 4_000.0);
    let walk = walk_known(&mut Selector::default(), &far, &mut tree, &resident);
    let selected: KeySet = walk.selected.iter().map(|pos| pos.key()).collect();
    assert!(disjoint(&selected));
    assert!(balanced(&selected), "neighbours at most one level apart");
    assert!(walk.stats.balanced > 0, "the patch needed balancing");
    for section in &walk.selected {
        if resident.overlaps(*section) {
            assert_eq!(section.level(), 0, "{section:?} overlaps resident columns");
        }
        assert!(!resident.covers(*section), "{section:?} is all resident");
    }
    // Every flat level-0 section is drawn by exactly one selected section, or left to
    // resident chunks.
    for x in 0..64 {
        for z in 0..64 {
            let leaf = pos(0, x, 0, z);
            let covering = std::iter::successors(Some(leaf), |pos| pos.parent())
                .filter(|pos| selected.contains(&pos.key()))
                .count();
            let expected = usize::from(!resident.covers(leaf));
            assert_eq!(covering, expected, "{leaf:?}");
        }
    }
    // The patch's edge sections overlap resident columns without being covered.
    let edge = pos(0, 20, 0, 20);
    assert!(resident.overlaps(edge) && !resident.covers(edge));
    assert!(selected.contains(&edge.key()));
    assert_eq!(ChunkRect::of(edge).area(), 4);
}

#[test]
fn a_close_camera_refines_to_level_zero_balanced_and_far_roots_stay_coarse() {
    let mut tree = flat_tree(0..8);
    let near = view([100.0, 40.0, 100.0], 6_000.0);
    let walk = walk_known(
        &mut Selector::default(),
        &near,
        &mut tree,
        &ResidentColumns::default(),
    );
    let selected: KeySet = walk.selected.iter().map(|pos| pos.key()).collect();
    assert!(disjoint(&selected) && balanced(&selected));
    assert!(
        selected.contains(&pos(0, 3, 0, 3).key()),
        "level 0 at the camera"
    );
    assert!(
        selected.contains(&pos(ROOT_LEVEL, 7, 0, 7).key()),
        "the far corner stays whole"
    );
}

#[test]
fn unread_children_keep_their_parent_unless_it_overlaps_resident_columns() {
    let root = pos(ROOT_LEVEL, 0, 0, 0);
    let full = flat_tree(0..1);
    let mut tree = ExploredTree::default();
    tree.insert(root.key(), full.get(root).expect("stored root"));
    let close = view([256.0, 40.0, 256.0], 2_000.0);
    let mut walk = Walk::default();
    let mut selector = Selector::default();
    selector.walk(&close, &tree, &ResidentColumns::default(), &mut walk);
    assert!(
        walk.selected.contains(&root),
        "kept whole while children are read"
    );
    assert_eq!(walk.stats.waiting_for_children, 1);
    let children: Vec<SectionPos> = Octant::ALL
        .into_iter()
        .filter(|octant| octant.y() == 0)
        .map(|octant| root.child(octant).expect("child"))
        .collect();
    assert!(children.iter().all(|child| walk.unknown.contains(child)));

    let resident = ResidentColumns::new([[3, 3]]);
    selector.walk(&close, &tree, &resident, &mut walk);
    assert!(
        walk.selected.is_empty(),
        "a resident overlap never draws the root"
    );
    assert!(children.iter().all(|child| walk.unknown.contains(child)));

    // Reading the children lets the walk go deeper; positions above the dimension are
    // never asked for.
    for child in &children {
        tree.insert(child.key(), full.get(*child).expect("stored child"));
    }
    selector.walk(&close, &tree, &ResidentColumns::default(), &mut walk);
    assert!(!walk.selected.contains(&root));
    let top: SectionKey = pos(ROOT_LEVEL, 0, 1, 0).key();
    assert!(walk.unknown.iter().all(|pos| pos.key() != top));
}

#[test]
fn stale_sections_stay_selected_while_they_are_read_again() {
    let mut tree = flat_tree(0..1);
    let far = view([256.0, 6_000.0, 256.0], 600.0);
    let resident = ResidentColumns::default();
    let mut selector = Selector::default();
    let root = pos(ROOT_LEVEL, 0, 0, 0);
    assert!(
        walk_known(&mut selector, &far, &mut tree, &resident)
            .selected
            .contains(&root)
    );
    tree.mark_stale(root.key());
    let mut walk = Walk::default();
    selector.walk(&far, &tree, &resident, &mut walk);
    assert!(walk.selected.contains(&root), "still drawn");
    assert!(walk.unknown.contains(&root), "and read again");
    tree.insert(root.key(), flat_tree(0..1).get(root).expect("stored"));
    selector.walk(&far, &tree, &resident, &mut walk);
    assert!(!walk.unknown.contains(&root));
}
