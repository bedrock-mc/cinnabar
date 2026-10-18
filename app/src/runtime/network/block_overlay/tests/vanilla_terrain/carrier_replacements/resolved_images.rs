use super::*;

/// Gives the first cube an explicit-filename carried alias while retaining its world source.
fn alias_keys() -> MaterialKeys {
    let original = keys();
    let terrain = original
        .terrain_entries()
        .map(|(key, value)| (key.into(), value.clone()))
        .collect::<Vec<_>>();
    let sources = original
        .material_sources()
        .map(|(id, source)| (id, source.clone()))
        .collect::<Vec<_>>();
    let mut blocks = original.block_sources().to_vec();
    blocks[0].carried_keys = std::array::from_fn(|_| Some("alias".into()));
    original.with_sources(terrain, sources, blocks)
}

#[test]
fn explicit_filename_carried_aliases_reuse_the_world_raster_at_the_layer_limit() {
    let base = base();
    let keys = alias_keys();
    let color = [140, 20, 30, 255];
    let view = timeline_pack(
        json!({"texture_data":{"alias":{"textures":"textures/base_a.png"}}}),
        json!([]),
        &[("textures/base_a", 16, 16, color, color)],
    );
    for limit in [2, 4] {
        let compiled = super::super::super::super::compile_block_overlay(
            &view,
            &CustomBlocks::default(),
            false,
            Some(&keys),
            limit,
            Some(&base),
        )
        .unwrap();
        assert_eq!(compiled.overlay.textures.first().unwrap().layers, 2);
        assert_eq!(compiled.vanilla_carried.len(), 1);
        let faces = compiled.vanilla_carried[0].faces;
        let sheet =
            render_model::equipment::blocks::overlay_face_sheet(&compiled.overlay, faces, None)
                .unwrap();
        assert!(
            sheet
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == color)
        );
        assert!(pack_entity_compiler::overlay_cube_icon(&compiled.overlay, faces, None).is_some());
        let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
        assert_eq!(material_color(&runtime, 1), color);
    }
}

#[test]
fn resolved_image_aliases_keep_their_grid_and_animation_policies() {
    let base = base();
    let keys = alias_keys();
    let a = [140, 20, 30, 255];
    let b = [30, 40, 180, 255];
    for animated in [false, true] {
        let view = timeline_pack(
            json!({"texture_data":{"alias":{"quad":if animated {0} else {1}, "textures":"textures/base_a.png"}}}),
            if animated {
                json!([{"atlas_tile":"state","flipbook_texture":"textures/base_a","frames":[1,0]}])
            } else {
                json!([])
            },
            &[("textures/base_a", 16, if animated { 32 } else { 16 }, a, b)],
        );
        let compiled = compile(&view, &base, &keys);
        assert_eq!(
            compiled.overlay.textures.first().unwrap().layers,
            if animated { 4 } else { 3 }
        );
        let faces = compiled.vanilla_carried[0].faces;
        let runtime = base.with_block_overlay(2, &compiled.overlay).unwrap();
        assert_eq!(runtime.texture_source_grid(runtime.material(1).texture), 0);
        assert_eq!(
            compiled.overlay.texture_source_grids[compiled.overlay.materials[faces[0] as usize]
                .texture
                .layer() as usize],
            if animated { 0 } else { 1 }
        );
        assert_eq!(material_color(&runtime, 1), if animated { b } else { a });
        let sheet =
            render_model::equipment::blocks::overlay_face_sheet(&compiled.overlay, faces, None)
                .unwrap();
        assert!(
            sheet
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == a)
        );
    }
}
