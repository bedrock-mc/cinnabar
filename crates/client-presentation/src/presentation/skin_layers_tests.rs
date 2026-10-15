use super::*;

/// One retained rectangular sprite sheet for the texture-page cache.
fn raster() -> EquipmentRaster {
    EquipmentRaster {
        width: 24,
        height: 512,
        rgba8: vec![255; 24 * 512 * 4].into(),
    }
}

#[test]
fn unchanged_animation_images_and_empty_views_reuse_pages() {
    let base = ActorArtworkPages::default();
    let image = raster();
    let mut cache = SkinLayerCache::default();
    cache.desired.push(image.clone());
    let first = cache.update_pages(&base).unwrap();
    assert_eq!(first.pages()[0].dimensions(), (image.width, image.height));
    cache.desired.clear();
    cache.desired.push(image.clone());
    assert!(cache.update_pages(&base).is_none());
    assert_eq!(cache.pages.as_ref().unwrap().identity(), first.identity());
    cache.desired.clear();
    assert!(cache.update_pages(&base).is_none());
    assert_eq!(cache.pages.as_ref().unwrap().identity(), first.identity());
    cache.retain_image(image.clone());
    assert!(cache.update_pages(&base).is_none());
    assert!(cache.image_location(&image).is_some());
}
#[test]
fn new_images_preserve_the_slots_and_allocations_of_resident_images() {
    let base = ActorArtworkPages::default();
    let (first, second, third) = (raster(), raster(), raster());
    let mut cache = SkinLayerCache::default();
    let publish = |cache: &mut SkinLayerCache, visible: &[&EquipmentRaster]| {
        cache.desired.clear();
        for image in visible {
            cache.retain_image((*image).clone());
        }
        cache.update_pages(&base)
    };
    let both = publish(&mut cache, &[&first, &second]).expect("first sight packs");
    assert!(publish(&mut cache, &[&first]).is_none(), "leaving view");
    assert!(cache.image_location(&first).is_some());
    assert!(
        publish(&mut cache, &[&second, &first]).is_none(),
        "returning"
    );
    assert_eq!(cache.pages.as_ref().unwrap().identity(), both.identity());
    let original_location = cache.image_location(&first);
    let next = publish(&mut cache, &[&first, &third]).expect("a new image appends");
    assert_eq!(cache.image_location(&first), original_location);
    for (old, new) in both.pages().iter().zip(next.pages()) {
        assert!(Arc::ptr_eq(&old.rgba8, &new.rgba8));
    }
    assert!(cache.image_location(&third).is_some());
    assert!(
        cache.image_location(&second).is_some(),
        "an arrival retains the offscreen image"
    );
}

