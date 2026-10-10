use crate::ui_runtime::presentation::tests::fixture_font;
use launcher::menu::{MenuGameCard, ServerDetails};
use {
    super::*,
    launcher::menu::{MenuServerCard, MenuView, PingInfo},
    ui::IconRef,
};

fn card(name: &str, address: &str) -> MenuServerCard {
    MenuServerCard {
        name: name.into(),
        address: address.into(),
        caption: String::new(),
        image_path: format!("{name}.png"),
        icon: None,
    }
}

fn detail_labels(view: &MenuView) -> Vec<String> {
    let font = fixture_font();
    let (mut nodes, mut next, mut layouts) =
        (Vec::new(), 1, ui::TextLayoutCache::new(128, 1024 * 1024));
    let metrics = super::super::super::super::TextMetrics::for_viewport(
        [1280, 720],
        ui::DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
    details::details_content(
        &mut canvas,
        view,
        &view.featured[0],
        0,
        [400.0, 100.0, 1200.0],
        &HashMap::new(),
    )
    .unwrap();
    canvas
        .nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } => Some(
                layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

#[test]
fn experience_details_show_positive_service_counts_without_ping_status() {
    let mut view = MenuView::new(true, "Fixture".into());
    let address = format!("{}fixture", launcher::menu::EXPERIENCE_ADDRESS_PREFIX);
    view.featured = vec![card("Experience", &address)];
    view.feeds.pings.insert(
        address.clone(),
        PingInfo {
            online: true,
            players: 2,
            ..Default::default()
        },
    );
    for count in [None, Some(0), Some(-1), Some(12_345), Some(i64::MAX)] {
        view.feeds.details.insert(
            address.clone(),
            ServerDetails {
                player_count: count,
                ..Default::default()
            },
        );
        let labels = detail_labels(&view);
        assert!(
            !labels
                .iter()
                .any(|label| label.contains("ping") || label == "Offline")
        );
        let numbers: Vec<_> = labels
            .iter()
            .filter(|label| label.chars().all(|ch| ch.is_ascii_digit()))
            .collect();
        let expected: Vec<_> = count
            .filter(|count| *count > 0)
            .map(|count| count.to_string())
            .into_iter()
            .collect();
        assert_eq!(numbers.into_iter().cloned().collect::<Vec<_>>(), expected);
    }
}

#[test]
fn creator_server_details_keep_pong_counts_instead_of_service_counts() {
    let mut view = MenuView::new(true, "Fixture".into());
    let address = "creator.test:19132";
    view.featured = vec![card("Creator", address)];
    view.feeds.details.insert(
        address.into(),
        ServerDetails {
            player_count: Some(12_345),
            ..Default::default()
        },
    );
    view.feeds.pings.insert(
        address.into(),
        PingInfo {
            online: true,
            players: 7,
            ping_ms: 40,
            ..Default::default()
        },
    );
    let labels = detail_labels(&view);
    assert!(labels.iter().any(|label| label == "Low ping"));
    assert!(labels.iter().any(|label| label == "7"));
    assert!(!labels.iter().any(|label| label == "12345"));
}

#[test]
fn groups_preserve_catalog_action_indices_and_use_real_motds() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.featured = vec![
        card("Creator", "creator.test:19132"),
        card("Experience", "gathering/fixture"),
    ];
    view.feeds.details.insert(
        view.featured[0].address.clone(),
        ServerDetails {
            group: "creator".into(),
            ..Default::default()
        },
    );
    view.feeds.details.insert(
        view.featured[1].address.clone(),
        ServerDetails {
            group: "featured".into(),
            ..Default::default()
        },
    );
    assert_eq!(group_entries(&view, "featured"), vec![1]);
    assert_eq!(group_entries(&view, "creator"), vec![0]);
    assert!(server_caption(&view, &view.featured[0]).is_empty());
    view.feeds.pings.insert(
        view.featured[0].address.clone(),
        PingInfo {
            motd: "§aLive server\n§lSeason 3".into(),
            ..Default::default()
        },
    );
    assert_eq!(
        server_caption(&view, &view.featured[0]),
        "Live server Season 3"
    );
}

#[test]
fn details_draw_complete_sections_and_end_at_content_with_independent_scroll() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.featured = vec![card("Creator", "creator.test:19132")];
    view.feeds.details.insert(
        view.featured[0].address.clone(),
        ServerDetails {
            group: "creator".into(),
            description: "A complete description".into(),
            banner: "banner.png".into(),
            news_title: "Latest update".into(),
            news: "News body".into(),
            games: vec![MenuGameCard {
                title: "Activity".into(),
                image_path: "activity.png".into(),
                description: "Build your base".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    let images = ["Creator.png", "banner.png", "activity.png"]
        .into_iter()
        .enumerate()
        .map(|(index, key)| {
            (
                key.into(),
                IconRef {
                    page: index as u16 + 1,
                    uv: [0, 0, 128, 128],
                    glint: false,
                },
            )
        })
        .collect();
    let font = fixture_font();
    let (mut nodes, mut next, mut layouts) =
        (Vec::new(), 1, ui::TextLayoutCache::new(128, 1024 * 1024));
    let metrics = super::super::super::super::TextMetrics::for_viewport(
        [1280, 720],
        ui::DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
    details(
        &mut canvas,
        &view,
        &view.featured[0],
        0,
        [400.0, 100.0, 1200.0, 700.0],
        &images,
    )
    .unwrap();
    let labels: Vec<_> = canvas
        .nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } => Some(
                layout
                    .glyphs()
                    .iter()
                    .map(|g| g.codepoint)
                    .collect::<String>()
                    .replace(' ', ""),
            ),
            _ => None,
        })
        .collect();
    let position = |label| labels.iter().position(|value| value == label).unwrap();
    assert!(position("Description") < position("Activities"));
    assert!(position("Activity") < position("News"));
    assert!(labels.iter().any(|v| v == "Acompletedescription"));
    assert!(canvas.nodes.iter().any(|n| matches!(
        n.visual(),
        ui::UiVisual::Sprite {
            texture_page: 2,
            ..
        }
    )));
    assert!(canvas.nodes.iter().any(|n| matches!(
        n.visual(),
        ui::UiVisual::Sprite {
            texture_page: 3,
            ..
        }
    )));
    let scroll = &canvas.scrolls[0];
    assert_eq!(scroll.key, "servers.details.0");
    assert!(scroll.max > 0.0);

    view.feeds
        .details
        .get_mut(&view.featured[0].address)
        .unwrap()
        .games
        .clear();
    view.feeds
        .details
        .get_mut(&view.featured[0].address)
        .unwrap()
        .description
        .clear();
    view.feeds
        .details
        .get_mut(&view.featured[0].address)
        .unwrap()
        .news
        .clear();
    view.feeds
        .details
        .get_mut(&view.featured[0].address)
        .unwrap()
        .news_title
        .clear();
    canvas.nodes.clear();
    let bottom = details::details_content(
        &mut canvas,
        &view,
        &view.featured[0],
        0,
        [400.0, 100.0, 1200.0],
        &images,
    )
    .unwrap();
    assert!(
        bottom < 700.0,
        "a short details card must not fill the viewport"
    );
}

#[test]
fn a_server_logo_is_never_stretched_into_missing_banner_art() {
    let view = MenuView::new(true, "Fixture".into());
    let server = card("Creator", "creator.test:19132");
    let images = HashMap::from([(
        server.image_path.clone(),
        IconRef {
            page: 42,
            uv: [0, 0, 128, 128],
            glint: false,
        },
    )]);
    let font = fixture_font();
    let (mut nodes, mut next, mut layouts) =
        (Vec::new(), 1, ui::TextLayoutCache::new(128, 1024 * 1024));
    let metrics = super::super::super::super::TextMetrics::for_viewport(
        [1280, 720],
        ui::DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
    details::details_content(&mut canvas, &view, &server, 0, [0.0, 0.0, 800.0], &images).unwrap();
    assert!(!canvas.nodes.iter().any(|n| matches!(
        n.visual(),
        ui::UiVisual::Sprite {
            texture_page: 42,
            ..
        }
    )));
}
