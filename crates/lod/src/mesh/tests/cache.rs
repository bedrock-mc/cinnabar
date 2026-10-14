//! The mesh cache: versions, stale results, fresh meshes and the byte budget.

use super::*;
use crate::mesh::cache::{Completion, MeshCache};

/// A small mesh of `pos`: one stone voxel in air.
fn mesh_of(pos: SectionPos) -> SectionMesh {
    let generate = move |level: u8, at: [i64; 3]| {
        let origin = [pos.x(), pos.y(), pos.z()].map(|value| i64::from(value) * 32);
        if level == pos.level() && at == origin {
            block(blocks::STONE, Light::FULL_SKY)
        } else {
            Voxel::air(Light::FULL_SKY)
        }
    };
    let mut world = World::new(&generate);
    world.build_around(pos, |_| true);
    Mesher::new().mesh(&world.input(pos), &mesh_classes(), ExteriorCulling::Off)
}

#[test]
fn wanted_sections_are_meshed_once_and_their_meshes_handed_out_fresh() {
    let mut cache = MeshCache::new(1 << 20);
    let section = pos(0, 1, 2, 3);
    cache.want(section);
    cache.want(section);
    let job = cache.next_job().expect("a first mesh");
    assert_eq!(job.pos, section);
    assert_eq!(cache.next_job(), None, "one job per section");
    assert_eq!(
        cache.complete(job, Some(mesh_of(section))),
        Completion::Stored
    );
    assert!(cache.is_idle());
    let mut fresh = Vec::new();
    cache.take_fresh(&mut fresh);
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].0, section.key());
    assert!(fresh[0].1.as_ref().is_some_and(|mesh| !mesh.is_empty()));
    cache.take_fresh(&mut fresh);
    assert_eq!(fresh.len(), 1, "taken once");
    assert!(cache.mesh(section.key()).is_some(), "kept for upload again");
    cache.want(section);
    assert_eq!(cache.next_job(), None, "a current mesh is not meshed again");
}

#[test]
fn a_write_meshes_the_section_and_every_wanted_mesh_reading_it_again() {
    let mut cache = MeshCache::new(1 << 20);
    let section = pos(1, 0, 0, 0);
    let beside = section.neighbour(Face::PosX).expect("neighbour");
    let unrelated = pos(1, 9, 0, 0);
    for wanted in [section, beside, unrelated] {
        cache.want(wanted);
    }
    while let Some(job) = cache.next_job() {
        cache.complete(job, None);
    }
    cache.changed(beside);
    let mut again: Vec<SectionPos> = std::iter::from_fn(|| cache.next_job())
        .map(|job| job.pos)
        .collect();
    again.sort();
    let mut expected = vec![section, beside];
    expected.sort();
    assert_eq!(again, expected);
    // A finer child on the shared face is read by the coarser section's to-finer list.
    let child = crate::mesh::input::touching_children(beside, Face::NegX)[0].expect("child");
    let mut cache = MeshCache::new(1 << 20);
    cache.want(section);
    let job = cache.next_job().expect("job");
    cache.complete(job, None);
    cache.changed(child);
    assert_eq!(cache.next_job().map(|job| job.pos), Some(section));
}

#[test]
fn a_result_for_an_older_version_is_dropped_and_the_section_meshed_again() {
    let mut cache = MeshCache::new(1 << 20);
    let section = pos(0, 0, 0, 0);
    cache.want(section);
    let old = cache.next_job().expect("job");
    cache.changed(section);
    assert_eq!(cache.next_job(), None, "the running job finishes first");
    assert_eq!(
        cache.complete(old, Some(mesh_of(section))),
        Completion::Stale
    );
    let mut fresh = Vec::new();
    cache.take_fresh(&mut fresh);
    assert!(fresh.is_empty(), "stale meshes are never handed out");
    let new = cache.next_job().expect("meshed again");
    assert!(new.version > old.version);
    assert_eq!(cache.complete(new, None), Completion::Stored);
    assert_eq!(cache.stats().stale, 1);
}

#[test]
fn a_released_section_drops_its_job_even_when_wanted_again_meanwhile() {
    let mut cache = MeshCache::new(1 << 20);
    let section = pos(2, 0, 1, 0);
    cache.want(section);
    let old = cache.next_job().expect("job");
    cache.release(section.key());
    cache.want(section);
    let new = cache.next_job().expect("a job for the new want");
    assert_ne!(old.version, new.version);
    assert_eq!(cache.complete(old, None), Completion::Unwanted);
    assert_eq!(cache.complete(new, None), Completion::Stored);
    assert!(cache.is_idle());
}

