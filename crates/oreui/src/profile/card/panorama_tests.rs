use super::*;
use crate::{
    Originals,
    images::{
        OreUiImages, OreUiSprite,
        panorama::{VIEWS, profile_key},
    },
};
use std::{collections::HashMap, sync::Arc};

#[test]
fn panorama_artwork_profile_uses_account_banner_but_featured_image_wins() {
    let mut view = MenuView::new(true, "Fixture".into());
    let originals = Originals::new(
        10,
        OreUiImages {
            sprites: Arc::new(
                VIEWS
                    .iter()
                    .enumerate()
                    .map(|(index, view)| {
                        (
                            view.key.into(),
                            OreUiSprite {
                                page: index as u16,
                                bounds: [0, 0, 640, 360],
                            },
                        )
                    })
                    .collect(),
            ),
            ..Default::default()
        },
    );
    for narrow in [false, true] {
        for account in 0..VIEWS.len() - 1 {
            view.feeds.profile.xuid = account.to_string();
            let banner_page = 10
                + originals
                    .sprite(profile_key(&view.feeds.profile.xuid))
                    .unwrap()
                    .page;
            for featured in [
                None,
                Some(IconRef {
                    page: 30,
                    uv: [0, 0, 160, 90],
                    glint: false,
                }),
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
                draw(
                    &mut canvas,
                    &view,
                    [0.0, 0.0, 600.0, 700.0],
                    None,
                    None,
                    featured,
                    narrow,
                )
                .unwrap();

                drop(canvas);
                let pages: Vec<_> = nodes
                    .iter()
                    .filter_map(|node| match node.visual() {
                        ui::UiVisual::Sprite { texture_page, .. } => Some(*texture_page),
                        _ => None,
                    })
                    .collect();
                assert!(pages.contains(&featured.map_or(banner_page, |icon| icon.page)));
                if featured.is_some() {
                    assert!(!pages.contains(&banner_page));
                }
            }
        }
    }
}

#[test]
fn panorama_artwork_profile_without_available_art_remains_plain() {
    let view = MenuView::new(true, "Fixture".into());
    let (_, _, nodes) = crate::review_tests::paint(HashMap::new(), |canvas| {
        draw(
            canvas,
            &view,
            [0.0, 0.0, 600.0, 700.0],
            None,
            None,
            None,
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
