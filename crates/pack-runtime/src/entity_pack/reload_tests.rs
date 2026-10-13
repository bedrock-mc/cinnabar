//! Texture-only reloads update artwork paths without introducing pack bindings.

use super::*;

const ITEM: &str = "fixture:chestplate";
const TEXTURE: &str = "textures/items/fixture_armor";
/// A validated startup artwork table with no inherited pack bindings.
fn base_catalog() -> Arc<assets::RuntimeEquipmentCatalog> {
    Arc::new(
        assets::RuntimeEquipmentCatalog::from_parts(
            [1; 32],
            Vec::new(),
            vec![assets::EquipmentTexture {
                identifier: TEXTURE.into(),
                width: 4,
                height: 2,
                rgba8: Arc::from([10, 20, 30, 255].repeat(8)),
            }],
        )
        .unwrap(),
    )
}

/// A stack with the same pack and texture names but independently authored pixels.
fn texture_stack(color: [u8; 4]) -> Arc<resource_pack::ValidatedPackStack> {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(4, 2, image::Rgba(color))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    crate::test_support::stack(&[(&format!("{TEXTURE}.png"), png.get_ref())])
}

/// Compiles on the same cache used by session preparation, retaining its dependency reads.
fn resolve(
    cache: &EntityCache,
    stack: &Arc<resource_pack::ValidatedPackStack>,
    catalog: &Arc<assets::RuntimeEquipmentCatalog>,
) -> (
    Option<Arc<SessionEntityPack>>,
    crate::reload_diff::Dependencies,
) {
    let mut dependencies = Default::default();
    let pack = crate::reload_diff::compile(
        crate::reload_diff::Subscriber::Entities,
        stack,
        &mut dependencies,
        |view| {
            cache
                .get_or_compile(
                    EntityInputs {
                        stack: crate::stack_fingerprint(stack),
                        vanilla: None,
                        equipment: Some(catalog.clone()),
                        vanilla_pack_dir: None,
                    },
                    view,
                    None,
                    &|| false,
                    super::super::compile_encoded,
                )
                .expect("the compile was not cancelled")
        },
    );
    (pack, dependencies)
}

#[test]
fn a_texture_only_reload_changes_artwork_for_the_same_equipped_item() {
    let (catalog, cache) = (base_catalog(), EntityCache::default());
    let equipped = ITEM;
    let first_stack = texture_stack([21, 43, 65, 255]);
    let (first, reads) = resolve(&cache, &first_stack, &catalog);
    let first = first.expect("texture-only armour retains artwork without pack bindings");
    let first_catalog = first.equipment.as_ref().unwrap();
    assert!(first_catalog.binding(equipped).is_none());
    let first_pixels = first_catalog.texture(TEXTURE).unwrap();
    assert_eq!(&first_pixels.rgba8[..4], &[21, 43, 65, 255]);
    let (unchanged, _) = resolve(&cache, &first_stack, &catalog);
    assert!(Arc::ptr_eq(&first, &unchanged.unwrap()));

    let next_stack = texture_stack([87, 109, 131, 255]);
    let changes = crate::reload_diff::Changes::between(
        &next_stack,
        Some((
            &resource_pack::PackAdmission::Validated(first_stack),
            &reads,
        )),
    );
    assert!(changes.entities, "changed pixels must refresh equipment");
    let (next, _) = resolve(&cache, &next_stack, &catalog);
    let next = next.unwrap();
    assert!(!Arc::ptr_eq(&first, &next));
    let next_catalog = next.equipment.as_ref().unwrap();
    assert!(next_catalog.binding(equipped).is_none());
    assert_ne!(
        next_catalog.source_manifest_sha256(),
        first_catalog.source_manifest_sha256()
    );
    assert_eq!(
        &next_catalog.texture(TEXTURE).unwrap().rgba8[..4],
        &[87, 109, 131, 255]
    );
    assert_eq!(&first_pixels.rgba8[..4], &[21, 43, 65, 255]);
    let empty = crate::test_support::stack(&[]);
    let removed = crate::reload_diff::Changes::between(
        &empty,
        Some((&resource_pack::PackAdmission::Validated(next_stack), &reads)),
    );
    assert!(removed.entities);
    assert!(resolve(&cache, &empty, &catalog).0.is_none());
}

#[test]
fn artwork_only_layers_do_not_create_attachable_rigs() {
    let catalog = base_catalog();
    let stack = texture_stack([21, 43, 65, 255]);
    let pack = resolve(&EntityCache::default(), &stack, &catalog)
        .0
        .unwrap();
    assert!(pack.assets.rig_bindings().is_empty());
    assert!(pack.assets.symbols().is_empty());
    assert!(pack.equipment.as_ref().unwrap().bindings().is_empty());
    let encoded = pack.encode(&pack.assets.encode().unwrap()).unwrap();
    let restored = SessionEntityPack::decode(&encoded).unwrap();
    assert!(restored.assets.rig_bindings().is_empty());
    assert!(restored.assets.symbols().is_empty());
    let artwork = restored.equipment.unwrap();
    assert!(artwork.bindings().is_empty());
    assert_eq!(
        &artwork.texture(TEXTURE).unwrap().rgba8[..4],
        &[21, 43, 65, 255]
    );
}
