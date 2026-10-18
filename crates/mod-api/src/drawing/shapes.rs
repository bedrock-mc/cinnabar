//! Pure local-coordinate shape builders for the generated retained drawing API.
//!
//! Each result contains independent polylines, with no connecting segments between them.
//! Upload each geometry once with `api::create_geometry`, then reuse its handle in instances.
//! Builders perform no observation or host calls. Host admission and aggregate quotas remain
//! authoritative, including earlier imports in the callback and the owner's other resources.

use super::{
    api::{DrawError, ErrorCode, GeometrySpec, Polyline, Vertex},
    expanded_vertices,
};
use crate::{
    MAX_DRAW_COLOR_COMPONENT, MAX_DRAW_EXPANDED_VERTICES, MAX_DRAW_GEOMETRIES,
    MAX_DRAW_GEOMETRY_BYTES, MAX_DRAW_MUTATION_BYTES, MAX_DRAW_VERTICES,
    MAX_PRIMITIVE_EXTENT_BLOCKS, MAX_RENDER_CALLS,
};

#[cfg(test)]
mod tests;

/// Emits the twelve box edges once, ordered by the starting corner's XYZ bits then axis.
pub fn box_wireframe(
    bounds: [[f32; 3]; 2],
    color: [f32; 4],
) -> Result<Vec<GeometrySpec>, DrawError> {
    style(color)?;
    let [min, max] = bounds;
    if !bounds.into_iter().all(point) || !(0..3).all(|axis| min[axis] < max[axis]) {
        return Err(error(ErrorCode::Invalid, "box bounds"));
    }
    budget(12, 2, false)?;
    let corners: [[f32; 3]; 8] = std::array::from_fn(|i| {
        std::array::from_fn(|axis| {
            if i & (1 << axis) == 0 {
                min[axis]
            } else {
                max[axis]
            }
        })
    });
    let mut lines = Vec::with_capacity(12);
    for a in 0..corners.len() {
        for axis in 0..3 {
            if a & (1 << axis) == 0 {
                lines.push(line([corners[a], corners[a | (1 << axis)]], color, false));
            }
        }
    }
    Ok(lines)
}

/// Builds twelve edges from caller-supplied, authorized local entity bounds.
/// This helper does not query an entity, authorize observation or create an entity anchor.
pub fn entity_bounding_box(
    bounds: [[f32; 3]; 2],
    color: [f32; 4],
) -> Result<Vec<GeometrySpec>, DrawError> {
    box_wireframe(bounds, color)
}

/// Builds a closed XZ circle at the origin, starting at +X and proceeding towards +Z.
/// The closing edge is represented by `closed`, without a duplicate endpoint.
pub fn circle(radius: f32, segments: u32, color: [f32; 4]) -> Result<Vec<GeometrySpec>, DrawError> {
    circle_budget(radius, segments, color, 1)?;
    Ok(vec![line(ring(radius, segments), color, true)])
}

/// Builds three closed great circles in XZ, XY and YZ order at the origin.
pub fn sphere_wireframe(
    radius: f32,
    segments: u32,
    color: [f32; 4],
) -> Result<Vec<GeometrySpec>, DrawError> {
    circle_budget(radius, segments, color, 3)?;
    let points = ring(radius, segments);
    Ok(vec![
        line(points.iter().copied(), color, true),
        line(points.iter().map(|&[x, _, z]| [x, z, 0.]), color, true),
        line(points.iter().map(|&[x, _, z]| [0., x, z]), color, true),
    ])
}

/// Builds an open path or closed polyline, preserving point order and repeated points.
pub fn polyline(
    points: &[[f32; 3]],
    closed: bool,
    color: [f32; 4],
) -> Result<Vec<GeometrySpec>, DrawError> {
    style(color)?;
    path_valid(points, closed)?;
    Ok(vec![line(points.iter().copied(), color, closed)])
}

