use assets::BlockFace;
use assets::gui_item::{GuiBlockQuad, cube_face};

use {super::*, assets::gui_item::GUI_ITEM_SIDE, ui::IconRef};

/// Places a small test tile on the requested atlas page.
fn tile(page: u16) -> IconRef {
    IconRef {
        page,
        uv: [16, 32, 32, 48],
        glint: false,
    }
}

/// Makes a textured cube face for projection and batching assertions.
fn quad(face: BlockFace) -> GuiBlockQuad {
    let (corners, uvs) = cube_face(face);
    GuiBlockQuad {
        corners,
        uvs,
        material: 0,
    }
}

#[test]
fn quads_draw_through_the_cube_projection_with_their_own_depth() {
    // A bottom slab's top face sits in front of the full block's bottom face.
    let mut slab_top = quad(BlockFace::Up);
    for corner in &mut slab_top.corners {
        corner[1] = 0.5;
    }
    let quads = [(quad(BlockFace::Down), tile(3)), (slab_top, tile(3))];
    let mesh = mesh(&quads).unwrap();
    assert_eq!(mesh.vertices().len(), 8);
    assert_eq!(mesh.batches().len(), 1);
    let batch = &mesh.batches()[0];
    assert!(batch.depth_test && batch.depth_write);
    assert_eq!(batch.alpha_cutoff, Some(ALPHA_CUTOFF));
    for (vertex, corner) in mesh
        .vertices()
        .iter()
        .zip(quads.iter().flat_map(|(quad, _)| quad.corners))
    {
        let [x, y, _] = project_cube(corner);
        assert_eq!(vertex.position, [x / GUI_ITEM_SIDE, y / GUI_ITEM_SIDE]);
        assert!(vertex.alpha_test);
    }
    // Greater UI depth is nearer: the raised top face beats the bottom face beneath it.
    let nearest = |range: std::ops::Range<usize>| {
        mesh.vertices()[range]
            .iter()
            .map(|vertex| vertex.clip_z)
            .fold(f32::MIN, f32::max)
    };
    assert!(nearest(4..8) > nearest(0..4));
    assert!(
        mesh.vertices()
            .iter()
            .all(|vertex| (0.0..=1.0).contains(&vertex.clip_z))
    );
    // UVs land inside the quad's atlas tile, in texels.
    assert!(mesh.vertices().iter().all(|vertex| {
        (16.0..=32.0).contains(&vertex.uv[0]) && (32.0..=48.0).contains(&vertex.uv[1])
    }));
}

#[test]
fn quads_on_separate_pages_draw_as_separate_batches() {
    let quads = [
        (quad(BlockFace::Up), tile(4)),
        (quad(BlockFace::South), tile(2)),
        (quad(BlockFace::West), tile(4)),
    ];
    let mesh = mesh(&quads).unwrap();
    let pages: Vec<_> = mesh
        .batches()
        .iter()
        .map(|batch| (batch.texture_page, batch.index_range.len()))
        .collect();
    assert_eq!(pages, [(2, 6), (4, 12)]);
}

#[test]
fn a_uv_outside_its_tile_keeps_the_thumbnail() {
    let mut wrapped = quad(BlockFace::Up);
    wrapped.uvs[2] = [1.5, 1.0];
    assert!(mesh(&[(wrapped, tile(1))]).is_none());
}
