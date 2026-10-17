use super::*;
use mod_api::*;

/// Validates topology and computes cumulative distance once for a reusable polyline.
pub(super) fn geometry(spec: GeometrySpec) -> Result<Geometry, DrawError> {
    let (vertices, indices, closed, polyline) = match spec {
        GeometrySpec::Triangles { vertices, indices } => (vertices, indices, false, false),
        GeometrySpec::Polyline { vertices, closed } => (vertices, Vec::new(), closed, true),
    };
    if vertices.len() > MAX_DRAW_VERTICES || indices.len() > MAX_DRAW_INDICES {
        return Err(error(ErrorCode::BudgetExceeded, "geometry elements"));
    }
    if vertices.len() < if polyline { 2 } else { 3 }
        || (!polyline && (indices.is_empty() || !indices.len().is_multiple_of(3)))
        || indices
            .iter()
            .any(|&index| index as usize >= vertices.len())
        || vertices.iter().any(|v| {
            !v.position
                .iter()
                .chain(&v.uv)
                .chain(&v.data)
                .all(|v| v.is_finite())
                || v.position
                    .iter()
                    .any(|v| v.abs() > MAX_PRIMITIVE_EXTENT_BLOCKS)
                || !color(v.color)
        })
    {
        return Err(error(ErrorCode::Invalid, "geometry topology or vertex"));
    }
    let mut distances = Vec::new();
    if polyline {
        distances.push(0.);
        let mut distance = 0.;
        for pair in vertices.windows(2) {
            distance += pair[0]
                .position
                .iter()
                .zip(pair[1].position)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>()
                .sqrt();
            distances.push(distance);
        }
    }
    let bounds = std::array::from_fn(|side| {
        std::array::from_fn(|axis| {
            vertices
                .iter()
                .map(|v| v.position[axis])
                .reduce(if side == 0 { f32::min } else { f32::max })
                .expect("validated nonempty geometry")
        })
    });
    let geometry = Geometry {
        bounds,
        vertices: vertices.into(),
        indices: indices.into(),
        distances: distances.into(),
        closed,
        polyline,
    };
    if mesh::expanded_vertices(&geometry) > MAX_DRAW_EXPANDED_VERTICES {
        return Err(error(ErrorCode::BudgetExceeded, "expanded vertices"));
    }
    Ok(geometry)
}

/// Measures admitted immutable payloads; shared instances never multiply geometry charges.
pub(super) fn bytes(geometry: &Geometry) -> usize {
    geometry.vertices.len() * std::mem::size_of::<Vertex>()
        + geometry.indices.len() * std::mem::size_of::<u32>()
        + geometry.distances.len() * std::mem::size_of::<f32>()
}

/// Accounts retiring snapshots until their renderer references have been released.
pub(super) fn budget(resources: &Resources, retired: &[Arc<Geometry>]) -> Result<(), DrawError> {
    let geometries: Vec<_> = resources
        .geometry
        .values()
        .chain(retired.iter().filter(|g| {
            Arc::strong_count(g) > 1
                && !resources.geometry.values().any(|live| Arc::ptr_eq(g, live))
        }))
        .collect();
    for (amount, limit, label) in [
        (
            geometries.iter().map(|g| mesh::expanded_vertices(g)).sum(),
            MAX_DRAW_EXPANDED_VERTICES,
            "expanded vertices",
        ),
        (geometries.len(), MAX_DRAW_GEOMETRIES, "geometries"),
        (resources.instances.len(), MAX_DRAW_INSTANCES, "instances"),
        (
            geometries.iter().map(|g| g.vertices.len()).sum(),
            MAX_DRAW_VERTICES,
            "vertices",
        ),
        (
            geometries.iter().map(|g| g.indices.len()).sum(),
            MAX_DRAW_INDICES,
            "indices",
        ),
        (
            geometries.iter().map(|g| bytes(g)).sum(),
            MAX_DRAW_GEOMETRY_BYTES,
            "geometry bytes",
        ),
    ] {
        if amount > limit {
            return Err(error(ErrorCode::BudgetExceeded, label));
        }
    }
    Ok(())
}

/// Validates finite TRS, bounded transformed extent, references and elevated draw authority.
pub(super) fn instance(
    instance: &Instance,
    resources: &Resources,
    permissions: DrawingPermissions,
) -> Result<(), DrawError> {
    if !permissions.world_draw
        || (instance.depth == Depth::Always && !permissions.render_through_walls)
        || (matches!(instance.anchor, Anchor::Entity(_))
            && !(permissions.entity_observe && permissions.entity_highlight))
    {
        return Err(error(ErrorCode::Denied, "drawing authority"));
    }
    if let Anchor::Entity(anchor) = instance.anchor {
        if !resources.entities.contains_key(&anchor.entity) {
            return Err(error(ErrorCode::StaleHandle, "entity"));
        }
    }
    let geometry = resources
        .geometry
        .get(&instance.geometry)
        .ok_or(error(ErrorCode::StaleHandle, "geometry"))?;
    let transform = instance.transform;
    let rotation_length = transform.rotation.iter().map(|v| v * v).sum::<f32>();
    if !transform
        .translation
        .iter()
        .chain(&transform.rotation)
        .chain(&transform.scale)
        .all(|v| v.is_finite())
        || (rotation_length - 1.).abs() > 0.001
        || !color(instance.tint)
        || !instance.width.is_finite()
        || instance.width <= 0.
        || instance.width > MAX_PRIMITIVE_EXTENT_BLOCKS
        || (0..3)
            .map(|axis| {
                (geometry.bounds[0][axis]
                    .abs()
                    .max(geometry.bounds[1][axis].abs())
                    * transform.scale[axis])
                    .powi(2)
            })
            .sum::<f32>()
            .sqrt()
            + transform
                .translation
                .iter()
                .map(|v| v * v)
                .sum::<f32>()
                .sqrt()
            > MAX_PRIMITIVE_EXTENT_BLOCKS
    {
        return Err(error(
            ErrorCode::Invalid,
            "instance transform, width or tint",
        ));
    }
    let valid = match instance.anchor {
        Anchor::World { origin, .. } => origin
            .iter()
            .all(|v| v.is_finite() && v.abs() <= f64::from(MAX_PRIMITIVE_COORDINATE)),
        Anchor::Block { position, .. } => position
            .iter()
            .all(|v| f64::from(*v).abs() <= f64::from(MAX_PRIMITIVE_COORDINATE)),
        Anchor::Camera | Anchor::Entity(_) => true,
    };
    if !valid {
        return Err(error(ErrorCode::Invalid, "anchor"));
    }
    Ok(())
}

/// Requires finite linear RGBA with bounded coverage.
fn color(color: [f32; 4]) -> bool {
    color
        .iter()
        .all(|v| v.is_finite() && (0. ..=1.).contains(v))
}
