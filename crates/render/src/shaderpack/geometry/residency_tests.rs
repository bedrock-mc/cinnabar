use super::*;

/// A 28-byte vertex, as a packed layout stores.
const STRIDE: u64 = 28;
const FACE_BYTES: u64 = face_bytes(STRIDE);

fn plan(faces: u32) -> SectionPlan {
    SectionPlan {
        faces,
        categories: [0..faces, faces..faces, faces..faces],
        facing: Default::default(),
        jobs: Default::default(),
    }
}

fn entity(index: u32) -> Entity {
    Entity::from_raw_u32(index).unwrap()
}

/// Frames without streaming, enough for draining to start.
fn settle(residency: &mut Residency) {
    for _ in 0..DRAIN_SETTLED_FRAMES {
        residency.observe_frame(false);
    }
}

#[test]
fn pages_hold_the_page_size_or_the_binding_limit() {
    let residency = Residency::new(u64::MAX / 2, 1 << 30, STRIDE);
    assert_eq!(u64::from(residency.page_faces), PAGE_BYTES / FACE_BYTES);
    let small = Residency::new(100 * FACE_BYTES, 1 << 30, STRIDE);
    assert_eq!(small.page_faces, 100);
}

#[test]
fn revoking_a_page_that_failed_to_allocate_drops_it_and_parks_the_section() {
    let mut residency = Residency::new(10 * FACE_BYTES, 1 << 30, STRIDE);
    let first = residency.place(entity(1), &plan(10)).unwrap();
    residency.commit(entity(1), &first, &plan(10), [0; 3]);
    let opened = residency.place(entity(2), &plan(5)).unwrap();
    assert!(opened.new_page);
    residency.revoke(entity(2), &opened);
    assert_eq!(residency.pages.len(), 1);
    assert_eq!(residency.capacity_faces(), 10);
    assert_eq!(residency.waiting_len(), 1);
    assert!(!residency.sections.contains_key(&entity(2)));
}

#[test]
fn pages_open_whole_and_the_last_takes_what_the_budget_leaves() {
    let mut residency = Residency::new(100 * FACE_BYTES, 250 * FACE_BYTES, STRIDE);
    let first = residency.place(entity(1), &plan(60)).unwrap();
    assert_eq!(
        first,
        Placement {
            page: 0,
            faces: 0..60,
            new_page: true
        }
    );
    assert_eq!(residency.capacity_faces(), 100, "a page opens at full size");
    residency.commit(entity(1), &first, &plan(60), [0; 3]);
    let second = residency.place(entity(2), &plan(40)).unwrap();
    assert_eq!(
        second,
        Placement {
            page: 0,
            faces: 60..100,
            new_page: false
        },
        "faces fill the open page before another opens"
    );
    residency.commit(entity(2), &second, &plan(40), [0; 3]);
    let third = residency.place(entity(3), &plan(90)).unwrap();
    assert_eq!((third.page, third.new_page), (1, true));
    residency.commit(entity(3), &third, &plan(90), [0; 3]);
    let fourth = residency.place(entity(4), &plan(50)).unwrap();
    assert_eq!((fourth.page, fourth.new_page), (2, true));
    assert_eq!(
        residency.pages[2].faces.capacity(),
        50,
        "the last page is what the budget leaves"
    );
    residency.commit(entity(4), &fourth, &plan(50), [0; 3]);
    assert_eq!(
        residency.place(entity(5), &plan(20)),
        None,
        "the budget is spent"
    );
    assert_eq!(residency.pages.len(), 3);
    residency.remove(entity(3));
    assert_eq!(
        residency
            .place(entity(5), &plan(20))
            .map(|placed| placed.page),
        Some(1),
        "removed faces are reused"
    );
}

