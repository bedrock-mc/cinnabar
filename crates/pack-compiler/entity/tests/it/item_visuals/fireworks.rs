use super::*;

#[test]
fn firework_mask_is_opaque_and_defaults_to_black_without_tinting_shared_items() {
    let pack = item_pack(false);
    write(pack.path(), "textures/item_texture.json", br#"{"texture_data":{"fireworks_charge":{"textures":"textures/items/shared"},"apple":{"textures":"textures/items/shared"}}}"#);
    let image = image::RgbaImage::from_fn(3, 1, |x, _| {
        image::Rgba(match x {
            0 => [200, 100, 50, 4],
            1 => [255, 255, 255, 255],
            _ => [0, 0, 0, 0],
        })
    });
    image
        .save(pack.path().join("textures/items/shared.png"))
        .unwrap();
    let compiled = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    let icons = RuntimeIconCatalog::decode(&compiled.bytes).unwrap();
    let star = icons
        .lookup("minecraft:firework_star", 0)
        .expect("firework star icon");
    assert_eq!(
        star.rgba8.as_ref(),
        &[196, 98, 49, 255, 0, 0, 0, 255, 0, 0, 0, 0]
    );
    let apple = icons.lookup("minecraft:apple", 0).unwrap();
    assert_eq!(apple.rgba8.as_ref(), image.as_raw());
    let raw = icons
        .sources()
        .unwrap()
        .iter()
        .find(|source| source.path.as_ref() == "textures/items/shared.png")
        .map(|source| &icons.sprites()[source.sprite as usize])
        .unwrap();
    assert_eq!(raw.rgba8.as_ref(), image.as_raw());
}
