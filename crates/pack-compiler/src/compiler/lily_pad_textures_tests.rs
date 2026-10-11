use super::*;

/// Creates a page filled with copies of one canonical mip chain.
fn repeated_page(chain: &[TextureMip], layers: usize) -> TexturePage {
    TexturePage::new(TextureArray {
        layers: layers as u32,
        mips: chain
            .iter()
            .map(|mip| TextureMip {
                size: mip.size,
                rgba8: mip.rgba8.repeat(layers).into_boxed_slice(),
            })
            .collect(),
    })
}

#[test]
fn lily_pad_copies_keep_pages_device_compatible_and_fail_at_total_capacity() {
    let base = [80, 100, 120, 255].repeat((TILE_SIZE * TILE_SIZE) as usize);
    let chain = assets::build_texture_mip_chain(base.into_boxed_slice(), TILE_SIZE).unwrap();
    let full = repeated_page(&chain, MAX_CARRIER_TEXTURE_LAYERS);
    let mut pages = vec![full.clone()];
    let tinted = copy_chain(&pages, TextureRef::DIAGNOSTIC, [128, 255, 255]).unwrap();
    let reference = append_layer(&mut pages, tinted.clone()).unwrap();
    assert_eq!(reference, TextureRef::new(1, 0).unwrap());
    assert_eq!(pages.len(), MAX_TEXTURE_PAGES);
    assert_eq!(pages[0], full);
    assert_eq!(pages[1].texture.layers, 1);
    assert_eq!(pages[1].texture.mips, tinted);

    let mut pages = vec![full; MAX_TEXTURE_PAGES];
    let original = pages.clone();
    assert!(append_layer(&mut pages, tinted).is_err());
    assert_eq!(pages, original);
}
