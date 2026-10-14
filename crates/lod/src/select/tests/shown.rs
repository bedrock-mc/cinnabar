//! The shown set: swaps without holes, timeouts, neighbour levels, the byte budget and
//! which meshes are wanted.

use std::sync::Arc;

use super::*;
use crate::{
    draw::NeighbourLevel,
    fixtures::{blocks, fixture_classifier},
    geometry::{Face, SectionKey},
    mesh::{
        ExteriorCulling, class::MeshClasses, input::MeshInput, mesher::Mesher, output::SectionMesh,
    },
    section::Section,
    select::shown::{DrawUpdate, SWAP_TIMEOUT_STEPS, ShownSet},
    storage::{DenseSection, PackScratch},
    voxel::{FULL_COVERAGE, Light, MaterialId, Voxel},
};

const CAMERA: [f64; 3] = [0.0, 0.0, 0.0];
const BUDGET: usize = usize::MAX;

/// The mesh of a solid stone section at `pos` with no known neighbours.
fn mesh(pos: SectionPos) -> Arc<SectionMesh> {
    let stone = Voxel::new(
        MaterialId::new(blocks::STONE).expect("id"),
        Light::FULL_SKY,
        FULL_COVERAGE,
        false,
    );
    let section = Section::from_dense(
        pos,
        &DenseSection::filled(stone, 1),
        ChildMask::UNKNOWN,
        &mut PackScratch::default(),
    )
    .expect("section");
    let classes = MeshClasses::new(&fixture_classifier(), blocks::SAND + 1);
    let input = MeshInput::new(&section, Default::default());
    Arc::new(Mesher::new().mesh(&input, &classes, ExteriorCulling::Off))
}

/// The four lower children of `parent`.
fn lower_children(parent: SectionPos) -> Vec<SectionPos> {
    Octant::ALL
        .into_iter()
        .filter(|octant| octant.y() == 0)
        .map(|octant| parent.child(octant).expect("child"))
        .collect()
}

fn keys(sections: &[SectionPos]) -> Vec<SectionKey> {
    let mut keys: Vec<SectionKey> = sections.iter().map(|pos| pos.key()).collect();
    keys.sort_unstable();
    keys
}

fn upserted(update: &DrawUpdate) -> Vec<SectionKey> {
    let mut keys: Vec<SectionKey> = update.upserts.iter().map(|s| s.pos.key()).collect();
    keys.sort_unstable();
    keys
}

fn shown_keys(update: &DrawUpdate) -> Vec<SectionKey> {
    update.shown.iter().map(|section| section.key).collect()
}

fn interest(set: &mut ShownSet) -> (Vec<SectionKey>, Vec<SectionKey>) {
    let (mut want, mut release) = (Vec::new(), Vec::new());
    set.take_interest(&mut want, &mut release);
    (keys(&want), release)
}

/// Shows `sections` from an empty set, every mesh arriving.
fn showing(sections: &[SectionPos]) -> ShownSet {
    let mut set = ShownSet::default();
    set.set_target(sections, &ResidentColumns::default());
    for &section in sections {
        set.mesh_arrived(section.key(), Some(mesh(section)));
    }
    set.update(CAMERA, BUDGET).expect("shown");
    interest(&mut set);
    set
}

#[test]
fn a_refinement_swaps_once_every_child_result_arrived() {
    let parent = pos(1, 0, 0, 0);
    let children = lower_children(parent);
    let mut set = showing(&[parent]);

    set.set_target(&children, &ResidentColumns::default());
    assert_eq!(interest(&mut set), (keys(&children), Vec::new()));
    for &child in &children[..3] {
        set.mesh_arrived(child.key(), Some(mesh(child)));
    }
    assert!(
        set.update(CAMERA, BUDGET).is_none(),
        "the parent stays until all arrive"
    );
    // The last child has no record: ready, drawing nothing.
    set.mesh_arrived(children[3].key(), None);
    let update = set.update(CAMERA, BUDGET).expect("swapped");
    assert_eq!(update.removed, vec![parent.key()]);
    assert_eq!(upserted(&update), keys(&children[..3]));
    assert_eq!(shown_keys(&update), keys(&children[..3]));
    assert_eq!(interest(&mut set), (Vec::new(), vec![parent.key()]));
}

