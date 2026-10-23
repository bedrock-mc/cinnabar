use super::*;
use crate::{
    Originals,
    images::{OreUiImages, OreUiSprite, panorama::CREATE_WORLD_KEY},
};
use launcher::local_worlds::world::{Difficulty, Generator, World};
use std::{collections::HashMap, sync::Arc};

#[test]
fn panorama_artwork_world_preview_yields_to_an_edited_world_thumbnail() {
    let originals = Originals::new(
        10,
        OreUiImages {
            sprites: Arc::new(HashMap::from([(
                CREATE_WORLD_KEY.into(),
                OreUiSprite {
                    page: 0,
                    bounds: [0, 0, 640, 360],
                },
            )])),
            ..Default::default()
        },
    );
    let mut view = MenuView::new(true, "Fixture".into());
    view.local.edited = Some(World {
        id: "fixture".into(),
        name: "Fixture".into(),
        game_mode: bridge::GameMode::Survival,
        generator: Generator::Normal,
        difficulty: Difficulty::Normal,
        backend: bridge::Backend::Dragonfly,
        seed: 0,
        options: Default::default(),
        options_unavailable: false,
        created_unix: 0,
        last_played_unix: 0,
        size_bytes: 0,
    });
    let artwork = HashMap::from([(
        thumbnail_key("fixture"),
        ui::IconRef {
            page: 30,
            uv: [0, 0, 320, 180],
            glint: false,
        },
    )]);
    for (route, available, expected) in [
        (Screen::Create, true, 10),
        (Screen::Edit, true, 30),
        (Screen::Edit, false, 10),
    ] {
        let (mut nodes, mut next, mut layouts) =
            (Vec::new(), 1, ui::TextLayoutCache::new(128, 1024 * 1024));
        let font = ui_engine::test_support::fixture_font();
        let metrics = ui_engine::text_metrics::TextMetrics::for_viewport(
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
            Some(2),
        );
        let mut canvas = Canvas::new(
            &mut nodes,
            &mut next,
            &mut layouts,
            &font,
            metrics,
            0,
            Some(&originals),
        );
        canvas.artwork = available.then_some(&artwork);
        draw(&mut canvas, &view, [0.0, 0.0, 320.0, 720.0], route, false).unwrap();

        drop(canvas);
        let pages: Vec<_> = nodes
            .iter()
            .filter_map(|node| match node.visual() {
                ui::UiVisual::Sprite { texture_page, .. } => Some(*texture_page),
                _ => None,
            })
            .collect();
        assert!(pages.contains(&expected));
        assert!(!pages.contains(&if expected == 10 { 30 } else { 10 }));
    }
}

#[test]
fn panorama_artwork_world_preview_without_available_art_remains_plain() {
    let view = MenuView::new(true, "Fixture".into());
    let (_, _, nodes) = crate::review_tests::paint(HashMap::new(), |canvas| {
        draw(
            canvas,
            &view,
            [0.0, 0.0, 320.0, 720.0],
            Screen::Create,
            false,
        )
        .unwrap();
    });
    assert!(
        !nodes
            .iter()
            .any(|node| matches!(node.visual(), ui::UiVisual::Sprite { .. }))
    );
}
