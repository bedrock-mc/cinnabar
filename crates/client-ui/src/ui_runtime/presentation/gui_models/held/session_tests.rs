use super::*;
use crate::test_support::fixture_hud;
use crate::ui_runtime::presentation::{
    SessionIcon, SessionIcons, UiPresentationRuntime, session_icons,
};
use ui_engine::test_support::fixture_font;

const ITEM: &str = "minecraft:diamond_sword";

/// Original asymmetric sword art differs from the base sprite in size, color and alpha edges.
fn sword(metadata: u32, color: [u8; 3]) -> SessionIcon {
    let [width, height] = [8, 12];
    let rgba8 = (0..width * height)
        .flat_map(|pixel| {
            let [x, y] = [pixel % width, pixel / width];
            let opaque = (x == 3 && y < 10) || (y == 8 && (1..7).contains(&x));
            [color[0], color[1], color[2], if opaque { 255 } else { 0 }]
        })
        .collect::<Vec<_>>()
        .into();
    SessionIcon {
        identifier: Arc::from(ITEM),
        metadata,
        width,
        height,
        rgba8,
    }
}

/// Uses original carried sprites and hand bones without installed game carriers.
fn presentation() -> UiPresentationRuntime {
    let (entities, icons) = tests::sprite_catalog(ITEM, ItemVisualDefinitionRoute::Missing);
    let mut presentation =
        UiPresentationRuntime::with_hud_and_icons(fixture_font(), fixture_hud(), Arc::new(icons))
            .unwrap();
    presentation
        .set_gui_models(&assets::RuntimeAssets::diagnostic(), &entities)
        .unwrap();
    presentation.set_player_preview_skin(Some(&[255; 64 * 64 * 4]), Default::default());
    presentation
}

#[test]
fn preview_held_sword_uses_session_icon_texels_and_alpha_edges() {
    let mut presentation = presentation();
    let art = sword(0, [12, 38, 96]);
    let expected = held_sprite_vertices(
        art.width as usize,
        art.height as usize,
        &art.rgba8,
        [0.0, 0.0, 1.0, 1.0],
    )
    .unwrap();
    let icons = Arc::new(SessionIcons {
        icons: vec![art],
        ..Default::default()
    });
    session_icons::observe(&mut presentation, Some(&icons));
    presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
    let hotbar = presentation.item_icon(ITEM, 0).unwrap();
    let mesh = presentation.gui_player_mesh().unwrap();
    let held = mesh.batches().last().unwrap();
    assert_eq!(
        held.texture_page, hotbar.page,
        "preview and hotbar must sample the active pack"
    );
    assert_eq!(
        held.index_range.len(),
        expected.len(),
        "the pack alpha mask must determine extrusion edges"
    );
    for vertex in &mesh.vertices()[held.index_range.start as usize..held.index_range.end as usize] {
        assert!(vertex.uv[0] >= f32::from(hotbar.uv[0]) && vertex.uv[0] <= f32::from(hotbar.uv[2]));
        assert!(vertex.uv[1] >= f32::from(hotbar.uv[1]) && vertex.uv[1] <= f32::from(hotbar.uv[3]));
    }
}

/// Publishes original artwork through the production session page.
fn install(presentation: &mut UiPresentationRuntime, icons: Vec<SessionIcon>) -> Arc<SessionIcons> {
    let icons = Arc::new(SessionIcons {
        icons,
        ..Default::default()
    });
    session_icons::observe(presentation, Some(&icons));
    icons
}

#[test]
fn preview_session_variants_and_both_hands_share_inventory_regions_and_cached_edges() {
    let mut presentation = presentation();
    let icons = install(
        &mut presentation,
        vec![sword(0, [12, 38, 96]), sword(7, [220, 80, 12])],
    );
    for metadata in [7, 29] {
        presentation.set_player_preview_gear([None; 4], Some((ITEM, metadata)));
        presentation.player_preview_gear.hands[1] =
            presentation.player_preview_gear.hands[0].clone();
        let icon = presentation.item_icon(ITEM, metadata).unwrap();
        let (_, first) = presentation
            .session_icons
            .held_sprite(ITEM, metadata)
            .unwrap();
        let textures = Arc::clone(&presentation.textures);
        for view in [
            player_preview::PreviewView::Hud,
            player_preview::PreviewView::Live {
                offset: [4.0, -2.0],
            },
        ] {
            presentation.player_preview_view = view;
            let mesh = presentation.gui_player_mesh().unwrap();
            let held = &mesh.batches()[mesh.batches().len() - 2..];
            for batch in held {
                assert_eq!(batch.texture_page, icon.page);
                assert_eq!(batch.index_range.len(), first.len());
                for index in batch.index_range.clone() {
                    let vertex = &mesh.vertices()[mesh.indices()[index as usize] as usize];
                    assert!(
                        vertex.uv[0] >= f32::from(icon.uv[0])
                            && vertex.uv[0] <= f32::from(icon.uv[2])
                    );
                    assert!(
                        vertex.uv[1] >= f32::from(icon.uv[1])
                            && vertex.uv[1] <= f32::from(icon.uv[3])
                    );
                }
            }
        }
        session_icons::observe(&mut presentation, Some(&icons));
        let (_, again) = presentation
            .session_icons
            .held_sprite(ITEM, metadata)
            .unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        assert!(Arc::ptr_eq(&textures, &presentation.textures));
        let pixels = &presentation
            .session_icons
            .sprite(ITEM, metadata)
            .unwrap()
            .rgba8;
        let expected = if metadata == 7 {
            [220, 80, 12]
        } else {
            [12, 38, 96]
        };
        assert_eq!(&pixels[..3], expected);
    }
}

