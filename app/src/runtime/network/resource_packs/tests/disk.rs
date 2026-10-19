use super::*;
use pack_runtime::compile_cache::CompileCache;
use std::{cell::Cell, sync::Arc};

#[test]
fn persistent_blocks_and_icons_preserve_compiled_output_and_skip_work() {
    let dir = tempfile::tempdir().unwrap();
    let keys = assets::material_keys::MaterialKeys::from_entries([(5, "fixture_key")]);
    let stack = resource_pack::validate_handoff(protocol::ResourcePackHandoff::from_archives(
        vec![archive(
            53,
            &[
                (
                    "textures/terrain_texture.json",
                    br#"{"texture_data":{"fixture_key":{"textures":"textures/blocks/fixture"}}}"#,
                ),
                ("textures/blocks/fixture.png", &png(16, 16)),
                (
                    "textures/item_texture.json",
                    br#"{"texture_data":{"fixture_icon":{"textures":"textures/items/fixture"}}}"#,
                ),
                ("textures/items/fixture.png", &png(16, 16)),
            ],
        )],
    ));
    let blocks = protocol::CustomBlocks::default();
    let compiles = Cell::new(0);
    let mut expected = None;
    for _ in 0..2 {
        let cache = CompileCache::new(dir.path().into(), 1 << 20);
        let view = resource_pack::LayeredPackView::tracked(stack.clone());
        let output: Option<Arc<super::super::CompiledBlockOverlay>> =
            cache.get_or_compile(&[1; 32], &view, || {
                compiles.set(compiles.get() + 1);
                super::super::compile_block_overlay(
                    &view,
                    &blocks,
                    false,
                    Some(&keys),
                    assets::MAX_TEXTURE_LAYERS as u32,
                    None,
                )
                .map(Arc::new)
            });
        let output = output.expect("the fixture overrides a terrain material");
        let reads = view.dependencies().unwrap().snapshot();
        let actual = (output, reads);
        if let Some(expected) = &expected {
            assert_eq!(expected, &actual);
        }
        expected = Some(actual);
    }
    assert_eq!(compiles.get(), 1);
    let mut expected = None;
    for _ in 0..2 {
        let cache = CompileCache::new(dir.path().into(), 1 << 20);
        let view = resource_pack::LayeredPackView::tracked(stack.clone());
        let output: Option<Arc<client_ui::ui_runtime::presentation::SessionIcons>> = cache
            .get_or_compile(&[2; 32], &view, || {
                compiles.set(compiles.get() + 1);
                super::super::compile_session_icons(
                    &view,
                    &[("fixture:item".into(), "fixture_icon".into())],
                    Default::default(),
                )
            });
        let output = output.expect("the fixture defines an icon");
        assert!(!output.icons.is_empty());
        let actual = (output, view.dependencies().unwrap().snapshot());
        if let Some(expected) = &expected {
            assert_eq!(expected, &actual);
        }
        expected = Some(actual);
    }
    assert_eq!(
        compiles.get(),
        2,
        "only the initial block and icon compiles run"
    );
}