thread_local! {
    static LOOKUP_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts explicit source-image candidates on this test thread.
pub(super) fn record_probe() {
    LOOKUP_PROBES.with(|count| count.set(count.get() + 1));
}

#[test]
fn bounded_lookup_work_for_distinct_skin_animation_images() {
    let images: Vec<_> = (0..render_model::MAX_RENDERED_PLAYERS)
        .map(|_| raster())
        .collect();
    let mut cache = SkinLayerCache::default();
    let base = ActorArtworkPages::default();
    LOOKUP_PROBES.with(|count| count.set(0));
    for image in &images {
        cache.retain_image(image.clone());
    }
    cache.update_pages(&base).unwrap();
    assert_eq!(
        cache.rasters.len(),
        images.len(),
        "equal pixels in separate allocations remain separate sources"
    );
    for image in &images {
        assert!(cache.image_location(image).is_some());
    }
    let work = LOOKUP_PROBES.with(std::cell::Cell::get);
    assert!(
        work <= images.len() * 4,
        "{work} image candidates for {} layers",
        images.len()
    );
    cache.desired.clear();
    for image in &images {
        cache.retain_image(image.clone());
    }
    assert!(cache.update_pages(&base).is_none());
    cache.desired.clear();
    assert!(cache.update_pages(&base).is_none());
    for image in &images {
        assert!(cache.image_location(image).is_some());
    }
}

/// Publishes one frame's selected images without retaining the previous desired set.
fn publish(
    cache: &mut SkinLayerCache,
    base: &ActorArtworkPages,
    images: &[EquipmentRaster],
) -> Option<ActorArtworkPages> {
    cache.desired.clear();
    for image in images {
        cache.retain_image(image.clone());
    }
    cache.update_pages(base)
}

#[test]
fn full_residency_replaces_only_an_offscreen_slot() {
    let base = ActorArtworkPages::default();
    let images: Vec<_> = (0..MAX_RESIDENT_SKIN_IMAGES).map(|_| raster()).collect();
    let mut cache = SkinLayerCache::default();
    let original = publish(&mut cache, &base, &images).unwrap();
    let retained = cache.image_location(&images[0]).unwrap();
    let evicted = cache.image_location(&images[1]).unwrap();
    let weak = Arc::downgrade(&images[1].rgba8);
    let newcomer = raster();
    let next = publish(&mut cache, &base, &[images[0].clone(), newcomer.clone()]).unwrap();
    assert_eq!(cache.rasters.len(), MAX_RESIDENT_SKIN_IMAGES);
    assert_eq!(next.pages().len(), MAX_RESIDENT_SKIN_IMAGES);
    assert_eq!(cache.image_location(&images[0]), Some(retained));
    assert_eq!(cache.image_location(&newcomer), Some(evicted));
    assert!(cache.image_location(&images[1]).is_none());
    for (index, (old, new)) in original.pages().iter().zip(next.pages()).enumerate() {
        assert_eq!(
            Arc::ptr_eq(&old.rgba8, &new.rgba8),
            index != usize::from(evicted.page()) - 1
        );
    }
    drop(original);
    drop(images);
    assert!(
        weak.upgrade().is_none(),
        "eviction releases the source allocation"
    );
}

#[test]
fn base_replacement_invalidates_pages_even_without_visible_images() {
    let base = ActorArtworkPages::default();
    let mut cache = SkinLayerCache::default();
    let image = raster();
    publish(&mut cache, &base, std::slice::from_ref(&image)).unwrap();
    let replacement = base.clone().with_actor_glint(EquipmentRaster {
        width: 1,
        height: 1,
        rgba8: Arc::from([3; 4]),
    });
    let published = publish(&mut cache, &replacement, &[]).unwrap();
    assert!(published.shares_storage_with(&replacement));
    assert!(cache.image_location(&image).is_none());
    assert!(cache.pages.is_none());
    let next = publish(&mut cache, &replacement, std::slice::from_ref(&image)).unwrap();
    assert!(Arc::ptr_eq(&next.pages()[0].rgba8, &image.rgba8));
    let mut routes_changed = replacement.clone();
    routes_changed.entity_identity = [17; 32];
    let rebuilt = publish(&mut cache, &routes_changed, &[image]).unwrap();
    assert_eq!(rebuilt.entity_identity, routes_changed.entity_identity);
}

#[test]
fn malformed_images_do_not_leave_holes_before_valid_page_slots() {
    let mut bad = raster();
    bad.width += 1;
    let good = raster();
    let mut cache = SkinLayerCache::default();
    let pages = publish(
        &mut cache,
        &ActorArtworkPages::default(),
        &[bad.clone(), good.clone()],
    )
    .unwrap();
    assert_eq!(pages.pages().len(), 1);
    assert!(cache.image_location(&bad).is_none());
    assert_eq!(cache.image_location(&good).unwrap().page(), 1);
}

#[test]
fn new_pixels_in_a_replacement_skin_receive_their_own_slot() {
    let base = ActorArtworkPages::default();
    let original = raster();
    let mut replacement = raster();
    Arc::make_mut(&mut replacement.rgba8)[3] = 7;
    let mut cache = SkinLayerCache::default();
    publish(&mut cache, &base, std::slice::from_ref(&original)).unwrap();
    let old = cache.image_location(&original).unwrap();
    let pages = publish(&mut cache, &base, std::slice::from_ref(&replacement)).unwrap();
    let new = cache.image_location(&replacement).unwrap();
    assert_ne!(old, new);
    assert_eq!(
        pages.pages()[usize::from(new.page()) - 1].pixels(),
        replacement.rgba8.as_ref()
    );
}

#[test]
fn empty_batches_keep_resident_pages_without_drawing_or_resolving_rigs() {
    let base = ActorArtworkPages::default();
    let image = raster();
    let mut cache = SkinLayerCache::default();
    let first = publish(&mut cache, &base, &[image]).unwrap();
    let mut batch = ActorPresentationBatch {
        submissions: Vec::new(),
        skin_layers: Vec::new(),
        artwork: Default::default(),
    };
    assert!(
        cache
            .apply(
                &mut batch,
                &base,
                |_| panic!("an empty view has no rig"),
                &mut SkinRigCache::default(),
                |_| panic!("an empty view has no geometry")
            )
            .is_none()
    );
    assert!(batch.submissions.is_empty());
    assert!(batch.artwork.is_empty());
    assert!(cache.pages.as_ref().unwrap().shares_storage_with(&first));
}

#[test]
fn page_id_pressure_uses_the_existing_grouped_admission_policy() {
    let pixel = render_model::ActorTexturePage {
        width: 1,
        height: 1,
        layers: 1,
        rgba8: Arc::from([1; 4]),
        color_mask: false,
        multitexture: false,
    };
    // Leave one generic page ID: both images still fit together in the grouped path.
    let mut base = ActorArtworkPages::default();
    base.pages = vec![pixel; render_model::MAX_ACTOR_TEXTURE_PAGES - 2].into();
    let images = [raster(), raster()];
    let (expected, locations) = base.clone().with_equipment_rasters(&images);
    let mut cache = SkinLayerCache::default();
    let actual = publish(&mut cache, &base, &images).unwrap();
    assert!(cache.packed);
    assert_eq!(actual.identity(), expected.identity());
    assert_eq!(
        actual.pages().last().unwrap(),
        expected.pages().last().unwrap()
    );
    for (image, location) in images.iter().zip(locations) {
        assert_eq!(cache.image_location(image), location);
    }
    assert!(publish(&mut cache, &base, &images).is_none());
    assert!(publish(&mut cache, &base, &[]).is_none());
}
