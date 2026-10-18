use super::*;
use serde_json::{Value, json};

/// Adjacent even-odd rectangles rasterize as the same uninterrupted silhouette as one rectangle.
#[test]
fn adjacent_rectangles_rasterize_without_a_shared_edge_seam() {
    for (size, dpi) in [([16.0; 2], 1.0), ([24.0, 20.0], 1.25), ([32.0; 2], 2.0)] {
        let adjacent = key(
            json!([
                [[0, 0], [8, 0], [8, 16], [0, 16]],
                [[8, 0], [16, 0], [16, 16], [8, 16], [8, 8]],
            ]),
            None,
            size,
            dpi,
        );
        let combined = key(
            json!([[[0, 0], [16, 0], [16, 16], [0, 16]]]),
            None,
            size,
            dpi,
        );
        assert_eq!(
            pixels(&mesh(&adjacent).unwrap(), &adjacent),
            pixels(&mesh(&combined).unwrap(), &combined),
            "shared edges must not change rasterized coverage at {size:?}, DPI {dpi}"
        );
    }
}

/// Uses generic caller-authored fixtures with a translucent constant tint.
fn key(paths: Value, stroke: Option<f32>, size: [f32; 2], dpi: f32) -> Key {
    let mut data: std::collections::BTreeMap<String, Value> =
        serde_json::from_value(json!({"view_box": [0, 0, 16, 16], "paths": paths})).unwrap();
    if let Some(stroke) = stroke {
        data.insert("stroke_width".into(), json!(stroke));
    }
    Key {
        path: super::super::parse(&data).unwrap(),
        size: size.map(f32::to_bits),
        dpi: dpi.to_bits(),
        page: 0,
        color: [31, 89, 211, 173],
    }
}

/// The original half-pixel cell tessellation, independently retaining every visible grid cell.
fn reference(key: &Key) -> UiMesh {
    let size = key.size.map(f32::from_bits);
    let coverage = Coverage::new(key).unwrap();
    let domain = Domain::new(&coverage).unwrap();
    let [width, height] = domain.cells;
    let mut vertices = Vec::new();
    for y in 0..=height {
        for x in 0..=width {
            let point = domain.point(x, y);
            let mut color = key.color;
            color[3] = (f32::from(color[3]) * coverage.at(point)).round() as u8;
            vertices.push(UiMeshVertex {
                position: [point[0] / size[0], point[1] / size[1]],
                clip_z: 0.0,
                clip_w: 1.0,
                uv: [0.0; 2],
                color,
                model_light: 1.0,
                overlay_color: [0.0; 4],
                style_flags: 0,
                alpha_test: false,
            });
        }
    }
    let mut indices = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let a = (y * (width + 1) + x) as u32;
            let corners = [a, a + 1, a + width as u32 + 1, a + width as u32 + 2];
            if corners
                .iter()
                .any(|&index| vertices[index as usize].color[3] != 0)
            {
                indices.extend([
                    corners[0], corners[1], corners[3], corners[0], corners[3], corners[2],
                ]);
            }
        }
    }
    let end = indices.len() as u32;
    UiMesh::new(
        vertices.into(),
        indices.into(),
        Arc::from([UiMeshBatch {
            texture_page: key.page,
            index_range: 0..end,
            blend: UiBlendMode::Alpha,
            depth_test: false,
            depth_write: false,
            alpha_cutoff: None,
        }]),
    )
    .unwrap()
}