#[test]
fn fresh_meshes_filling_the_budget_hold_new_jobs_back_and_taken_ones_are_dropped_first() {
    let first = pos(0, 0, 0, 0);
    let bytes = mesh_of(first).heap_bytes();
    let mut cache = MeshCache::new(bytes);
    let sections = [first, pos(0, 5, 0, 0), pos(0, 9, 0, 0)];
    for section in sections {
        cache.want(section);
    }
    let job = cache.next_job().expect("job");
    cache.complete(job, Some(mesh_of(job.pos)));
    assert_eq!(cache.next_job(), None, "the fresh mesh fills the budget");
    let mut fresh = Vec::new();
    cache.take_fresh(&mut fresh);
    assert_eq!(
        cache.next_job(),
        None,
        "a handed-out mesh fills the budget until received"
    );
    cache.received();
    let second = cache.next_job().expect("room again");
    cache.complete(second, Some(mesh_of(second.pos)));
    let stats = cache.stats();
    assert_eq!(stats.evicted, 1, "the taken mesh made room");
    assert!(stats.held_bytes <= bytes);
    assert!(cache.mesh(job.pos.key()).is_none());
    // A dropped mesh is meshed again once wanted again.
    cache.take_fresh(&mut fresh);
    cache.received();
    cache.want(job.pos);
    let jobs: Vec<SectionPos> = std::iter::from_fn(|| cache.next_job())
        .map(|job| job.pos)
        .collect();
    assert!(jobs.contains(&job.pos));
}

#[test]
fn a_deferred_job_goes_back_to_the_front_of_the_queue() {
    let mut cache = MeshCache::new(1 << 20);
    let (first, second) = (pos(0, 0, 0, 0), pos(0, 1, 0, 0));
    cache.want(first);
    cache.want(second);
    let job = cache.next_job().expect("job");
    assert_eq!(job.pos, first);
    cache.defer(job);
    let again = cache.next_job().expect("the deferred section first");
    assert_eq!(again, job, "same version");
    assert_eq!(cache.complete(again, None), Completion::Stored);
}

#[test]
fn a_change_where_some_sections_read_differently_meshes_only_those_and_their_readers() {
    let mut cache = MeshCache::new(1 << 20);
    let section = pos(1, 0, 0, 0);
    let beside = section.neighbour(Face::PosX).expect("neighbour");
    let unrelated = pos(1, 9, 0, 0);
    for wanted in [section, beside, unrelated] {
        cache.want(wanted);
    }
    while let Some(job) = cache.next_job() {
        cache.complete(job, None);
    }
    cache.changed_where(|at| at == beside);
    let mut again: Vec<SectionPos> = std::iter::from_fn(|| cache.next_job())
        .map(|job| job.pos)
        .collect();
    again.sort();
    let mut expected = vec![section, beside];
    expected.sort();
    assert_eq!(
        again, expected,
        "the section reading the changed one and the changed one"
    );
}

#[test]
fn handed_out_meshes_count_against_the_budget_until_received_even_once_dropped() {
    let sections = [pos(0, 0, 0, 0), pos(0, 5, 0, 0), pos(0, 9, 0, 0)];
    let bytes = mesh_of(sections[0]).heap_bytes();
    let mut cache = MeshCache::new(2 * bytes);
    for section in sections {
        cache.want(section);
    }
    let first = cache.next_job().expect("job");
    cache.complete(first, Some(mesh_of(first.pos)));
    let mut fresh = Vec::new();
    cache.take_fresh(&mut fresh);
    let second = cache
        .next_job()
        .expect("one more mesh fits beside the handed one");
    cache.complete(second, Some(mesh_of(second.pos)));
    assert_eq!(
        cache.next_job(),
        None,
        "fresh and handed meshes fill the budget"
    );
    assert_eq!(
        cache.stats().evicted,
        0,
        "a handed-out mesh is never dropped"
    );
    cache.release(first.pos.key());
    let stats = cache.stats();
    assert_eq!(stats.handed_bytes, bytes, "the receiver still holds it");
    assert_eq!(cache.next_job(), None, "so it still counts");
    cache.received();
    assert_eq!(cache.stats().handed_bytes, 0);
    assert!(cache.next_job().is_some(), "room once received");
}

#[test]
fn received_meshes_are_dropped_least_recently_used_first() {
    let sections = [pos(0, 0, 0, 0), pos(0, 5, 0, 0), pos(0, 9, 0, 0)];
    let bytes = mesh_of(sections[0]).heap_bytes();
    let mut cache = MeshCache::new(2 * bytes);
    let mut fresh = Vec::new();
    for section in &sections[..2] {
        cache.want(*section);
        let job = cache.next_job().expect("job");
        cache.complete(job, Some(mesh_of(job.pos)));
        cache.take_fresh(&mut fresh);
        cache.received();
    }
    // Using the older mesh makes the other one the least recently used.
    assert!(cache.mesh(sections[0].key()).is_some());
    cache.want(sections[2]);
    let job = cache.next_job().expect("job");
    cache.complete(job, Some(mesh_of(job.pos)));
    assert_eq!(cache.stats().evicted, 1);
    assert!(cache.mesh(sections[0].key()).is_some(), "recently used");
    assert!(
        cache.mesh(sections[1].key()).is_none(),
        "least recently used"
    );
    assert!(cache.mesh(sections[2].key()).is_some(), "fresh");
}
