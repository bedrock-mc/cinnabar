use super::super::{Source, cosmetic_requests};
use super::*;

#[test]
fn cosmetic_texture_edge_uvs_sample_original_border_texels() {
    let item = render_model::hats::fixture::crown();
    let set = ArtworkSet {
        cosmetics: cosmetic_requests::requested(&[item], &[], false),
        paths: vec![("neighbor".into(), 2)],
        ..Default::default()
    };
    let colors = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255; 4],
    ];
    let mut cache = DecodeCache::default();
    cache.decoded.insert(
        Source::Item(item, false).key(),
        Arc::new(Artwork {
            width: 2,
            height: 2,
            pixels: colors.into_iter().flatten().collect(),
        }),
    );
    cache.decoded.insert(
        Source::File("neighbor".into(), 2).key(),
        Arc::new(Artwork {
            width: 2,
            height: 2,
            pixels: [77, 88, 99, 255].repeat(4),
        }),
    );
    let packed = pack(&set, &cache, 1, true);
    let icon = packed.refs[&item.texture_key()];
    let page = &packed.pages[usize::from(icon.page)];
    let sample = |uv: [f32; 2]| {
        let [left, top, right, bottom] = icon.uv.map(f32::from);
        let x = (left + uv[0] * (right - left)) as usize;
        let y = (top + uv[1] * (bottom - top)) as usize;
        let index = (y * page.dimensions()[0] as usize + x) * 4;
        &page.pixels()[index..index + 4]
    };
    assert_eq!(sample([1.0, 0.25]), colors[1]);
    assert_eq!(sample([0.25, 1.0]), colors[2]);
    assert_eq!(sample([1.0, 1.0]), colors[3]);
    let neighbor = packed.refs["neighbor"];
    let page = &packed.pages[usize::from(neighbor.page)];
    let index = (usize::from(neighbor.uv[1]) * page.dimensions()[0] as usize
        + usize::from(neighbor.uv[0]))
        * 4;
    assert_eq!(&page.pixels()[index..index + 4], &[77, 88, 99, 255]);
}
