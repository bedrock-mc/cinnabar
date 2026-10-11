use crate::ui_runtime::{
    oreui_assets::{OreUiImages, OreUiSprite},
    presentation::{
        TextMetrics, forms::oreui::paint::Originals, forms::oreui::review_tests::solids,
        tests::fixture_font,
    },
};
use std::{collections::HashMap, sync::Arc};
use {super::*, launcher::menu::MenuView, launcher::menu::auth::AuthState, ui::IconRef};

fn paint(view: &MenuView, gamerpic: Option<IconRef>) -> Vec<ui::UiNode> {
    let sprites = Arc::new(HashMap::from([(
        SETTINGS_ICONS[8].to_owned(),
        OreUiSprite {
            page: 3,
            bounds: [10, 20, 34, 44],
        },
    )]));
    let originals = Originals {
        page: 10,
        images: OreUiImages {
            pages: Vec::new(),
            sprites: sprites.clone(),
            loading_frames: Default::default(),
            animations: Default::default(),
            source: None,
        },
        sprites,
        masks: HashMap::new(),
        loading_frames: Default::default(),
        animations: Default::default(),
    };
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let (mut nodes, mut next, mut layouts) = (Vec::new(), 1, ui::TextLayoutCache::new(8, 4096));
    let mut canvas = Canvas::new(
        &mut nodes,
        &mut next,
        &mut layouts,
        &font,
        metrics,
        0,
        Some(&originals),
    );
    assert!(draw(&mut canvas, view, [100.0, 100.0, 119.2, 119.2], gamerpic).unwrap());
    nodes
}

fn picture() -> IconRef {
    IconRef {
        page: 7,
        uv: [1, 2, 9, 10],
        glint: false,
    }
}

fn pages(nodes: &[ui::UiNode]) -> Vec<u16> {
    nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Sprite { texture_page, .. } => Some(*texture_page),
            _ => None,
        })
        .collect()
}

#[test]
fn signed_in_account_draws_the_actual_gamerpic_with_a_neutral_border() {
    let mut view = MenuView::new(true, "Player".into());
    view.auth_state = AuthState::Authenticated;
    view.feeds.profile.picture_path = "cached-profile-picture.png".into();
    let nodes = paint(&view, Some(picture()));
    assert_eq!(pages(&nodes), [picture().page]);
    let border = solids(&nodes);
    assert_eq!(border.len(), 4);
    assert!(
        border
            .iter()
            .all(|(_, color)| *color == theme::NEUTRAL.border)
    );
    assert!(nodes.iter().any(|node| matches!(node.visual(),
        ui::UiVisual::Sprite { uv, .. } if *uv == picture().uv)));
}

#[test]
fn cached_picture_without_a_signed_in_account_uses_the_unmodified_native_icon() {
    let mut view = MenuView::new(true, "Player".into());
    view.feeds.profile.picture_path = "cached-profile-picture.png".into();
    for auth in [
        AuthState::SignedOut,
        AuthState::Checking,
        AuthState::Failed("offline".into()),
    ] {
        view.auth_state = auth;
        let nodes = paint(&view, Some(picture()));
        assert_eq!(pages(&nodes), [13]);
        assert!(solids(&nodes).is_empty());
        assert!(nodes.iter().all(|node| matches!(node.visual(),
            ui::UiVisual::Sprite { color, .. } if *color == [255; 4])));
    }
    view.auth_state = AuthState::Authenticated;
    view.feeds.profile.picture_path.clear();
    assert_eq!(pages(&paint(&view, Some(picture()))), [13]);
}

#[test]
fn a_pending_gamerpic_keeps_its_border_without_substituting_an_account_head() {
    let mut view = MenuView::new(true, "Player".into());
    view.auth_state = AuthState::Authenticated;
    view.feeds.profile.picture_path = "pending-profile-picture.png".into();
    let nodes = paint(&view, None);
    assert!(pages(&nodes).is_empty());
    assert_eq!(solids(&nodes).len(), 4);
}

#[test]
fn a_wide_gamerpic_covers_the_square_icon_around_its_center() {
    let mut view = MenuView::new(true, "Player".into());
    view.auth_state = AuthState::Authenticated;
    view.feeds.profile.picture_path = "wide-profile-picture.png".into();
    let icon = IconRef {
        uv: [1, 2, 13, 10],
        ..picture()
    };
    let nodes = paint(&view, Some(icon));
    assert!(nodes.iter().any(|node| matches!(node.visual(),
        ui::UiVisual::Sprite { uv, .. } if *uv == [3, 2, 11, 10])));
}
