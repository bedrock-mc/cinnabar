use super::*;
use mod_api::*;

/// Admits immutable topology and caches every fact needed by later instance validation.
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
    {
        return Err(error(ErrorCode::Invalid, "geometry topology or vertex"));
    }
    let expanded_vertices =
        drawing::expanded_vertices(vertices.len(), indices.len(), polyline, closed);
    if expanded_vertices > MAX_DRAW_EXPANDED_VERTICES {
        return Err(error(ErrorCode::BudgetExceeded, "expanded vertices"));
    }
    let mut bounds = [vertices[0].position; 2];
    let mut radius_squared = 0_f64;
    let mut distances = Vec::with_capacity(if polyline { vertices.len() } else { 0 });
    let mut previous = vertices[0].position;
    let mut distance = 0.;
    for vertex in &vertices {
        #[cfg(test)]
        super::tests::work::VERTEX_VISITS.with(|visits| visits.set(visits.get() + 1));
        if !vertex
            .position
            .iter()
            .chain(&vertex.uv)
            .chain(&vertex.data)
            .all(|v| v.is_finite())
            || vertex
                .position
                .iter()
                .any(|v| v.abs() > MAX_PRIMITIVE_EXTENT_BLOCKS)
            || !color(vertex.color)
        {
            return Err(error(ErrorCode::Invalid, "geometry topology or vertex"));
        }
        for (axis, &position) in vertex.position.iter().enumerate() {
            bounds[0][axis] = bounds[0][axis].min(position);
            bounds[1][axis] = bounds[1][axis].max(position);
        }
        radius_squared =
            radius_squared.max(vertex.position.iter().map(|v| f64::from(*v).powi(2)).sum());
        if polyline {
            distance += vertex
                .position
                .iter()
                .zip(previous)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>()
                .sqrt();
            distances.push(distance);
            previous = vertex.position;
        }
    }
    let bytes = vertices.len() * std::mem::size_of::<Vertex>()
        + indices.len() * std::mem::size_of::<u32>()
        + distances.len() * std::mem::size_of::<f32>();
    Ok(Geometry {
        bounds,
        radius: radius_squared.sqrt() as f32,
        bytes,
        expanded_vertices,
        vertices: vertices.into(),
        indices: indices.into(),
        distances: distances.into(),
        closed,
        polyline,
    })
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
    let vertex_work = resources
        .instances
        .values()
        .try_fold(0usize, |sum, instance| {
            let geometry = resources.geometry.get(&instance.geometry)?;
            sum.checked_add(geometry.expanded_vertices)
        })
        .ok_or(error(ErrorCode::BudgetExceeded, "vertex work"))?;
    for (amount, limit, label) in [
        (vertex_work, MAX_DRAW_VERTEX_WORK, "vertex work"),
        (
            geometries.iter().map(|g| g.expanded_vertices).sum(),
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
            geometries.iter().map(|g| g.bytes).sum(),
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
    if (instance.material.is_some() && !permissions.render_material)
        || !permissions.world_draw
        || (instance.depth == Depth::Always && !permissions.render_through_walls)
        || (matches!(instance.anchor, Anchor::Entity(_))
            && !(permissions.entity_observe && permissions.entity_highlight))
    {
        return Err(error(ErrorCode::Denied, "drawing authority"));
    }
    if let Anchor::Entity(anchor) = instance.anchor
        && !resources.entities.contains_key(&anchor.entity)
    {
        return Err(error(ErrorCode::StaleHandle, "entity"));
    }
    if let Some(material) = instance.material
        && !resources.materials.contains_key(&material)
    {
        return Err(error(ErrorCode::StaleHandle, "material"));
    }
    if !instance.params.iter().all(|value| value.is_finite()) {
        return Err(error(ErrorCode::Invalid, "material parameters"));
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
        || !extent(geometry, transform)
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

/// Checks cached box and sphere bounds in constant work, including nonuniform signed scales.
/// The sphere is exact for uniform scales; either bound may admit a nonuniform scale.
fn extent(geometry: &Geometry, transform: Transform) -> bool {
    let translation = transform
        .translation
        .iter()
        .map(|v| v * v)
        .sum::<f32>()
        .sqrt();
    let bound = (0..3)
        .map(|axis| {
            (geometry.bounds[0][axis]
                .abs()
                .max(geometry.bounds[1][axis].abs())
                * transform.scale[axis])
                .powi(2)
        })
        .sum::<f32>()
        .sqrt()
        + translation;
    bound <= MAX_PRIMITIVE_EXTENT_BLOCKS
        || geometry.radius * transform.scale.iter().map(|s| s.abs()).fold(0., f32::max)
            + translation
            <= MAX_PRIMITIVE_EXTENT_BLOCKS
}

/// Requires finite linear RGBA with bounded coverage.
fn color(color: [f32; 4]) -> bool {
    color
        .iter()
        .all(|v| v.is_finite() && (0. ..=MAX_DRAW_COLOR_COMPONENT).contains(v))
}
