//! Installed OreUI artwork registered as texture pages by the presentation runtime.

use std::{collections::HashMap, sync::Arc};

use super::super::super::UiPresentationRuntime;
use oreui::images::{OreUiImages, OreUiSprite};
use ui_engine::test_support::fixture_font;

#[test]
fn unchanged_native_art_reuses_pages_and_changed_art_replaces_its_range() {
    use oreui::images::OreUiPage;
    let images = |count: usize| OreUiImages {
        pages: (0..count)
            .map(|page| OreUiPage {
                dimensions: [2, 2],
                pixels: vec![page as u8; 16].into(),
            })
            .collect(),
        sprites: Arc::new(HashMap::from([(
            "native".into(),
            OreUiSprite {
                page: 0,
                bounds: [0, 0, 2, 2],
            },
        )])),
        loading_frames: Default::default(),
        animations: Default::default(),
        source: None,
    };
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    let start = runtime.textures.dynamic_start();
    let dynamic_count = runtime.textures.pages().len() - start;
    let first = images(1);
    runtime.enable_oreui_originals(first.clone()).unwrap();
    let registered = runtime.textures.clone();
    runtime.enable_oreui_originals(first).unwrap();
    assert!(
        Arc::ptr_eq(&registered, &runtime.textures),
        "unchanged art must retain the published texture catalog"
    );
    runtime.enable_oreui_originals(images(2)).unwrap();
    assert_eq!(runtime.textures.dynamic_start(), start + 2);
    assert_eq!(runtime.textures.pages().len(), start + 2 + dynamic_count);
    assert_eq!(runtime.textures.pages()[start + 1].pixels(), &[1; 16]);
    assert_eq!(
        runtime.screens.oreui.originals.as_ref().unwrap().page(),
        start as u16
    );
}
