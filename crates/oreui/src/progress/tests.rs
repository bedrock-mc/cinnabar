use super::super::{review_tests::paint, test_support::solids};
use {super::*, launcher::menu::MenuView};

fn fill(seconds: f64, fraction: Option<f32>) -> Option<[f32; 4]> {
    let (_, _, nodes) = paint(Default::default(), |canvas| {
        canvas.seconds = seconds;
        track(canvas, [10.0, 20.0, 210.0, 32.0], fraction).unwrap();
    });
    solids(&nodes)
        .into_iter()
        .find(|(_, color)| *color == theme::PRIMARY_ROLE.fill)
        .map(|(bounds, _)| bounds)
}

#[test]
fn unknown_progress_animates_the_loader_without_claiming_a_percentage() {
    let mut previous = None;
    for seconds in [0.0, 0.25, 0.5, 0.75, 1.0, 1.25] {
        let (_, _, nodes) = paint(Default::default(), |canvas| {
            canvas.seconds = seconds;
            loader(canvas, [10.0, 20.0, 58.0, 68.0]).unwrap();
        });
        let frame = nodes
            .iter()
            .filter_map(|node| {
                if let ui::UiVisual::RotatedSprite { color, .. } = node.visual() {
                    Some(*color)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(frame.len(), 3);
        assert_eq!(fill(seconds, None), None);
        if let Some(previous) = previous {
            assert_ne!(frame, previous);
        }
        previous = Some(frame);
    }
}

#[test]
fn download_progress_is_determinate_and_clamps_invalid_input() {
    let half = fill(0.0, Some(0.5)).unwrap();
    assert_eq!(Some(half), fill(10.0, Some(0.5)));
    assert!((half[2] - 110.0).abs() < 1e-5);
    assert_eq!(fill(0.0, Some(2.0)).unwrap()[2], 208.0);
    assert_eq!(fill(0.5, Some(f32::NAN)), None);
}

#[test]
fn loading_logo_and_status_form_one_compact_centered_group() {
    use {ui::IconRef, ui_engine::textures::TITLE_KEY};
    static ART: std::sync::OnceLock<std::collections::HashMap<String, IconRef>> =
        std::sync::OnceLock::new();
    let art = ART.get_or_init(|| {
        std::collections::HashMap::from([(
            TITLE_KEY.to_owned(),
            IconRef {
                page: 1,
                uv: [0, 0, 300, 100],
                glint: false,
            },
        )])
    });
    let mut view = MenuView::new(true, "Fixture".into());
    view.connecting = true;
    let mut rem = 0.0;
    let (_, _, nodes) = paint(Default::default(), |canvas| {
        rem = canvas.rem;
        canvas.title_artwork = art.get(TITLE_KEY).copied();
        join(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
    });
    let logo = nodes
        .iter()
        .find(|node| {
            matches!(
                node.visual(),
                ui::UiVisual::Sprite {
                    texture_page: 1,
                    ..
                }
            )
        })
        .unwrap()
        .bounds();
    let panel = solids(&nodes)
        .into_iter()
        .find(|(bounds, color)| {
            *color == theme::NEUTRAL80.fill && bounds[2] - bounds[0] > rem * 20.0
        })
        .unwrap()
        .0;
    let gap = panel[1] - logo.max().y();
    assert!(
        gap >= 0.0 && gap <= rem * 2.4 + 0.01,
        "logo-to-status gap: {gap}"
    );
    assert!(((logo.min().y() + panel[3]) * 0.5 - 360.0).abs() <= rem);
}

/// Short join status fits every window tall enough for it, so it never scrolls, including
/// sizes whose rounded panel bounds once left a sub-pixel overflow.
#[test]
fn short_join_status_never_shows_a_scrollbar() {
    use ui_engine::{test_support::fixture_font, text_metrics::TextMetrics};
    let logo = ui::IconRef {
        page: 1,
        uv: [0, 0, 300, 100],
        glint: false,
    };
    let mut view = MenuView::new(true, "Fixture".into());
    view.feeds.join = launcher::menu::JoinProgress::new(launcher::menu::JoinKind::External);
    let font = fixture_font();
    let mut layouts = ui::TextLayoutCache::new(128, 1024 * 1024);
    for width in [640, 854, 1280, 1920] {
        for height in 320..=1080 {
            let (mut nodes, mut next) = (Vec::new(), 1);
            let metrics =
                TextMetrics::for_viewport([width, height], ui::DpiScale::new(1.0).unwrap(), None);
            let mut canvas =
                Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
            canvas.title_artwork = Some(logo);
            let size = [width as f32, height as f32];
            join(&mut canvas, &view, size, &|_| None).unwrap();
            assert!(
                canvas
                    .scrolls
                    .iter()
                    .all(|scroll| scroll.max == 0.0 && scroll.thumb.is_none()),
                "{size:?} scrolled by {:?}",
                canvas
                    .scrolls
                    .iter()
                    .map(|scroll| scroll.max)
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn long_loading_details_scroll_without_moving_cancel_off_screen() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.local.progress = Some(launcher::local_worlds::Progress {
        stage: launcher::local_worlds::Stage::DownloadingServer,
        fraction: Some(0.5),
        detail: "A long world name and progress description ".repeat(40),
    });
    for size in [[1280.0, 720.0], [640.0, 320.0], [320.0, 560.0]] {
        let (scrolls, hits, _) = paint(Default::default(), |canvas| {
            join(canvas, &view, size, &|_| None).unwrap()
        });
        let bounds = hits[0].1;
        assert!(bounds.min().y() >= 0.0 && bounds.max().y() <= size[1]);
        assert!(bounds.min().x() >= 0.0 && bounds.max().x() <= size[0]);
        let scroll = scrolls.iter().find(|scroll| scroll.max > 0.0).unwrap();
        assert!(scroll.viewport.min().y() < scroll.viewport.max().y());
        assert!(scroll.viewport.max().y() < bounds.min().y());
    }
}
