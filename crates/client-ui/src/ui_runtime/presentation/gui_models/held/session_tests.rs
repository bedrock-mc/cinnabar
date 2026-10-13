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
