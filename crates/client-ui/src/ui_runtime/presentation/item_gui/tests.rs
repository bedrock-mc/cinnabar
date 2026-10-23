use assets::{BlockFace, gui_item::CUBE_OFFSET};
use {super::*, assets::gui_item::GUI_ITEM_SIDE, ui::IconRef};

fn face(page: u16) -> IconRef {
    IconRef {
        color_mask: None,
        page,
        uv: [32, 48, 48, 64],
        glint: false,
    }
}

#[test]
fn cube_uses_native_design_projection_without_a_thumbnail_sampling_grid() {
    assert_eq!(
        cube_project([0.0; 3]),
        CUBE_OFFSET.map(|value| value / GUI_ITEM_SIDE)
    );
    let mesh = cube(std::array::from_fn(|face_index| face(face_index as u16))).unwrap();
    assert_eq!(mesh.vertices().len(), 12);
    assert_eq!(mesh.indices().len(), 18);
    assert_eq!(mesh.batches().len(), 3);
    for (index, (block_face, positions, _, brightness)) in CUBE_FACES.into_iter().enumerate() {
        let shade = (brightness * f32::from(u8::MAX)) as u8;
        let vertices = &mesh.vertices()[index * 4..][..4];
        for (vertex, authored) in vertices.iter().zip(positions) {
            assert_eq!(vertex.position, cube_project(authored));
            assert_eq!(vertex.clip_w, 1.0);
            assert_eq!(vertex.color, [shade, shade, shade, 255]);
        }
        assert_eq!(mesh.batches()[index].texture_page, block_face as u16);
        assert!(!mesh.batches()[index].depth_test);
        assert!(!mesh.batches()[index].depth_write);
    }
    // Geometry positions are not snapped to a sixteen/32/64-pixel offscreen raster.
    assert!(mesh.vertices().iter().any(|vertex| {
        vertex
            .position
            .iter()
            .any(|v| (v * GUI_ITEM_SIDE).fract() != 0.0)
    }));
}

#[test]
fn cube_preserves_authored_face_uv_edges_and_glint() {
    let mut faces = [face(2); 6];
    faces[BlockFace::Up as usize].glint = true;
    let mesh = cube(faces).unwrap();
    assert_eq!(mesh.vertices()[0].uv, [32.0, 48.0]);
    assert_eq!(mesh.vertices()[1].uv, [32.0, 64.0]);
    assert_eq!(mesh.vertices()[2].uv, [48.0, 64.0]);
    assert_eq!(mesh.vertices()[3].uv, [48.0, 48.0]);
    assert!(
        mesh.vertices()[..4]
            .iter()
            .all(|vertex| vertex.style_flags == u16::from(ui::UI_STYLE_GLINT))
    );
    assert!(
        mesh.vertices()[4..]
            .iter()
            .all(|vertex| vertex.style_flags == 0)
    );
    faces[BlockFace::South as usize].uv[2] = faces[BlockFace::South as usize].uv[0];
    assert!(cube(faces).is_none());
}

#[test]
fn cutout_cubes_show_outward_faces_with_the_thumbnail_alpha_threshold() {
    let faces = std::array::from_fn(|index| face(index as u16));
    let mesh = cutout_cube(faces).unwrap();
    assert_eq!(mesh.vertices().len(), CUBE_FACES.len() * 4);
    assert_eq!(mesh.indices().len(), CUBE_FACES.len() * 6);
    for (face, _, _, _) in CUBE_FACES {
        let batch = mesh
            .batches()
            .iter()
            .find(|batch| batch.texture_page == faces[face as usize].page)
            .expect("outward faces must remain drawable");
        assert_eq!(batch.index_range.len(), 6);
        assert!(batch.depth_test && batch.depth_write);
        assert_eq!(
            batch.alpha_cutoff,
            Some(assets::gui_item::GUI_BLOCK_ALPHA_THRESHOLD as f32 / u8::MAX as f32)
        );
        let (corners, uvs) = assets::gui_item::cube_face(face);
        let indices =
            &mesh.indices()[batch.index_range.start as usize..batch.index_range.end as usize];
        for ((corner, uv), index) in corners.into_iter().zip(uvs).zip([0, 1, 2, 5]) {
            let vertex = mesh.vertices()[indices[index] as usize];
            let [x, y, _] = assets::gui_item::project_cube(corner);
            assert_eq!(vertex.position, [x / GUI_ITEM_SIDE, y / GUI_ITEM_SIDE]);
            assert_eq!(vertex.uv, atlas_uv(faces[face as usize], uv).unwrap());
            assert!(vertex.alpha_test);
        }
    }
}

#[test]
fn model_uv_contract_preserves_half_texel_centers_and_rejects_nonfinite() {
    assert_eq!(atlas_uv(face(1), [0.5, 0.25]), Some([40.0, 52.0]));
    assert_eq!(
        atlas_uv(face(1), [0.5 / 16.0, 0.5 / 16.0]),
        Some([32.5, 48.5])
    );
    assert_eq!(atlas_uv(face(1), [f32::NAN, 0.25]), None);
}
