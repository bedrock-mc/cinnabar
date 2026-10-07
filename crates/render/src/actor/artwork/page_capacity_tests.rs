use super::*;

#[test]
fn pack_artwork_retains_more_than_byte_sized_distinct_texture_pages() {
    let textures = (0..usize::from(u8::MAX) + 8)
        .map(|index| {
            let height = u16::try_from(index + 1).unwrap();
            let pixel = [index as u8, (index >> 8) as u8, 7, 255];
            let rgba8 = pixel.repeat(usize::from(height));
            assets::ActorTexture {
                source: index as u32,
                width: 1,
                height,
                pixel_sha256: Sha256::digest(&rgba8).into(),
                rgba8: rgba8.into(),
            }
        })
        .collect::<Vec<_>>();
    let bindings = textures
        .iter()
        .enumerate()
        .map(|(index, _)| assets::ActorArtworkBinding {
            rig: index as u32,
            geometry_candidate: index as u32,
            entity_symbol: index as u32,
            geometry: index as u32,
            render_controller: 0,
            texture: index as u32,
            material: "entity".into(),
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
        })
        .collect::<Vec<_>>();
    let pages = ActorArtworkPages::default().with_pack_artwork(&textures, &bindings);
    assert_eq!(pages.rejected_bindings(), 0);
    assert_eq!(pages.pages().len(), textures.len());
    for (index, texture) in textures.iter().enumerate() {
        let rig = render_model::pack_rig_id(index as u32);
        let location = pages
            .route(rig)
            .expect("valid pack binding retains its page");
        assert!(pages.valid(rig, location));
        assert_eq!(pages.variant_location(rig, index as u32), Some(location));
        let page = &pages.pages()[usize::from(location.page()) - 1];
        assert_eq!(page.dimensions(), (texture.width, texture.height));
        assert_eq!(page.pixels(), texture.rgba8.as_ref());
    }
}