/// Builds an open path with linear RGBA interpolated by cumulative distance.
/// Repeated points share a color; an entirely zero-length path uses the starting color.
pub fn path_gradient(
    points: &[[f32; 3]],
    start: [f32; 4],
    end: [f32; 4],
) -> Result<Vec<GeometrySpec>, DrawError> {
    style(start)?;
    style(end)?;
    path_valid(points, false)?;
    let total: f32 = points.windows(2).map(|p| distance(p[0], p[1])).sum();
    let mut traveled = 0.;
    let vertices = points
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            if i > 0 {
                traveled += distance(points[i - 1], p);
            }
            let t = if total > 0. { traveled / total } else { 0. };
            let color = if t == 0. {
                start
            } else if t == 1. {
                end
            } else {
                std::array::from_fn(|axis| start[axis] + (end[axis] - start[axis]) * t)
            };
            vertex(p, color)
        })
        .collect();
    Ok(vec![GeometrySpec::Line(Polyline {
        vertices,
        closed: false,
    })])
}

/// Builds a shaft and two head wings as three independent segments.
/// Head length is measured back from the tip; head width is the full wing span.
/// The wing plane uses the world axis least aligned with the shaft (XYZ tie order).
pub fn arrow(
    start: [f32; 3],
    end: [f32; 3],
    head_length: f32,
    head_width: f32,
    color: [f32; 4],
) -> Result<Vec<GeometrySpec>, DrawError> {
    style(color)?;
    let length = distance(start, end);
    if !point(start)
        || !point(end)
        || !extent(head_length)
        || !extent(head_width)
        || head_length > length
        || length == 0.
    {
        return Err(error(ErrorCode::Invalid, "arrow dimensions"));
    }
    budget(3, 2, false)?;
    let direction: [f32; 3] = std::array::from_fn(|i| (end[i] - start[i]) / length);
    let axis = (0..3)
        .min_by(|&a, &b| direction[a].abs().total_cmp(&direction[b].abs()))
        .unwrap();
    let mut reference = [0.; 3];
    reference[axis] = 1.;
    let side = [
        direction[1] * reference[2] - direction[2] * reference[1],
        direction[2] * reference[0] - direction[0] * reference[2],
        direction[0] * reference[1] - direction[1] * reference[0],
    ];
    let side_length = distance([0.; 3], side);
    let wings = [-1., 1.].map(|sign| {
        std::array::from_fn(|i| {
            end[i] - direction[i] * head_length + sign * side[i] / side_length * head_width * 0.5
        })
    });
    if !wings.into_iter().all(point) {
        return Err(error(ErrorCode::Invalid, "arrow head extent"));
    }
    Ok(vec![
        line([start, end], color, false),
        line([end, wings[0]], color, false),
        line([end, wings[1]], color, false),
    ])
}

/// Builds an XZ grid from the origin to `size`, including both outside edges.
/// Each positive cell count subdivides that axis into equal intervals.
pub fn grid(
    size: [f32; 2],
    cells: [u32; 2],
    color: [f32; 4],
) -> Result<Vec<GeometrySpec>, DrawError> {
    style(color)?;
    if !size.into_iter().all(extent) || cells.contains(&0) {
        return Err(error(ErrorCode::Invalid, "grid dimensions"));
    }
    let count = u64::from(cells[0]) + u64::from(cells[1]) + 2;
    let count =
        usize::try_from(count).map_err(|_| error(ErrorCode::BudgetExceeded, "grid lines"))?;
    budget(count, 2, false)?;
    let mut lines = Vec::with_capacity(count);
    for axis in 0..2 {
        for i in 0..=cells[axis] {
            let offset = size[axis] * (i as f32 / cells[axis] as f32);
            let ends = if axis == 0 {
                [[offset, 0., 0.], [offset, 0., size[1]]]
            } else {
                [[0., 0., offset], [size[0], 0., offset]]
            };
            lines.push(line(ends, color, false));
        }
    }
    Ok(lines)
}

/// Builds the twelve edges of caller-supplied local chunk bounds.
/// The caller supplies the chunk dimensions; no world observation is performed.
pub fn chunk_border(
    bounds: [[f32; 3]; 2],
    color: [f32; 4],
) -> Result<Vec<GeometrySpec>, DrawError> {
    box_wireframe(bounds, color)
}

