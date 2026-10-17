use super::*;
use render_model::hats::thumbnail::prepared_frame;

/// Admits a separately named animated rig so other tests cannot warm its camera cache.
fn cold_cosmetic(id: &str) -> cosmetics::Item {
    let mut descriptor = render_model::hats::fixture::bunny().descriptor().clone();
    descriptor.id = format!("fixture:artwork-slice-{id}");
    render_model::hats::admit(
        descriptor,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../render-model/src/hats/bunny_ears.geo.json"
        )),
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../render-model/src/hats/bunny_ears.png"
        )),
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../render-model/src/hats/fixture/animation.json"
        )),
    )
    .unwrap()
}

#[test]
fn cosmetic_preparation_waits_for_each_decode_slice() {
    let items = [cold_cosmetic("first"), cold_cosmetic("second")];
    let set = ArtworkSet {
        cosmetics: items
            .map(|item| CosmeticArtwork {
                item,
                kind: CosmeticArtworkKind::GalleryTexture,
            })
            .into(),
        ..Default::default()
    };
    let mut cache = DecodeCache::default();
    let missing = cache.missing(&set);
    assert_eq!(missing.len(), items.len());
    assert!(items.iter().all(|&item| prepared_frame(item).is_none()));

    cache.decode(&missing[..1], &set);
    assert!(prepared_frame(items[0]).is_some());
    assert!(prepared_frame(items[1]).is_none());
    assert!(cache.decoded.contains_key(&missing[0].key()));
    assert!(!cache.decoded.contains_key(&missing[1].key()));

    let missing = cache.missing(&set);
    assert_eq!(missing.len(), 1);
    assert!(prepared_frame(items[1]).is_none());
    cache.decode(&missing, &set);
    assert!(prepared_frame(items[1]).is_some());
    assert!(cache.missing(&set).is_empty());
}
