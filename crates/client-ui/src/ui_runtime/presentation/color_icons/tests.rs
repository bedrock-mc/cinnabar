use super::*;
use sha2::Digest;
use std::sync::Arc;

/// Constructs distinct processed and raw icon regions with original fixture artwork.
fn presentation() -> UiPresentationRuntime {
    let mut compiled = pack_entity_compiler::compile_entity_pack(vec![
        ("entity/item.entity.json".into(), br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"fixture:item","geometry":{"default":"geometry.item"},"render_controllers":["controller.render.item"]}}}"#.to_vec()),
        ("models/entity/item.geo.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.item"},"bones":[{"name":"root"}]}]}"#.to_vec()),
        ("render_controllers/item.json".into(), br#"{"format_version":"1.8.0","render_controllers":{"controller.render.item":{"geometry":"Geometry.default"}}}"#.to_vec()),
    ]).unwrap().unwrap().assets;
    let mut sources = compiled.sources.into_vec();
    let source = sources.len() as u32;
    sources.push(assets::EntityAssetSource {
        path: "textures/items/fixture.png".into(),
        source_bytes: 4,
        source_sha256: sha2::Sha256::digest(b"mask").into(),
    });
    compiled.sources = sources.into();
    let defining_source = compiled
        .sources
        .iter()
        .position(|source| source.path.as_ref() == "entity/item.entity.json")
        .unwrap() as u32;
    compiled.item_visuals = vec![assets::ItemVisualDefinition {
        key: assets::ItemVisualKey {
            identifier: "minecraft:firework_star".into(),
            metadata: 0,
        },
        source: defining_source,
        route: assets::ItemVisualDefinitionRoute::Sprite {
            texture: assets::ItemTextureReference { source, variant: 0 },
        },
        grip: assets::item::grip::ItemGrip::Unresolved,
        glint: assets::item::glint::ItemGlint::Unresolved,
        fuel: assets::item::fuel::ItemFuel::Unresolved,
        durability: assets::item::durability::ItemDurability::Unresolved,
        armour: assets::item::armour::ItemArmour::Unresolved,
        first_person: assets::ItemDisplayTransform::identity(),
        third_person: assets::ItemDisplayTransform::identity(),
        dropped: assets::ItemDisplayTransform::identity(),
    }]
    .into();
    let entities =
        assets::RuntimeEntityAssets::from_compiled(compiled.with_unresolved_item_use()).unwrap();
    let sprites = [
        assets::IconSprite {
            width: 1,
            height: 1,
            rgba8: vec![0, 0, 0, 255].into(),
        },
        assets::IconSprite {
            width: 1,
            height: 1,
            rgba8: vec![255; 4].into(),
        },
    ];
    let icons = assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog_with_sources(
            entities.source_manifest_sha256(),
            &sprites,
            &[assets::IconEntry {
                identifier: "minecraft:firework_star".into(),
                metadata: 0,
                sprite: 0,
            }],
            &[],
            &[],
            &[assets::icon::IconSource {
                path: "textures/items/fixture.png".into(),
                sprite: 1,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::new(icons),
    )
    .unwrap();
    presentation.gui_models.color_masks = prepare(
        &entities,
        presentation.icon_catalog.as_deref().unwrap(),
        presentation.icon_refs.as_deref().unwrap(),
    );
    presentation
}

#[test]
fn dyed_firework_stacks_use_raw_artwork_and_preserve_session_overrides() {
    use crate::ui_runtime::presentation::publish::item_icons::test_support::stack_icon;
    let mut presentation = presentation();
    let item = crate::test_support::fixture_item("minecraft:firework_star");
    let runtime = crate::ui_runtime::UiRuntime::new(1);
    for rgb in [0, 0x112233, 0xff8000, 0xffffff] {
        let mut nbt = vec![0xff, 0xff, 1, 0x0a, 0, 0, 3];
        nbt.extend_from_slice(&11u16.to_le_bytes());
        nbt.extend_from_slice(b"customColor");
        nbt.extend_from_slice(&u32::to_le_bytes(rgb));
        nbt.push(0);
        let stack = protocol::NetworkItemStack {
            network_id: 1,
            count: 1,
            extra_data: nbt.into(),
            ..protocol::NetworkItemStack::empty()
        };
        let icon = stack_icon(&runtime, &presentation, &stack, item.clone(), None).unwrap();
        assert_eq!(
            icon.color_mask,
            Some([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8])
        );
        assert_ne!(
            icon.uv,
            presentation
                .item_icon("minecraft:firework_star", 0)
                .unwrap()
                .uv
        );
    }
    let icons = Arc::new(super::super::session_icons::SessionIcons {
        icons: vec![super::super::session_icons::SessionIcon {
            identifier: "minecraft:firework_star".into(),
            metadata: 0,
            width: 1,
            height: 1,
            rgba8: vec![25, 50, 75, 255].into(),
        }],
        ..Default::default()
    });
    super::super::session_icons::observe(&mut presentation, Some(&icons));
    assert_eq!(
        presentation.color_mask_icon("minecraft:firework_star", 0, 0xff0000),
        None
    );
}
