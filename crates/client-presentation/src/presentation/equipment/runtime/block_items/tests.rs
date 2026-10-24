//! Carried world-item meshes retain the shape and pixels from one prepared generation.

use super::*;

impl PreparedBlockItems {
    /// Returns a narrow box with original face tiles and its matching terrain allocation.
    pub(crate) fn test_model(seed: u8) -> (Arc<Self>, Arc<RuntimeAssets>) {
        let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE;
        let side = usize::from(assets::BLOCK_ITEM_FACE_SIDE);
        let columns = usize::from(assets::BLOCK_ITEM_SHEET_GRID[0]);
        let mut pixels = vec![0_u8; usize::from(width) * usize::from(height) * 4];
        for face in 0..assets::BlockFace::ALL.len() {
            for y in 0..side {
                for x in 0..side {
                    let pixel = ((face / columns * side + y) * usize::from(width)
                        + face % columns * side
                        + x)
                        * 4;
                    pixels[pixel..pixel + 4].copy_from_slice(&[seed + face as u8, 40, 60, 255]);
                }
            }
        }
        let atlas = SpriteAtlas::pack(&[assets::IconSprite {
            width,
            height,
            rgba8: pixels.into(),
        }]);
        let mut vertices =
            render_model::textured_cube_vertices(render_model::equipment::blocks::face_rects([
                0.0, 0.0, 1.0, 1.0,
            ]));
        for vertex in &mut vertices {
            vertex.position[0] *= 0.25;
            vertex.position[2] *= 0.25;
        }
        let world = super::super::tests::block_reload_tests::terrain(128, [5, 6, 7, 255], 0);
        let source = Arc::new(Self {
            world: Some(Arc::clone(&world)),
            sheets: [(1, 0)].into(),
            models: [(1, vertices)].into(),
            alpha: BTreeMap::new(),
            placements: atlas.placements,
            rasters: atlas.layers,
        });
        (source, world)
    }
}

#[test]
fn carried_drop_geometry_keeps_shape_and_distinct_face_pixels() {
    let (source, _) = PreparedBlockItems::test_model(20);
    let model = source.dropped_model(1).unwrap();
    assert_eq!(source.dropped_model_layers(1), Some(model.materials.len()));
    for (face, (sprite, _)) in model.materials.iter().enumerate() {
        assert!(
            sprite
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| { *pixel == [20 + face as u8, 40, 60, 255] })
        );
    }
    let mesh = render_model::dropped_item::block_mesh(&model, 1, 64).unwrap();
    for held in &source.models[&1] {
        assert!(
            mesh.iter().any(|dropped| {
                dropped.position == held.position && dropped.normal == held.normal
            })
        );
    }
    assert!(source.dropped_model(2).is_none());
}

#[test]
fn unused_carried_sheet_tiles_do_not_consume_model_layers() {
    let (mut source, _) = PreparedBlockItems::test_model(20);
    let vertices = Arc::get_mut(&mut source)
        .unwrap()
        .models
        .get_mut(&1)
        .unwrap();
    for vertex in vertices {
        vertex.uv = std::array::from_fn(|axis| vertex.uv[axis] - vertex.glint_origin[axis]);
        vertex.back_uv = vertex.uv;
        vertex.glint_origin = [0.0; 2];
    }
    let model = source.dropped_model(1).unwrap();
    assert_eq!(source.dropped_model_layers(1), Some(1));
    assert_eq!(model.materials.len(), 1);
    assert!(model.quads.iter().all(|quad| quad.material == 0));
}

#[test]
fn unavailable_prepared_sheets_keep_the_model_fallback_available() {
    let (mut source, _) = PreparedBlockItems::test_model(20);
    let source = Arc::get_mut(&mut source).unwrap();
    source.placements[0] = None;
    assert!(source.dropped_model_layers(1).is_none());
    assert!(source.dropped_model(1).is_none());
}
