use super::*;
use render_model::{UI_BLEND_INVERT, UiRenderBatch, UiTextureCatalog, UiTexturePage};

/// A reusable two-triangle quad whose shared texture catalog has stable identity.
fn quad() -> UiRenderInput {
    UiRenderInput {
        revision: 1,
        viewport_size: [100, 80],
        safe_area: [0; 4],
        vertices: [[10.0, 20.0], [30.0, 20.0], [30.0, 40.0], [10.0, 40.0]]
            .map(|position| UiRenderVertex {
                position,
                clip_z: 0.0,
                clip_w: 1.0,
                uv: [0.0; 2],
                color: [255; 4],
                style_flags: 0,
                alpha_cutoff: -1.0,
                model_light: 1.0,
                overlay_color: [0.0; 4],
            })
            .into(),
        indices: Arc::from([0, 1, 2, 0, 2, 3]),
        batches: Arc::from([UiRenderBatch::new(
            0,
            UiScissor::new(0, 0, 100, 80),
            0,
            6,
            UI_BLEND_ALPHA,
        )]),
        textures: Arc::new(
            UiTextureCatalog::new(
                vec![UiTexturePage::owned([1, 1], Arc::from([255; 4])).unwrap()],
                1,
            )
            .unwrap(),
        ),
    }
}

/// Publishes a position change without changing topology or the texture catalog.
fn moved(input: &UiRenderInput, offset: [f32; 2]) -> UiRenderInput {
    let mut next = input.clone();
    next.revision += 1;
    for vertex in Arc::make_mut(&mut next.vertices) {
        for (position, offset) in vertex.position.iter_mut().zip(offset) {
            *position += offset;
        }
    }
    next
}

#[test]
fn unchanged_publications_retain_pixels_across_revisions_and_buffer_identity() {
    let previous = quad();
    assert_eq!(plan(&previous, &previous), UiDamage::Unchanged);
    let mut next = previous.clone();
    next.revision += 1;
    next.vertices = previous.vertices.to_vec().into();
    next.indices = previous.indices.to_vec().into();
    next.batches = previous.batches.to_vec().into();
    assert_eq!(plan(&previous, &next), UiDamage::Unchanged);
}

#[test]
fn moving_quad_includes_old_and_new_coverage_rounded_outward() {
    let previous = quad();
    let next = moved(&previous, [40.25, -5.5]);
    assert_eq!(
        plan(&previous, &next),
        UiDamage::Rect(UiScissor::new(9, 13, 63, 28))
    );
}

#[test]
fn one_changed_vertex_invalidates_its_entire_triangle() {
    let previous = quad();
    let mut next = previous.clone();
    Arc::make_mut(&mut next.vertices)[1].position = [29.0, 21.0];
    assert_eq!(
        plan(&previous, &next),
        UiDamage::Rect(UiScissor::new(9, 19, 22, 22))
    );
}

#[test]
fn damage_intersects_each_batch_scissor_and_the_viewport() {
    let mut previous = quad();
    Arc::make_mut(&mut previous.batches)[0].scissor = UiScissor::new(20, 25, 15, 10);
    assert_eq!(
        plan(&previous, &moved(&previous, [80.0, 40.0])),
        UiDamage::Rect(UiScissor::new(20, 25, 15, 10))
    );
    let previous = moved(&quad(), [80.0, 40.0]);
    assert_eq!(
        plan(&previous, &moved(&previous, [10.0, 20.0])),
        UiDamage::Rect(UiScissor::new(89, 59, 11, 21))
    );
    let previous = moved(&quad(), [200.0, 200.0]);
    assert_eq!(
        plan(&previous, &moved(&previous, [10.0, 10.0])),
        UiDamage::Unchanged
    );
}

#[test]
fn every_changed_batch_contributes_only_its_scissored_coverage() {
    let mut previous = quad();
    previous.indices = Arc::from([0, 1, 2, 0, 1, 2]);
    previous.batches = Arc::from([
        UiRenderBatch::new(0, UiScissor::new(10, 20, 3, 4), 0, 3, UI_BLEND_ALPHA),
        UiRenderBatch::new(0, UiScissor::new(27, 36, 3, 4), 3, 3, UI_BLEND_ALPHA),
    ]);
    assert_eq!(
        plan(&previous, &moved(&previous, [1.0, 1.0])),
        UiDamage::Rect(UiScissor::new(10, 20, 20, 20))
    );
}

#[test]
fn viewport_safe_area_catalog_topology_and_batch_state_changes_redraw_everything() {
    let previous = quad();
    for change in 0..9 {
        let mut next = previous.clone();
        match change {
            0 => next.viewport_size[0] += 1,
            1 => next.safe_area[0] += 1,
            2 => next.textures = Arc::new((*previous.textures).clone()),
            3 => Arc::make_mut(&mut next.indices)[0] = 1,
            4 => next.vertices = next.vertices[..3].into(),
            5 => Arc::make_mut(&mut next.batches)[0].scissor.width -= 1,
            6 => Arc::make_mut(&mut next.batches)[0].blend_mode = UI_BLEND_INVERT,
            7 => Arc::make_mut(&mut next.batches)[0].depth_test = 1,
            8 => Arc::make_mut(&mut next.batches)[0].isolated_depth_scope = Some(1),
            _ => unreachable!(),
        }
        assert_eq!(plan(&previous, &next), UiDamage::Full, "change {change}");
    }
}

