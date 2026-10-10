use super::*;
use crate::ui_runtime::{
    oreui_assets::shipped_oreui_images,
    presentation::{TextMetrics, forms::oreui::paint::Originals, tests::fixture_font},
};
use std::collections::HashMap;

#[test]
fn shipped_back_chevron_uses_the_header_text_color_in_both_themes() {
    let images = shipped_oreui_images();
    let mask = images.sprites[&format!("@mask/{CHEVRON_LEFT_IMAGE}")];
    let originals = Originals {
        page: 0,
        sprites: images.sprites.clone(),
        masks: HashMap::from([(CHEVRON_LEFT_IMAGE.into(), mask)]),
        loading_frames: images.loading_frames.clone(),
        animations: images.animations.clone(),
        images,
    };
    let font = fixture_font();
    for appearance in [theme::Appearance::Default, theme::Appearance::Dark] {
        let (mut nodes, mut next, mut layouts) = (Vec::new(), 1, ui::TextLayoutCache::new(8, 4096));
        let metrics =
            TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
        let mut canvas = Canvas::new(
            &mut nodes,
            &mut next,
            &mut layouts,
            &font,
            metrics,
            0,
            Some(&originals),
        );
        canvas.appearance = appearance;
        let color = canvas.role(theme::NEUTRAL20).text;
        header(
            &mut canvas,
            &MenuView::new(true, "Player".into()),
            1280.0,
            &|_| None,
        )
        .unwrap();
        assert!(
            nodes
                .iter()
                .any(|node| matches!(node.visual(), ui::UiVisual::Sprite {
            texture_page, uv, color: drawn,
        } if *texture_page == mask.page && *uv == mask.bounds && *drawn == color))
        );
    }
}
