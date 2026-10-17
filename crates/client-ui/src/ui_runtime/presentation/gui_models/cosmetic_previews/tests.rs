use super::*;

/// Prepares the gallery camera through the production artwork worker's decoding path.
fn prepare(item: Item) {
    assert!(hats::thumbnail::render(item, 96, 112).is_some());
}

#[test]
fn menu_paper_dolls_animate_without_a_world_or_texture_rebuilds() {
    let mut runtime = UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let wings = hats::fixture::dragon_wings();
    let crown = hats::fixture::crown();
    runtime.set_preview_cosmetics(&[wings, crown]);
    runtime.gui_models.skin =
        Some(render_model::UiTexturePage::owned([64, 64], vec![255; 64 * 64 * 4].into()).unwrap());
    runtime.menu_artwork.refs.insert(
        wings.texture_key(),
        IconRef {
            page: 3,
            uv: [0, 0, 64, 64],
            glint: false,
        },
    );
    runtime.sync_cosmetic_previews(true);
    let textures = Arc::clone(&runtime.textures);
    let before = runtime.gui_player_mesh().unwrap();
    let views = [
        super::super::super::player_preview::PreviewView::Live { offset: [0.0; 2] },
        super::super::super::player_preview::PreviewView::Doll {
            yaw: 30.0,
            tilt: -10.0,
        },
        super::super::super::player_preview::PreviewView::SkinSelector {
            yaw: 30.0,
            tilt: -10.0,
            offset: [0.0; 2],
        },
    ];
    let first: Vec<_> = views
        .iter()
        .map(|&view| {
            runtime.player_preview_view = view;
            runtime.gui_player_mesh().unwrap()
        })
        .collect();
    let crown_before = runtime.gui_models.cosmetics.items[&crown].vertices.clone();
    runtime.menu_seconds = 0.7;
    runtime.sync_cosmetic_previews(true);
    assert_ne!(
        before.vertices(),
        runtime.gui_player_mesh().unwrap().vertices()
    );
    for (view, first) in views.into_iter().zip(first) {
        runtime.player_preview_view = view;
        assert_ne!(
            first.vertices(),
            runtime.gui_player_mesh().unwrap().vertices()
        );
    }
    assert!(Arc::ptr_eq(
        &crown_before,
        &runtime.gui_models.cosmetics.items[&crown].vertices
    ));
    assert!(Arc::ptr_eq(&textures, &runtime.textures));
    let frozen = runtime.gui_models.cosmetics.items[&wings].vertices.clone();
    let (_, allocations) = test_alloc::count(|| runtime.sync_cosmetic_previews(true));
    assert_eq!(
        allocations, 0,
        "unchanged playback retains its working buffers"
    );
    assert!(Arc::ptr_eq(
        &frozen,
        &runtime.gui_models.cosmetics.items[&wings].vertices
    ));
    runtime.sync_visible_cosmetic_previews(&[]);
    assert!(runtime.gui_models.cosmetics.items.is_empty());
    runtime.sync_cosmetic_previews(true);
    runtime.set_preview_cosmetics(&[]);
    runtime.sync_cosmetic_previews(true);
    assert!(runtime.gui_models.cosmetics.items.is_empty());
}

#[test]
fn visible_tiles_animate_with_original_texels_and_keep_their_camera() {
    let wings = hats::fixture::dragon_wings();
    prepare(wings);
    let icon = IconRef {
        page: 7,
        uv: [12, 34, 76, 98],
        glint: false,
    };
    let mut previews = Previews::default();
    let active = [(wings, Some(icon))];
    previews.sync(&active, 10.0);
    let first = previews.items[&wings].gallery.as_ref().unwrap().1.clone();
    previews.sync(&active, 10.0);
    assert!(Arc::ptr_eq(
        &first,
        &previews.items[&wings].gallery.as_ref().unwrap().1
    ));
    previews.sync(&active, 10.7);
    let next = &previews.items[&wings].gallery.as_ref().unwrap().1;
    assert_ne!(first.vertices(), next.vertices());
    assert_eq!(first.batches(), next.batches());
    assert_eq!(first.batches()[0].texture_page, icon.page);
    for (before, after) in first.vertices().iter().zip(next.vertices()) {
        assert_eq!(before.uv, after.uv);
        assert_eq!(before.color, after.color);
        assert!(after.position.iter().all(|v| (0.0..=1.0).contains(v)));
    }
    let frame = hats::thumbnail::prepared_frame(wings).unwrap();
    let fixed = [0.0, 1.5, 0.0];
    let projected = frame.project(fixed, [0.0; 3], 96, 112);
    previews.sync(&active, 11.4);
    assert_eq!(projected, frame.project(fixed, [0.0; 3], 96, 112));
    previews.sync(&[], 12.0);
    assert!(previews.items.is_empty());
}

#[test]
fn gallery_replacement_preserves_bounds_parent_clip_and_selection() {
    use ui::UiNodeId;
    let wings = hats::fixture::dragon_wings();
    prepare(wings);
    let thumbnail = IconRef {
        page: 3,
        uv: [0, 0, 96, 112],
        glint: false,
    };
    let texture = IconRef {
        page: 4,
        ..thumbnail
    };
    let mut runtime = UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    runtime
        .menu_artwork
        .refs
        .insert(wings.thumbnail_key(), thumbnail);
    runtime
        .gui_models
        .cosmetics
        .sync(&[(wings, Some(texture))], 0.0);
    let bounds = super::super::super::rect(10.0, 20.0, 96.0, 112.0).unwrap();
    let tile = UiNode::new(UiNodeId::new(2), Some(UiNodeId::new(1)), bounds)
        .with_clip_children(true)
        .with_visual(UiVisual::Sprite {
            texture_page: thumbnail.page,
            uv: thumbnail.uv,
            color: [255; 4],
        });
    let badge = UiNode::new(UiNodeId::new(3), None, bounds);
    let mut nodes = vec![tile.clone(), badge.clone()];
    runtime.apply_cosmetic_gallery_models(&mut nodes);
    let UiVisual::Mesh(mesh) = nodes[0].visual() else {
        panic!("animated tile mesh")
    };
    assert_eq!(nodes[0], tile.with_visual(UiVisual::Mesh(Arc::clone(mesh))));
    assert_eq!(nodes[1], badge);
}