#[test]
fn depth_style_uv_color_and_material_edits_damage_existing_triangle_bounds() {
    let mut previous = quad();
    let batch = &mut Arc::make_mut(&mut previous.batches)[0];
    batch.depth_test = 1;
    batch.depth_write = 1;
    batch.isolated_depth_scope = Some(4);
    for change in 0..8 {
        let mut next = previous.clone();
        let vertex = &mut Arc::make_mut(&mut next.vertices)[1];
        match change {
            0 => vertex.clip_z = 0.5,
            1 => vertex.style_flags = render_model::UI_STYLE_ALPHA_TEST,
            2 => vertex.uv[0] = 0.5,
            3 => vertex.color[0] = 128,
            4 => vertex.alpha_cutoff = 0.25,
            5 => vertex.model_light = 0.75,
            6 => vertex.overlay_color[0] = 0.5,
            7 => vertex.clip_z = -0.0,
            _ => unreachable!(),
        }
        assert_eq!(
            plan(&previous, &next),
            UiDamage::Rect(UiScissor::new(9, 19, 22, 22)),
            "change {change}"
        );
    }
}

#[test]
fn positive_homogeneous_coordinates_are_divided_before_bounding() {
    let mut previous = quad();
    for vertex in Arc::make_mut(&mut previous.vertices) {
        vertex.position = vertex.position.map(|coordinate| coordinate * 2.0);
        vertex.clip_w = 2.0;
    }
    let next = moved(&previous, [20.0, 0.0]);
    assert_eq!(
        plan(&previous, &next),
        UiDamage::Rect(UiScissor::new(9, 19, 32, 22))
    );
}

#[test]
fn invalid_coordinates_and_animated_or_projected_content_never_retain() {
    for change in 0..11 {
        let mut input = quad();
        let vertex = &mut Arc::make_mut(&mut input.vertices)[0];
        match change {
            0 => vertex.clip_w = 0.0,
            1 => vertex.clip_w = -1.0,
            2 => vertex.clip_w = f32::NAN,
            3 => vertex.clip_w = f32::INFINITY,
            4 => vertex.position[0] = f32::INFINITY,
            5 => vertex.clip_z = f32::NAN,
            6 => vertex.uv[0] = f32::NAN,
            7 => vertex.style_flags = UI_STYLE_GLINT,
            8 => vertex.overlay_color[0] = f32::NAN,
            9 => vertex.clip_w = f32::from_bits(1),
            10 => Arc::make_mut(&mut input.batches)[0].world_projection = 1,
            _ => unreachable!(),
        }
        assert_eq!(plan(&input, &input), UiDamage::Full, "change {change}");
    }
}

#[test]
fn malformed_topology_scissors_and_scope_lifetimes_fall_back() {
    for change in 0..8 {
        let mut input = quad();
        match change {
            0 => Arc::make_mut(&mut input.indices)[0] = u32::MAX,
            1 => Arc::make_mut(&mut input.batches)[0].index_count = 5,
            2 => Arc::make_mut(&mut input.batches)[0].first_index = 3,
            3 => Arc::make_mut(&mut input.batches)[0].scissor.x = u32::MAX,
            4 => input.viewport_size[0] = 0,
            5 => input.safe_area = [u32::MAX, 0, 1, 0],
            6 => Arc::make_mut(&mut input.batches)[0].texture_page = 1,
            7 => {
                input.indices = Arc::from([0, 1, 2, 0, 1, 2, 0, 1, 2]);
                input.batches = [Some(1), None, Some(1)]
                    .into_iter()
                    .enumerate()
                    .map(|(index, scope)| {
                        UiRenderBatch::new(
                            0,
                            UiScissor::new(0, 0, 100, 80),
                            index as u32 * 3,
                            3,
                            UI_BLEND_ALPHA,
                        )
                        .with_isolated_depth_scope(scope)
                    })
                    .collect::<Vec<_>>()
                    .into();
            }
            _ => unreachable!(),
        }
        assert_eq!(plan(&input, &input), UiDamage::Full, "change {change}");
    }
}

#[test]
fn unused_changed_vertices_and_empty_inputs_have_no_pixel_damage() {
    let mut previous = quad();
    previous.indices = Arc::from([0, 1, 2]);
    Arc::make_mut(&mut previous.batches)[0].index_count = 3;
    let mut next = previous.clone();
    Arc::make_mut(&mut next.vertices)[3].color[0] = 0;
    assert_eq!(plan(&previous, &next), UiDamage::Unchanged);
    previous.vertices = Arc::from([]);
    previous.indices = Arc::from([]);
    previous.batches = Arc::from([]);
    assert_eq!(plan(&previous, &previous), UiDamage::Unchanged);
}

#[test]
fn subnormal_homogeneous_projection_falls_back_before_gpu_flush_to_zero() {
    for (w, position) in [
        (f32::from_bits(1), 60.0 * f32::from_bits(1)),
        (f32::MIN_POSITIVE, 60.0 * f32::MIN_POSITIVE),
        (f32::MIN_POSITIVE, f32::from_bits(1)),
        (2.0 * f32::MIN_POSITIVE, 125.0 * f32::MIN_POSITIVE),
    ] {
        let mut previous = quad();
        let vertex = &mut Arc::make_mut(&mut previous.vertices)[0];
        vertex.clip_w = w;
        vertex.position = [position; 2];
        let mut current = previous.clone();
        Arc::make_mut(&mut current.vertices)[0].color[0] = 128;
        assert_eq!(plan(&previous, &current), UiDamage::Full);
    }
}
