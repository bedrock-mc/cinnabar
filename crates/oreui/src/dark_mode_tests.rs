use {super::*, launcher::menu::MenuView};

#[test]
fn dark_mode_dims_the_full_home_backdrop_more_than_default() {
    let view = MenuView::new(true, "Player".into());
    let overlay = |appearance| {
        let (_, _, nodes) = review_tests::paint(Default::default(), |canvas| {
            canvas.appearance = appearance;
            home::draw(canvas, &view, [1280.0, 720.0], None, &|_| None).unwrap();
        });
        test_support::solids(&nodes)
            .into_iter()
            .find_map(|(bounds, color)| {
                (bounds == [0.0, 0.0, 1280.0, 720.0] && color[..3] == [0; 3]).then_some(color[3])
            })
            .unwrap()
    };
    assert!(overlay(theme::Appearance::Dark) > overlay(theme::Appearance::Default));
}

#[test]
fn dark_mode_preserves_full_color_art_and_readable_labels_on_light_tags() {
    let (_, _, nodes) = review_tests::paint(Default::default(), |canvas| {
        canvas.appearance = theme::Appearance::Dark;
        widgets::tag(
            canvas,
            "Owner",
            [0.0, 0.0],
            theme::SUCCESS_TINT,
            theme::TEXT_DARK,
        )
        .unwrap();
        canvas
            .icon_ref(
                ui::IconRef {
                    color_mask: None,
                    page: 17,
                    uv: [0, 0, 32, 32],
                    glint: false,
                },
                [0.0, 30.0, 32.0, 62.0],
            )
            .unwrap();
    });
    assert!(nodes.iter().any(|node| matches!(node.visual(), ui::UiVisual::Text { color, .. } if *color == theme::TEXT_DARK)));
    assert!(nodes.iter().any(|node| matches!(
        node.visual(),
        ui::UiVisual::Sprite {
            texture_page: 17,
            color: [255, 255, 255, 255],
            ..
        }
    )));
}