#[test]
fn a_replacement_that_does_not_fit_keeps_the_old_faces() {
    let mut residency = Residency::new(100 * FACE_BYTES, 100 * FACE_BYTES, STRIDE);
    let placement = residency.place(entity(1), &plan(10)).unwrap();
    residency.commit(entity(1), &placement, &plan(10), [0; 3]);
    assert_eq!(residency.place(entity(1), &plan(101)), None);
    assert_eq!(residency.sections[&entity(1)].faces, 0..10);
    assert!(
        residency.place(entity(2), &plan(90)).is_some(),
        "only the old faces stay taken"
    );
}

#[test]
fn the_origin_follows_the_camera_on_a_grid_and_counts_epochs() {
    let mut residency = Residency::new(1 << 20, 1 << 20, STRIDE);
    assert!(residency.follow_camera(DVec3::new(700.0, 64.0, -1600.0)));
    assert_eq!(residency.origin, Some(IVec3::new(1024, 0, -2048)));
    assert!(!residency.follow_camera(DVec3::new(3000.0, 64.0, -2048.0)));
    assert!(residency.follow_camera(DVec3::new(6000.0, 64.0, -2048.0)));
    assert_eq!(residency.origin, Some(IVec3::new(6144, 0, -2048)));
    assert_eq!(residency.epoch, 2);
}

#[test]
fn packed_faces_stay_current_across_a_rebase() {
    let mut residency = Residency::new(100 * FACE_BYTES, 100 * FACE_BYTES, STRIDE);
    residency.follow_camera(DVec3::ZERO);
    let placement = residency.place(entity(1), &plan(10)).unwrap();
    residency.commit(entity(1), &placement, &plan(10), [0; 3]);
    assert!(residency.follow_camera(DVec3::new(6000.0, 0.0, 0.0)));
    assert_ne!(residency.sections[&entity(1)].epoch, residency.epoch);
    residency.keep_faces_across_rebase();
    assert_eq!(residency.sections[&entity(1)].epoch, residency.epoch);
    assert_eq!(residency.pending_len(), 0, "nothing re-expands");
}

#[test]
fn the_queue_holds_each_section_once_and_forgets_removed_ones() {
    let mut residency = Residency::new(1 << 20, 1 << 20, STRIDE);
    residency.enqueue_all([entity(1), entity(2), entity(1)]);
    residency.remove(entity(2));
    assert_eq!(residency.pending_len(), 1);
    assert_eq!(residency.take_queue(), [entity(1)]);
    assert_eq!(residency.pending_len(), 0);
    residency.wait(entity(3));
    residency.enqueue(entity(3));
    assert_eq!(
        residency.pending_len(),
        1,
        "a queued section does not also wait"
    );
}

#[test]
fn over_budget_nearer_sections_evict_the_farthest_and_farther_ones_wait() {
    // Room for exactly two sections of 100 faces.
    let mut residency = Residency::new(1 << 30, 200 * FACE_BYTES, STRIDE);
    let eye = DVec3::new(8.0, 8.0, 8.0);
    let at = |x: i32| [x, 0, 0];
    let place = |residency: &mut Residency, id: u32, x: i32, eviction: &mut Eviction| {
        let near = Some((eye, section_distance(at(x), eye)));
        let placement = residency.place_nearest(entity(id), &plan(100), near, eviction)?;
        residency.commit(entity(id), &placement, &plan(100), at(x));
        Some(placement)
    };
    let mut eviction = Eviction::default();
    assert!(place(&mut residency, 1, 160, &mut eviction).is_some());
    assert!(place(&mut residency, 2, 320, &mut eviction).is_some());
    // A nearer section takes the farthest one's room; that one waits.
    let mut eviction = Eviction::default();
    assert!(place(&mut residency, 3, 0, &mut eviction).is_some());
    assert!(residency.sections.contains_key(&entity(1)));
    assert!(!residency.sections.contains_key(&entity(2)));
    assert_eq!(residency.pending().collect::<Vec<_>>(), [entity(2)]);
    // A farther section never evicts nearer ones.
    let mut eviction = Eviction::default();
    assert!(place(&mut residency, 4, 480, &mut eviction).is_none());
    assert!(residency.sections.contains_key(&entity(1)));
    assert!(residency.sections.contains_key(&entity(3)));
    // Without a camera nothing is evicted.
    assert_eq!(
        residency.place_nearest(entity(5), &plan(100), None, &mut Eviction::default()),
        None
    );
    // Freed room brings waiting sections back to the queue.
    residency.remove(entity(1));
    residency.retry_waiting();
    assert_eq!(residency.take_queue(), [entity(2)]);
}

