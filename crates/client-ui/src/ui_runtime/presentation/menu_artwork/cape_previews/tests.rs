use super::super::{ArtworkSet, DecodeCache, pack};
use super::*;

#[test]
fn raw_hd_cape_fits_the_atlas_and_preserves_every_texel() {
    let (width, height) = render_api::CAPE_DIMENSIONS.last().copied().unwrap();
    let pixels: Vec<_> = (0..height)
        .flat_map(|y| (0..width).flat_map(move |x| [x as u8, y as u8, (x ^ y) as u8, 255]))
        .collect();
    let cape = CapeImage {
        width,
        height,
        rgba8: pixels.into(),
    };
    let key = cape_texture_key(&cape);
    let set = ArtworkSet {
        capes: vec![CapeArtwork {
            key: key.clone(),
            cape: cape.clone(),
            thumbnail: false,
        }],
        ..Default::default()
    };
    let mut cache = DecodeCache::default();
    cache.decode(&cache.missing(&set), &set);
    let packed = pack(&set, &cache, 1, true);
    let icon = packed.refs.get(&key).expect("raw cape is packed");
    let page = &packed.pages[icon.page as usize];
    let dimensions = page.dimensions();
    assert!(u32::from(icon.uv[2]) <= dimensions[0]);
    assert!(u32::from(icon.uv[3]) <= dimensions[1]);
    assert_eq!(u32::from(icon.uv[2] - icon.uv[0]), width);
    assert_eq!(u32::from(icon.uv[3] - icon.uv[1]), height);
    for y in 0..height as usize {
        let source = y * width as usize * 4;
        let target = ((y + icon.uv[1] as usize) * dimensions[0] as usize + icon.uv[0] as usize) * 4;
        assert!(
            cape.rgba8[source..source + width as usize * 4]
                == page.pixels()[target..target + width as usize * 4],
            "cape row {y} preserves its original texels"
        );
    }
}
