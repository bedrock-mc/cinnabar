use super::*;
use render_model::hats::thumbnail::prepared_frame;

/// Admits a separately named animated rig so other tests cannot warm its camera cache.
fn cold_cosmetic(id: &str) -> cosmetics::Item {
    render_model::hats::fixture::named_bunny(&format!("fixture:artwork-slice-{id}"))
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