/// Rasterizes the constant material's coverage with GPU subpixel precision and top-left ownership.
fn pixels(mesh: &UiMesh, key: &Key) -> Vec<u8> {
    let size = key.size.map(f32::from_bits);
    let dpi = f32::from_bits(key.dpi);
    let [width, height] = size.map(|value| (value * dpi).ceil() as usize + 6);
    let mut alpha = vec![0.0_f64; width * height];
    let edge = |a: [i64; 2], b: [i64; 2], p: [i64; 2]| {
        (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
    };
    let owns = |weight: i64, a: [i64; 2], b: [i64; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        weight > 0 || (weight == 0 && (dy < 0 || (dy == 0 && dx > 0)))
    };
    for triangle in mesh.indices().as_chunks::<3>().0 {
        let mut corners = triangle.map(|index| mesh.vertices()[index as usize]);
        let mut points = corners.map(|vertex| {
            std::array::from_fn::<_, 2, _>(|axis| {
                let origin = [2.25, 2.375][axis];
                ((vertex.position[axis] * size[axis] * dpi + origin) * 256.0).round() as i64
            })
        });
        let mut area = edge(points[0], points[1], points[2]);
        if area == 0 {
            continue;
        }
        if area < 0 {
            points.swap(1, 2);
            corners.swap(1, 2);
            area = -area;
        }
        let [a, b, c] = points;
        let min = std::array::from_fn::<_, 2, _>(|axis| {
            points
                .iter()
                .map(|p| p[axis])
                .min()
                .unwrap()
                .div_euclid(256)
                .max(0) as usize
        });
        let max = std::array::from_fn::<_, 2, _>(|axis| {
            points
                .iter()
                .map(|p| p[axis])
                .max()
                .unwrap()
                .div_euclid(256) as usize
                + 1
        });
        for y in min[1]..max[1].min(height) {
            for x in min[0]..max[0].min(width) {
                let point = [x as i64 * 256 + 128, y as i64 * 256 + 128];
                let weights = [edge(b, c, point), edge(c, a, point), edge(a, b, point)];
                if !owns(weights[0], b, c) || !owns(weights[1], c, a) || !owns(weights[2], a, b) {
                    continue;
                }
                let source = weights
                    .into_iter()
                    .zip(corners)
                    .map(|(weight, vertex)| weight as f64 * f64::from(vertex.color[3]))
                    .sum::<f64>()
                    / (area as f64 * 255.0);
                let pixel = &mut alpha[y * width + x];
                *pixel = source + *pixel * (1.0 - source);
            }
        }
    }
    alpha
        .into_iter()
        .map(|value| (value * 255.0).round() as u8)
        .collect()
}

/// A broad uniform interior no longer spends six expanded vertices on every half-pixel cell.
#[test]
fn opaque_coverage_runs_admit_large_translucent_fills() {
    let key = key(
        json!([[[0, 0], [16, 0], [16, 16], [0, 16]]]),
        None,
        [80.0; 2],
        1.0,
    );
    let original = reference(&key);
    assert!(original.indices().len() > MAX_MESH_INDICES);
    let packed = mesh(&key).unwrap();
    assert!(packed.indices().len() <= MAX_MESH_INDICES);
    assert!(packed.indices().len() * 8 < original.indices().len());
    assert_eq!(pixels(&packed, &key), pixels(&original, &key));
}

/// A complex polygon at four physical pixels per source unit fits without changing edge coverage.
#[test]
fn a_large_complex_silhouette_fits_without_coarsening_its_edges() {
    let contour: Vec<_> = (0..143)
        .map(|index| {
            let angle = index as f32 * std::f32::consts::TAU / 143.0;
            let radius = if index % 6 < 3 { 7.0 } else { 6.0 };
            [8.5 + radius * angle.cos(), 7.5 + radius * angle.sin()]
        })
        .collect();
    let mut key = key(json!([contour]), None, [68.0, 60.0], 1.0);
    key.path.view_box = [0.0, 0.0, 17.0, 15.0].map(f32::to_bits);
    let original = reference(&key);
    assert!(original.indices().len() > MAX_MESH_INDICES);
    let packed = mesh(&key).unwrap();
    assert!(packed.indices().len() <= MAX_MESH_INDICES);
    assert_eq!(pixels(&packed, &key), pixels(&original, &key));
}

/// Edge pixels, holes and round stroke joins remain identical at fractional and high device density.
#[test]
fn packed_interiors_preserve_grid_edge_pixels_at_each_dpi() {
    for dpi in [1.0, 1.25, 2.0] {
        let paths = json!([
            [[0, 1], [13, 0], [16, 9], [11, 16], [0, 12]],
            [[4, 5], [10, 4], [11, 10], [4, 11]],
        ]);
        let fill = key(paths, None, [32.0, 28.0], dpi);
        assert_eq!(
            pixels(&mesh(&fill).unwrap(), &fill),
            pixels(&reference(&fill), &fill)
        );
        let stroke = key(
            json!([[[2, 2], [14, 9], [5, 14]]]),
            Some(2.0),
            [30.0; 2],
            dpi,
        );
        assert_eq!(
            pixels(&mesh(&stroke).unwrap(), &stroke),
            pixels(&reference(&stroke), &stroke)
        );
    }
}

/// Repeated straight stroke edges fit at Retina density without dropping their coverage.
#[test]
fn parallel_strokes_fit_without_coarsening_their_antialiased_edges() {
    let paths: Vec<_> = (0..6)
        .map(|row| {
            [
                [1.0, 1.0 + row as f32 * 2.0],
                [15.0, 1.0 + row as f32 * 2.0],
            ]
        })
        .collect();
    for vertical in [false, true] {
        let paths: Vec<_> = paths
            .iter()
            .map(|line| line.map(|[x, y]| if vertical { [y, x] } else { [x, y] }))
            .collect();
        let key = key(json!(paths), Some(0.8), [60.0; 2], 2.0);
        let original = reference(&key);
        assert!(original.indices().len() > MAX_MESH_INDICES);
        let packed = mesh(&key).expect("parallel strokes must fit the bounded vector mesh");
        assert!(packed.indices().len() <= MAX_MESH_INDICES);
        assert_eq!(pixels(&packed, &key), pixels(&original, &key));
    }
}