#[test]
fn draw_origin_slots_are_reused_and_survive_a_failed_replacement() {
    let mut residency = Residency::new(100 * FACE_BYTES, 100 * FACE_BYTES, STRIDE);
    let commit = |residency: &mut Residency, id: u32, faces: u32| {
        let placement = residency.place(entity(id), &plan(faces)).unwrap();
        residency.commit(entity(id), &placement, &plan(faces), [0; 3])
    };
    assert_eq!(commit(&mut residency, 1, 10), 0);
    assert_eq!(commit(&mut residency, 2, 10), 1);
    // A replacement that does not fit keeps the section's slot with its old faces.
    assert_eq!(residency.place(entity(1), &plan(101)), None);
    assert_eq!(residency.sections[&entity(1)].slot, 0);
    // A replacement that fits gives the old slot back and takes the free one.
    assert_eq!(commit(&mut residency, 1, 20), 0);
    residency.remove(entity(2));
    assert_eq!(commit(&mut residency, 3, 10), 1);
    assert_eq!(residency.slot_count(), 2);
}

#[test]
fn a_page_whose_faces_fit_elsewhere_drains_and_goes() {
    let mut residency = Residency::new(100 * FACE_BYTES, 1 << 30, STRIDE);
    let commit = |residency: &mut Residency, id: u32, faces: u32| {
        let placement = residency.place(entity(id), &plan(faces)).unwrap();
        residency.commit(entity(id), &placement, &plan(faces), [id as i32; 3]);
        placement.page
    };
    // Three pages: [60, 40], [90], [50] faces.
    assert_eq!(commit(&mut residency, 1, 60), 0);
    assert_eq!(commit(&mut residency, 2, 40), 0);
    assert_eq!(commit(&mut residency, 3, 90), 1);
    assert_eq!(commit(&mut residency, 4, 50), 2);
    // Freeing 60 faces of page 0 leaves 60 + 10 + 50 free: page 0's 40 fit elsewhere.
    residency.remove(entity(1));
    residency.enqueue(entity(3));
    settle(&mut residency);
    assert!(!residency.start_draining(), "nothing may be pending");
    assert_eq!(residency.take_queue(), [entity(3)]);
    assert!(residency.start_draining());
    assert_eq!(residency.take_queue(), [entity(2)]);
    assert_eq!(
        residency.finish_draining(),
        None,
        "page 0 still holds faces"
    );
    // The moved section lands on page 2, not back on page 0.
    assert_eq!(commit(&mut residency, 2, 40), 2);
    assert_eq!(residency.finish_draining(), Some(0));
    assert_eq!(residency.pages.len(), 2);
    assert_eq!(
        residency.sections[&entity(3)].page,
        0,
        "later pages renumber"
    );
    assert_eq!(residency.sections[&entity(2)].page, 1);
    assert_eq!(residency.sections[&entity(4)].page, 1);
}

