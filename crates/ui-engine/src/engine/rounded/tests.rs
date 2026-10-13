use super::*;

/// Builds an authored rounded paint fixture in a fixed logical box.
fn key(color: [u8; 4], end_color: [u8; 4], border: f32, border_color: [u8; 4]) -> Key {
    Key {
        width: 40f32.to_bits(),
        height: 20f32.to_bits(),
        radius: 6f32.to_bits(),
        fringe: 1f32.to_bits(),
        page: 0,
        color,
        end_color,
        horizontal: false,
        border: border.to_bits(),
        border_color,
    }
}

/// A cold insertion preserves recently used shapes instead of clearing the whole cache.
#[test]
fn rounded_geometry_cache_evicts_only_the_coldest_entry() {
    let mut cache = ShapeCache::default();
    let first = key([255; 4], [255; 4], 0.0, [0; 4]);
    let retained = cache.get_or_create(first).unwrap();
    for index in 1..MAX_CACHED_SHAPES {
        cache
            .get_or_create(Key {
                width: (40.0 + index as f32).to_bits(),
                ..first
            })
            .unwrap();
    }
    assert!(Arc::ptr_eq(&cache.get_or_create(first).unwrap(), &retained));
    cache
        .get_or_create(Key {
            width: 5000f32.to_bits(),
            ..first
        })
        .unwrap();
    assert_eq!(cache.entries.len(), MAX_CACHED_SHAPES);
    assert!(Arc::ptr_eq(&cache.get_or_create(first).unwrap(), &retained));
    assert!(!cache.entries.contains_key(&Key {
        width: 41f32.to_bits(),
        ..first
    }));
}

/// Returns the vertex's visible straight color after evaluating its coverage paint.
fn painted_color(vertex: &ui::UiMeshVertex) -> Option<[u8; 4]> {
    let paint = render_api::UiCoveragePaint::from_vertex_colors(vertex.color, vertex.overlay_color);
    let sample = paint.sample(vertex.uv[0], [vertex.uv[1], vertex.model_light]);
    (sample[3] > 0.0).then(|| {
        [
            (sample[0] / sample[3] * 255.0).round() as u8,
            (sample[1] / sample[3] * 255.0).round() as u8,
            (sample[2] / sample[3] * 255.0).round() as u8,
            (sample[3] * 255.0).round() as u8,
        ]
    })
}

/// Visible vertices as logical pixel positions and evaluated colors.
fn solid(mesh: &UiMesh) -> Vec<([f32; 2], [u8; 4])> {
    mesh.vertices()
        .iter()
        .filter_map(|vertex| {
            painted_color(vertex).map(|color| {
                (
                    [vertex.position[0] * 40.0, vertex.position[1] * 20.0],
                    color,
                )
            })
        })
        .collect()
}

#[test]
fn plain_fill_keeps_one_color() {
    let red = [255, 0, 0, 255];
    let mesh = mesh_of(key(red, red, 0.0, [0; 4]));
    assert!(solid(&mesh).iter().all(|(_, color)| color[..3] == red[..3]));
}

#[test]
fn vertical_gradient_runs_from_top_to_bottom() {
    let (top, bottom) = ([0, 0, 0, 255], [200, 100, 0, 255]);
    let mesh = mesh_of(key(top, bottom, 0.0, [0; 4]));
    let vertices = solid(&mesh);
    let (_, upper) = vertices
        .iter()
        .min_by(|a, b| a.0[1].total_cmp(&b.0[1]))
        .unwrap();
    let (_, lower) = vertices
        .iter()
        .max_by(|a, b| a.0[1].total_cmp(&b.0[1]))
        .unwrap();
    assert!(upper[0] < 20 && lower[0] > 180, "{upper:?} {lower:?}");
    let center = vertices.iter().find(|(at, _)| *at == [20.0, 10.0]).unwrap();
    assert_eq!(center.1, [100, 50, 0, 255]);
    let mut horizontal = key(top, bottom, 0.0, [0; 4]);
    horizontal.horizontal = true;
    let vertices = solid(&mesh_of(horizontal));
    let (_, left) = vertices
        .iter()
        .min_by(|a, b| a.0[0].total_cmp(&b.0[0]))
        .unwrap();
    assert!(left[0] < 10);
}

/// Builds the fixture mesh without involving cached geometry.
fn mesh_of(key: Key) -> UiMesh {
    mesh(key).unwrap()
}

#[test]
fn border_occupies_the_edge_and_the_fill_stays_inside_it() {
    let (fill, border) = ([0, 0, 255, 64], [255, 255, 255, 255]);
    let mesh = mesh_of(key(fill, fill, 2.0, border));
    let vertices = solid(&mesh);
    for (at, color) in &vertices {
        if *color == fill {
            assert!(
                at[0] >= 2.0 - 1e-4
                    && at[0] <= 38.0 + 1e-4
                    && at[1] >= 2.0 - 1e-4
                    && at[1] <= 18.0 + 1e-4,
                "fill vertex {at:?} lies under the border"
            );
        }
    }
    assert!(
        vertices
            .iter()
            .any(|(at, color)| *color == border && at[1] <= 1.0)
    );
    assert!(
        mesh.vertices()
            .iter()
            .any(|vertex| painted_color(vertex).is_none()),
        "the border's outer edge is antialiased"
    );
    let square = Key {
        radius: 0f32.to_bits(),
        ..key(fill, fill, 1.0, border)
    };
    assert!(
        solid(&mesh_of(square))
            .iter()
            .filter(|(_, color)| *color == fill)
            .all(|(at, _)| at[0] >= 1.0 - 1e-4 && at[1] >= 1.0 - 1e-4)
    );
}
