use super::*;

/// An actor is hidden only when every sub-chunk its box touches is known and not visible.
#[test]
fn a_box_is_hidden_only_when_all_its_known_sub_chunks_are_invisible() {
    let key = |x, y, z| SubChunkKey::new(0, x, y, z);
    let cache = CaveVisibilityCache {
        camera: Some(key(0, 4, 0)),
        graph_generation: Some(7),
        visible: [key(1, 4, 0)].into_iter().collect(),
        initialized: true,
        ..CaveVisibilityCache::default()
    };
    let known = |key: SubChunkKey| key.y < 8;
    let hides = |low, high| cache.hides_box(0, 7, known, low, high);
    assert!(hides([-8.0, 64.0, 4.0], [-7.0, 66.0, 5.0]));
    // Straddling into the visible neighbour, or reaching an unknown sub-chunk, draws it.
    assert!(!hides([15.5, 64.0, 4.0], [16.5, 66.0, 5.0]));
    assert!(!hides([-8.0, 127.0, 4.0], [-7.0, 129.0, 5.0]));
    // A stale graph or another dimension never hides anything.
    assert!(!cache.hides_box(0, 8, known, [-8.0, 64.0, 4.0], [-7.0, 66.0, 5.0]));
    assert!(!cache.hides_box(1, 7, known, [-8.0, 64.0, 4.0], [-7.0, 66.0, 5.0]));
}

/// Only entities whose key entered or left the visible set are written.
#[test]
fn publishing_touches_only_entities_whose_visibility_flipped() {
    let key = |x| SubChunkKey::new(0, x, 0, 0);
    let entity = |x| Entity::from_raw_u32(x as u32 + 1).unwrap();
    let mut cache = CaveVisibilityCache {
        rendered: (0..4).map(|x| (key(x), entity(x))).collect(),
        visible_rendered: 4,
        ..CaveVisibilityCache::default()
    };
    let mut writes = Vec::new();
    cache.next_visible = [key(0), key(1), key(9)].into_iter().collect();
    cache.publish_next(|entity, visible| writes.push((entity, visible)));
    writes.sort_by_key(|(entity, _)| entity.index());
    assert_eq!(writes, [(entity(2), false), (entity(3), false)]);
    assert_eq!(cache.visible_rendered, 2);

    writes.clear();
    cache.next_visible = [key(1), key(2), key(9)].into_iter().collect();
    cache.publish_next(|entity, visible| writes.push((entity, visible)));
    writes.sort_by_key(|(entity, _)| entity.index());
    assert_eq!(writes, [(entity(0), false), (entity(2), true)]);
    assert_eq!(cache.visible_rendered, 2);

    writes.clear();
    cache.next_visible = cache.visible.clone();
    cache.publish_next(|entity, visible| writes.push((entity, visible)));
    assert!(writes.is_empty());
}