#[test]
fn preview_session_replacement_removal_and_transparency_retire_old_meshes() {
    let mut presentation = presentation();
    presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
    let base = presentation.gui_player_mesh().unwrap();
    let base_icon = presentation.item_icon(ITEM, 0).unwrap();
    install(&mut presentation, vec![sword(0, [12, 38, 96])]);
    let (_, first) = presentation.session_icons.held_sprite(ITEM, 0).unwrap();
    let first = Arc::downgrade(&first);
    let changed = presentation.gui_player_mesh().unwrap();
    let body = base.batches()[0].index_range.clone();
    for (base, changed) in base.vertices()[body.start as usize..body.end as usize]
        .iter()
        .zip(&changed.vertices()[body.start as usize..body.end as usize])
    {
        assert_eq!(base.position, changed.position);
        assert_eq!(base.uv, changed.uv);
        assert_eq!(base.color, changed.color);
        assert_eq!(base.model_light, changed.model_light);
    }
    let mut transparent = sword(0, [220, 80, 12]);
    transparent.rgba8.fill(0);
    install(&mut presentation, vec![transparent]);
    assert!(first.upgrade().is_none());
    assert!(presentation.session_icons.held_sprite(ITEM, 0).is_none());
    let empty = presentation.gui_player_mesh().unwrap();
    assert_eq!(empty.batches().len() + 1, base.batches().len());
    assert_eq!(
        empty.vertices().len() + (base.batches().last().unwrap().index_range.len()),
        base.vertices().len()
    );
    session_icons::observe(&mut presentation, None);
    assert_eq!(presentation.item_icon(ITEM, 0), Some(base_icon));
    let restored = presentation.gui_player_mesh().unwrap();
    assert_eq!(restored.vertices(), base.vertices());
    assert_eq!(restored.indices(), base.indices());
}

#[test]
fn preview_custom_session_identity_and_cpu_fallback_use_admitted_pixels() {
    let mut presentation = presentation();
    let identifier = "fixture:preview_sword";
    let mut art = sword(0, [220, 80, 12]);
    art.identifier = identifier.into();
    install(&mut presentation, vec![art]);
    assert!(
        presentation
            .icon_catalog
            .as_ref()
            .unwrap()
            .lookup(identifier, 0)
            .is_none()
    );
    presentation.set_player_preview_gear([None; 4], Some((identifier, 42)));
    let mesh = presentation.gui_player_mesh().unwrap();
    assert_eq!(
        mesh.batches().last().unwrap().texture_page,
        presentation.item_icon(identifier, 42).unwrap().page
    );
    let cpu = presentation.player_preview_gear.held.as_ref().unwrap();
    assert_eq!([cpu.width, cpu.height], [8, 12]);
    assert_eq!(&cpu.rgba[..3], [220, 80, 12]);
    session_icons::observe(&mut presentation, None);
    presentation.set_player_preview_gear([None; 4], Some((identifier, 42)));
    assert!(presentation.player_preview_gear.held.is_none());
    assert_eq!(
        presentation.gui_player_mesh().unwrap().batches().len() + 1,
        mesh.batches().len()
    );
}

#[test]
fn preview_session_thumbnail_does_not_replace_block_or_authored_held_geometry() {
    let mut presentation = presentation();
    let source = IconRef {
        page: 3,
        uv: [0, 0, 64, 48],
        glint: true,
    };
    let model = block(source, [[0.0; 3]; 2]);
    presentation.gui_models.held.insert(
        ItemVisualKey {
            identifier: ITEM.into(),
            metadata: 0,
        },
        HeldModel::ready(model),
    );
    presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
    let base = presentation.gui_player_mesh().unwrap();
    install(&mut presentation, vec![sword(0, [220, 80, 12])]);
    let changed = presentation.gui_player_mesh().unwrap();
    assert_eq!(base.vertices(), changed.vertices());
    assert_eq!(changed.batches().last().unwrap().texture_page, source.page);
    assert!(
        changed
            .vertices()
            .iter()
            .any(|vertex| vertex.style_flags & u16::from(ui::UI_STYLE_GLINT) != 0)
    );
}

#[test]
fn preview_rejects_invalid_duplicate_session_sprite_before_the_valid_source() {
    let mut presentation = presentation();
    let mut bad = sword(0, [1, 2, 3]);
    bad.rgba8 = Box::from([]);
    install(
        &mut presentation,
        vec![bad, sword(0, [12, 38, 96]), sword(0, [220, 80, 12])],
    );
    assert_eq!(
        &presentation.session_icons.sprite(ITEM, 0).unwrap().rgba8[..3],
        [12, 38, 96]
    );
    presentation.set_player_preview_gear([None; 4], Some((ITEM, 0)));
    assert_eq!(
        presentation
            .gui_player_mesh()
            .unwrap()
            .batches()
            .last()
            .unwrap()
            .texture_page,
        presentation.item_icon(ITEM, 0).unwrap().page
    );
}