#[test]
fn a_draining_page_takes_a_placement_nothing_else_has_room_for_and_stays() {
    let mut residency = Residency::new(100 * FACE_BYTES, 200 * FACE_BYTES, STRIDE);
    // Page 0: 50 + 50 faces; page 1: 20.
    for (id, faces) in [(1, 50), (2, 50), (3, 20)] {
        let placement = residency.place(entity(id), &plan(faces)).unwrap();
        residency.commit(entity(id), &placement, &plan(faces), [0; 3]);
    }
    residency.remove(entity(1));
    settle(&mut residency);
    assert!(
        residency.start_draining(),
        "page 1's 20 faces fit in page 0's 50"
    );
    assert_eq!(residency.take_queue(), [entity(3)]);
    // 70 faces fit only in the draining page, which then stays.
    let fallback = residency.place(entity(4), &plan(70)).unwrap();
    assert_eq!((fallback.page, fallback.new_page), (1, false));
    assert_eq!(residency.finish_draining(), None, "draining was given up");
}

#[test]
fn pages_drain_only_once_streaming_settles() {
    let mut residency = Residency::new(100 * FACE_BYTES, 1 << 30, STRIDE);
    for (id, faces) in [(1, 60), (2, 40), (3, 30)] {
        let placement = residency.place(entity(id), &plan(faces)).unwrap();
        residency.commit(entity(id), &placement, &plan(faces), [0; 3]);
    }
    residency.remove(entity(1));
    settle(&mut residency);
    residency.observe_frame(true);
    for _ in 1..DRAIN_SETTLED_FRAMES {
        residency.observe_frame(false);
    }
    assert!(
        !residency.start_draining(),
        "a section arrived or left within the last DRAIN_SETTLED_FRAMES frames"
    );
    residency.observe_frame(false);
    assert!(residency.start_draining());
    assert_eq!(residency.take_queue(), [entity(3)]);
}

#[test]
fn oversized_nearest_section_preserves_residents_and_can_retry_after_a_change() {
    let mut residency = Residency::new(100 * FACE_BYTES, 200 * FACE_BYTES, STRIDE);
    let far = entity(1);
    let near = entity(2);
    let placement = residency.place(far, &plan(100)).unwrap();
    residency.commit(far, &placement, &plan(100), [320, 0, 0]);
    let previous = residency.sections[&far].clone();
    let mut eviction = Eviction::default();
    assert!(
        residency
            .place_nearest(near, &plan(101), Some((DVec3::ZERO, 0.0)), &mut eviction)
            .is_none()
    );
    assert_eq!(residency.sections.get(&far), Some(&previous));
    assert!(residency.has_unplaceable());
    residency.wait(near);
    residency.enqueue(near);
    assert_eq!(residency.take_queue(), [near]);
    assert!(
        residency
            .place_nearest(near, &plan(100), Some((DVec3::ZERO, 0.0)), &mut eviction)
            .is_some()
    );
    assert_eq!(residency.sections.get(&far), Some(&previous));
    assert!(!residency.has_unplaceable());
}

#[test]
fn a_request_larger_than_the_total_budget_cannot_evict_residents() {
    let mut residency = Residency::new(200 * FACE_BYTES, 100 * FACE_BYTES, STRIDE);
    let placement = residency.place(entity(1), &plan(10)).unwrap();
    residency.commit(entity(1), &placement, &plan(10), [320, 0, 0]);
    assert!(
        residency
            .place_nearest(
                entity(2),
                &plan(101),
                Some((DVec3::ZERO, 0.0)),
                &mut Eviction::default()
            )
            .is_none()
    );
    assert!(residency.sections.contains_key(&entity(1)));
}