/// Rejects a shape that cannot fit an otherwise empty owner and callback before allocation.
fn budget(lines: usize, vertices_per_line: usize, closed: bool) -> Result<(), DrawError> {
    let vertices = lines.saturating_mul(vertices_per_line);
    let bytes = vertices.saturating_mul(std::mem::size_of::<Vertex>() + std::mem::size_of::<f32>());
    let expanded = expanded_vertices(vertices_per_line, 0, true, closed).saturating_mul(lines);
    if lines > MAX_DRAW_GEOMETRIES.min(MAX_RENDER_CALLS)
        || vertices > MAX_DRAW_VERTICES
        || bytes > MAX_DRAW_GEOMETRY_BYTES.min(MAX_DRAW_MUTATION_BYTES)
        || expanded > MAX_DRAW_EXPANDED_VERTICES
    {
        return Err(error(ErrorCode::BudgetExceeded, "shape geometry"));
    }
    Ok(())
}

/// Checks ring cardinality and total budget before evaluating trigonometry or allocating.
fn circle_budget(
    radius: f32,
    segments: u32,
    color: [f32; 4],
    rings: usize,
) -> Result<(), DrawError> {
    style(color)?;
    if !extent(radius) || segments < 3 {
        return Err(error(ErrorCode::Invalid, "circle dimensions"));
    }
    budget(rings, segments as usize, true)
}

/// Checks point payloads after rejecting excessive cardinality, without allocating.
fn path_valid(points: &[[f32; 3]], closed: bool) -> Result<(), DrawError> {
    budget(1, points.len(), closed)?;
    if points.len() < if closed { 3 } else { 2 } || !points.iter().copied().all(point) {
        return Err(error(ErrorCode::Invalid, "path points"));
    }
    Ok(())
}

/// Generates evenly spaced circle points, with exact cardinal axes when present.
fn ring(radius: f32, segments: u32) -> Vec<[f32; 3]> {
    (0..segments)
        .map(|i| {
            let quarter = u64::from(i) * 4;
            if quarter.is_multiple_of(u64::from(segments)) {
                return match quarter / u64::from(segments) {
                    0 => [radius, 0., 0.],
                    1 => [0., 0., radius],
                    2 => [-radius, 0., 0.],
                    _ => [0., 0., -radius],
                };
            }
            let angle = std::f32::consts::TAU * (i as f32 / segments as f32);
            let (sin, cos) = angle.sin_cos();
            [radius * cos, 0., radius * sin]
        })
        .collect()
}

/// Creates a single independent line with neutral UV and custom attributes.
fn line(points: impl IntoIterator<Item = [f32; 3]>, color: [f32; 4], closed: bool) -> GeometrySpec {
    GeometrySpec::Line(Polyline {
        vertices: points.into_iter().map(|p| vertex(p, color)).collect(),
        closed,
    })
}

/// Packs a linear color and local position directly into the generated WIT record.
fn vertex(position: [f32; 3], color: [f32; 4]) -> Vertex {
    Vertex {
        position: position.into(),
        color: color.into(),
        uv: (0., 0.),
        data: (0., 0., 0., 0.),
    }
}

/// Accepts only finite local positions within the owning geometry extent.
fn point(p: [f32; 3]) -> bool {
    p.into_iter()
        .all(|v| v.is_finite() && v.abs() <= MAX_PRIMITIVE_EXTENT_BLOCKS)
}

/// Accepts a positive finite shape size within the owning extent limit.
fn extent(v: f32) -> bool {
    v.is_finite() && v > 0. && v <= MAX_PRIMITIVE_EXTENT_BLOCKS
}

/// Validates linear RGBA against the retained unlit color range.
fn style(color: [f32; 4]) -> Result<(), DrawError> {
    if color
        .into_iter()
        .all(|v| v.is_finite() && (0. ..=MAX_DRAW_COLOR_COMPONENT).contains(&v))
    {
        Ok(())
    } else {
        Err(error(ErrorCode::Invalid, "shape color"))
    }
}

/// Computes a segment length for bounded, finite local points.
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (b[i] - a[i]).powi(2)).sum::<f32>().sqrt()
}

/// Returns a typed failure without including caller data in its diagnostic.
fn error(code: ErrorCode, detail: &str) -> DrawError {
    DrawError {
        code,
        detail: detail.into(),
    }
}