#[test]
fn a_coarsening_swaps_once_the_parent_mesh_arrived() {
    let parent = pos(1, 0, 0, 0);
    let children = lower_children(parent);
    let mut set = showing(&children);
    set.set_target(&[parent], &ResidentColumns::default());
    assert!(set.update(CAMERA, BUDGET).is_none());
    set.mesh_arrived(parent.key(), Some(mesh(parent)));
    let update = set.update(CAMERA, BUDGET).expect("swapped");
    assert_eq!(update.removed, keys(&children));
    assert_eq!(upserted(&update), vec![parent.key()]);
    assert_eq!(interest(&mut set), (vec![parent.key()], keys(&children)));
}

#[test]
fn a_swap_waiting_too_long_applies_with_what_is_ready() {
    let parent = pos(1, 0, 0, 0);
    let children = lower_children(parent);
    let mut set = showing(&[parent]);
    set.set_target(&children, &ResidentColumns::default());
    set.mesh_arrived(children[0].key(), Some(mesh(children[0])));
    for _ in 0..SWAP_TIMEOUT_STEPS {
        assert!(set.update(CAMERA, BUDGET).is_none());
    }
    let update = set.update(CAMERA, BUDGET).expect("timed out");
    assert_eq!(update.removed, vec![parent.key()]);
    assert_eq!(upserted(&update), vec![children[0].key()]);
    assert_eq!(set.stats().timed_out, 1);
    // Late children appear on their own, as nothing shown overlaps them.
    set.mesh_arrived(children[1].key(), Some(mesh(children[1])));
    let late = set.update(CAMERA, BUDGET).expect("late child");
    assert_eq!(upserted(&late), vec![children[1].key()]);
}

#[test]
fn a_section_over_resident_columns_goes_at_once_and_its_children_as_they_arrive() {
    let parent = pos(1, 0, 0, 0);
    let children = lower_children(parent);
    let mut set = showing(&[parent]);
    // A chunk column under the parent became resident, so the walk refined it.
    set.set_target(&children, &ResidentColumns::new([[3, 3]]));
    let update = set.update(CAMERA, BUDGET).expect("removed at once");
    assert_eq!(update.removed, vec![parent.key()]);
    assert!(update.upserts.is_empty(), "no child arrived yet");
    assert_eq!(set.stats().resident_swaps, 1);
    assert_eq!(set.stats().timed_out, 0);
    set.mesh_arrived(children[1].key(), Some(mesh(children[1])));
    let late = set.update(CAMERA, BUDGET).expect("a child as it arrives");
    assert_eq!(upserted(&late), vec![children[1].key()]);

    // Away from resident columns a refinement still waits for every child.
    let far = pos(1, 40, 0, 40);
    let mut set = showing(&[far]);
    set.set_target(&lower_children(far), &ResidentColumns::new([[3, 3]]));
    assert!(set.update(CAMERA, BUDGET).is_none());
}