#[test]
fn a_budget_change_retries_unplaceable_sections_without_evicting_other_pages() {
    let mut residency = Residency::new(200 * FACE_BYTES, 100 * FACE_BYTES, STRIDE);
    let placement = residency.place(entity(1), &plan(10)).unwrap();
    residency.commit(entity(1), &placement, &plan(10), [320, 0, 0]);
    let mut eviction = Eviction::default();
    let near = Some((DVec3::ZERO, 0.0));
    assert!(
        residency
            .place_nearest(entity(2), &plan(101), near, &mut eviction)
            .is_none()
    );
    assert!(residency.has_unplaceable());
    residency.wait(entity(2));
    residency.set_budget(200 * FACE_BYTES);
    assert_eq!(residency.take_queue(), [entity(2)]);
    assert!(
        residency
            .place_nearest(entity(2), &plan(101), near, &mut eviction)
            .is_none()
    );
    assert!(
        residency.sections.contains_key(&entity(1)),
        "the existing small page cannot grow by eviction"
    );
    residency.wait(entity(2));
    residency.set_budget(201 * FACE_BYTES);
    assert_eq!(residency.take_queue(), [entity(2)]);
    assert!(
        residency
            .place_nearest(entity(2), &plan(101), near, &mut eviction)
            .is_some()
    );
    assert!(!residency.has_unplaceable());
    assert!(residency.sections.contains_key(&entity(1)));
    residency.remove(entity(2));
    assert!(!residency.has_unplaceable());
}

#[test]
fn almost_full_pages_decline_draining_without_unsigned_underflow() {
    let mut residency = Residency::new(100 * FACE_BYTES, 200 * FACE_BYTES, STRIDE);
    for id in 1..=2 {
        let placement = residency.place(entity(id), &plan(90)).unwrap();
        residency.commit(entity(id), &placement, &plan(90), [0; 3]);
    }
    settle(&mut residency);
    assert!(!residency.start_draining());
    assert!(residency.take_queue().is_empty());
}

#[test]
fn a_failed_fragmented_drain_waits_for_space_to_change() {
    let mut residency = Residency::new(100 * FACE_BYTES, 300 * FACE_BYTES, STRIDE);
    for (id, faces) in [(1, 40), (2, 70), (3, 70)] {
        let placement = residency.place(entity(id), &plan(faces)).unwrap();
        residency.commit(entity(id), &placement, &plan(faces), [0; 3]);
    }
    settle(&mut residency);
    assert!(residency.start_draining());
    assert_eq!(residency.take_queue(), [entity(1)]);
    let placement = residency.place(entity(1), &plan(40)).unwrap();
    assert_eq!(placement.page, 0);
    residency.commit(entity(1), &placement, &plan(40), [0; 3]);
    for _ in 0..DRAIN_SETTLED_FRAMES * 2 {
        residency.observe_frame(false);
        assert!(!residency.start_draining());
        assert!(residency.take_queue().is_empty());
    }
    let unchanged = residency.place(entity(2), &plan(70)).unwrap();
    residency.commit(entity(2), &unchanged, &plan(70), [0; 3]);
    assert!(
        !residency.start_draining(),
        "same-size replacement kept the same free ranges"
    );
    let placement = residency.place(entity(2), &plan(50)).unwrap();
    residency.commit(entity(2), &placement, &plan(50), [0; 3]);
    assert!(
        residency.start_draining(),
        "a replacement changed the free ranges"
    );
}

#[test]
fn failed_drains_keep_all_queued_movers_blocked() {
    let mut residency = Residency::new(100 * FACE_BYTES, 600 * FACE_BYTES, STRIDE);
    for (id, faces) in [
        (1, 20),
        (2, 20),
        (3, 90),
        (4, 90),
        (5, 90),
        (6, 90),
        (7, 90),
    ] {
        let placement = residency.place(entity(id), &plan(faces)).unwrap();
        residency.commit(entity(id), &placement, &plan(faces), [0; 3]);
    }
    settle(&mut residency);
    assert!(residency.start_draining());
    let movers = residency.take_queue();
    assert_eq!(movers.len(), 2);
    for mover in movers {
        let placement = residency.place(mover, &plan(20)).unwrap();
        assert_eq!(placement.page, 0);
        residency.commit(mover, &placement, &plan(20), [0; 3]);
    }
    assert!(
        !residency.start_draining(),
        "later queued movers did not change free space"
    );
    assert!(residency.take_queue().is_empty());
}
