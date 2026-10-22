use super::*;
use launcher::{
    local_worlds::{
        Screen,
        world::{THUMBNAIL_SIZE, World, WorldThumbnail, thumbnail_key},
    },
    menu::{LocalWorldCard, MenuAction, MenuScreen, MenuView},
};

/// Supplies more previews than the bounded atlas can hold, using one encoded image fixture.
fn catalog() -> MenuView {
    let pixels = image::RgbImage::from_pixel(
        THUMBNAIL_SIZE[0],
        THUMBNAIL_SIZE[1],
        image::Rgb([0, 0, 255]),
    );
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&pixels)
        .unwrap();
    let jpeg: Arc<[u8]> = jpeg.into();
    let mut view = MenuView::new(true, "Preview".into());
    view.screen = MenuScreen::Play;
    view.local_worlds = (0..MAX_ARTWORKS + 2)
        .map(|index| LocalWorldCard {
            thumbnail: Some(WorldThumbnail::new(
                &format!("{index:016x}"),
                Arc::clone(&jpeg),
            )),
            ..Default::default()
        })
        .collect();
    view
}

/// A visible row target, already clipped by the worlds list's scroll viewport.
fn row(index: usize) -> (MenuAction, ui::UiRect) {
    (
        MenuAction::PlayLocalWorld(index),
        ui::UiRect::new(
            ui::UiPoint::new(0.0, 0.0).unwrap(),
            ui::UiPoint::new(100.0, 50.0).unwrap(),
        )
        .unwrap(),
    )
}

#[test]
fn first_world_list_paint_requests_a_bounded_preview_batch_without_row_geometry() {
    for count in [0, 1, FIRST_BATCH, FIRST_BATCH + 1] {
        let mut view = catalog();
        view.local_worlds.truncate(count);
        let images = view_images(&view, &[]);
        let expected: Vec<_> = view
            .local_worlds
            .iter()
            .take(FIRST_BATCH)
            .filter_map(|world| world.thumbnail.as_ref())
            .map(|thumbnail| thumbnail.key.clone())
            .collect();
        assert_eq!(
            images.into_iter().map(|(key, _)| key).collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn scrolling_past_the_atlas_budget_installs_the_revealed_world_preview() {
    let view = catalog();
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    presentation.menu_hit_targets = vec![row(0)];
    presentation.sync_menu_artwork_view(&view);
    presentation.finish_menu_artwork();
    let first = thumbnail_key(&format!("{:016x}", 0));
    let last_index = view.local_worlds.len() - 1;
    let last = thumbnail_key(&format!("{last_index:016x}"));
    assert!(presentation.menu_artwork_icon(&first).is_some());
    assert!(presentation.menu_artwork_icon(&last).is_none());
    presentation.menu_hit_targets = vec![row(last_index - 1), row(last_index)];
    presentation.sync_menu_artwork_view(&view);
    presentation.finish_menu_artwork();
    assert!(presentation.menu_artwork_icon(&last).is_some());
    assert!(presentation.menu_artwork_icon(&first).is_none());
    assert_eq!(presentation.menu_artwork_set.images.len(), 2);
}

#[test]
fn the_edited_world_gets_atlas_space_from_play_and_storage() {
    let mut view = catalog();
    let last_index = view.local_worlds.len() - 1;
    let id = format!("{last_index:016x}");
    view.local.screen = Screen::Edit;
    view.local.edited = Some(World {
        options: Default::default(),
        id: id.clone(),
        name: "Preview".into(),
        game_mode: bridge::GameMode::Survival,
        generator: Default::default(),
        difficulty: Default::default(),
        backend: bridge::Backend::Dragonfly,
        seed: 0,
        created_unix: 0,
        last_played_unix: 0,
        options_unavailable: false,
        size_bytes: 0,
    });
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    // Row geometry from the old list must not take space from the edited preview.
    presentation.menu_hit_targets = (0..MAX_ARTWORKS).map(row).collect();
    for screen in [MenuScreen::Play, MenuScreen::Settings] {
        view.screen = screen;
        presentation.sync_menu_artwork_view(&view);
        presentation.finish_menu_artwork();
        assert!(
            presentation
                .menu_artwork_icon(&thumbnail_key(&id))
                .is_some()
        );
        assert_eq!(presentation.menu_artwork_set.images.len(), 1);
    }
    view.local.screen = Screen::ConfirmDelete;
    assert_eq!(view_images(&view, &presentation.menu_hit_targets).len(), 1);
    view.local.screen = Screen::Create;
    assert!(view_images(&view, &presentation.menu_hit_targets).is_empty());
}