#[test]
fn neighbour_levels_follow_what_is_drawn_and_relink_on_change() {
    let coarse = pos(1, 1, 0, 0);
    let fine = lower_children(pos(1, 0, 0, 0));
    let mut sections = fine.clone();
    sections.push(coarse);
    let mut set = ShownSet::default();
    set.set_target(&sections, &ResidentColumns::default());
    for &section in &sections {
        set.mesh_arrived(section.key(), Some(mesh(section)));
    }
    let update = set.update(CAMERA, BUDGET).expect("shown");
    let levels = |update: &DrawUpdate, pos: SectionPos| {
        update
            .shown
            .iter()
            .find(|section| section.key == pos.key())
            .expect("shown")
            .neighbours
    };
    assert_eq!(
        levels(&update, coarse).get(Face::NegX),
        NeighbourLevel::Finer
    );
    assert_eq!(
        levels(&update, coarse).get(Face::PosX),
        NeighbourLevel::None
    );
    let edge = pos(0, 1, 0, 0);
    assert_eq!(
        levels(&update, edge).get(Face::PosX),
        NeighbourLevel::Coarser
    );
    assert_eq!(levels(&update, edge).get(Face::NegX), NeighbourLevel::Same);
    assert_eq!(
        levels(&update, pos(0, 0, 0, 0)).get(Face::NegX),
        NeighbourLevel::None
    );
    let upserted_edge = update
        .upserts
        .iter()
        .find(|section| section.pos == edge)
        .expect("upserted");
    assert_eq!(upserted_edge.neighbours, levels(&update, edge));

    // A new mesh of a drawn section is upserted with its levels, not relinked.
    set.mesh_arrived(coarse.key(), Some(mesh(coarse)));
    let remeshed = set.update(CAMERA, BUDGET).expect("remeshed");
    assert_eq!(upserted(&remeshed), vec![coarse.key()]);
    assert!(remeshed.relinked.is_empty());

    // Dropping the coarse section relinks its finer neighbours.
    set.set_target(&fine, &ResidentColumns::default());
    let dropped = set.update(CAMERA, BUDGET).expect("dropped");
    assert_eq!(dropped.removed, vec![coarse.key()]);
    let relinked: Vec<SectionKey> = dropped.relinked.iter().map(|s| s.key).collect();
    assert_eq!(relinked, keys(&[edge, pos(0, 1, 0, 1)]));
    assert!(
        dropped
            .relinked
            .iter()
            .all(|section| section.neighbours.get(Face::PosX) == NeighbourLevel::None)
    );
}

#[test]
fn updates_take_units_nearest_first_within_the_byte_budget() {
    let sections = [pos(0, 9, 0, 0), pos(0, 1, 0, 0), pos(0, 5, 0, 0)];
    let mut set = ShownSet::default();
    set.set_target(&sections, &ResidentColumns::default());
    for &section in &sections {
        set.mesh_arrived(section.key(), Some(mesh(section)));
    }
    let budget = mesh(sections[0]).heap_bytes();
    let order: Vec<Vec<SectionKey>> = (0..3)
        .map(|_| upserted(&set.update(CAMERA, budget).expect("one each")))
        .collect();
    assert_eq!(
        order,
        vec![
            vec![sections[1].key()],
            vec![sections[2].key()],
            vec![sections[0].key()]
        ]
    );
    assert!(set.update(CAMERA, budget).is_none());
}

#[test]
fn a_reset_removes_everything_without_releasing() {
    let sections = [pos(0, 0, 0, 0), pos(0, 3, 0, 0)];
    let mut set = showing(&sections);
    set.reset();
    let update = set.update(CAMERA, BUDGET).expect("cleared");
    assert_eq!(update.removed, keys(&sections));
    assert!(update.shown.is_empty());
    assert_eq!(interest(&mut set), (Vec::new(), Vec::new()));
    assert_eq!(set.stats().shown, 0);
}

#[test]
fn a_reset_never_keeps_old_sections_under_a_new_target() {
    let sections = [pos(0, 0, 0, 0), pos(0, 3, 0, 0)];
    let mut set = showing(&sections);
    set.reset();
    // The new target names the same keys, as another dimension's sections would.
    set.set_target(&sections, &ResidentColumns::default());
    let update = set.update(CAMERA, BUDGET).expect("cleared");
    assert_eq!(update.removed, keys(&sections));
    assert!(
        update.upserts.is_empty(),
        "nothing of the new target arrived"
    );
    assert!(update.shown.is_empty());
    assert_eq!(set.stats().waiting, sections.len());
    set.mesh_arrived(sections[0].key(), Some(mesh(sections[0])));
    let update = set
        .update(CAMERA, BUDGET)
        .expect("the arrived section shows");
    assert_eq!(upserted(&update), vec![sections[0].key()]);
    assert!(update.removed.is_empty());
}
