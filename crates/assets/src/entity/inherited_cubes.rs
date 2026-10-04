//! Native legacy geometry inheritance, shared by actor meshes and player skins.
use super::EntityGeometryBone;

impl EntityGeometryBone {
    /// A same-name child appends cubes to its inherited part; only explicit `reset: true`
    /// removes the inherited cubes. Bone properties are resolved separately by each caller.
    ///
    /// Reset rewinds
    /// the cube vector's end, then authored cubes append at its existing end. The pinned
    /// vanilla adult sheep uses this contract for the face beneath its wool overlay.
    /// `maximum` is the caller's remaining cube budget for this bone, including its existing
    /// cubes. Returns the new count, or `None` without mutation or allocation on overflow.
    pub fn append_inherited_cubes(&mut self, child: &Self, maximum: usize) -> Option<usize> {
        let inherited = if child.reset == Some(true) {
            0
        } else {
            self.cubes.len()
        };
        let count = inherited.checked_add(child.cubes.len())?;
        if count > maximum {
            return None;
        }
        if child.reset == Some(true) {
            self.cubes = Box::default();
        }
        if !child.cubes.is_empty() {
            let mut cubes = std::mem::take(&mut self.cubes).into_vec();
            cubes.extend_from_slice(&child.cubes);
            self.cubes = cubes.into_boxed_slice();
        }
        Some(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bone(origin: f32) -> EntityGeometryBone {
        let data = serde_json::json!({
            "format_version": "1.12.0",
            "minecraft:geometry": [{
                "description": {"identifier": "geometry.fixture"},
                "bones": [{"name": "head", "cubes": [{
                    "origin": [origin, 0, 0], "size": [1, 1, 1], "uv": [0, 0]
                }]}]
            }]
        });
        let geometry = crate::parse_skin_geometry(
            r#"{"geometry":{"default":"geometry.fixture"}}"#,
            &data.to_string(),
        )
        .unwrap()
        .unwrap();
        geometry.bones[0].clone()
    }

    #[test]
    fn inherited_cubes_append_in_order_across_multiple_levels() {
        let mut base = bone(1.0);
        base.append_inherited_cubes(&bone(2.0), crate::MAX_ENTITY_GEOMETRY_CUBES)
            .unwrap();
        let mut child = bone(3.0);
        child.reset = Some(false);
        base.append_inherited_cubes(&child, crate::MAX_ENTITY_GEOMETRY_CUBES)
            .unwrap();
        assert_eq!(
            base.cubes
                .iter()
                .map(|cube| cube.origin[0].get())
                .collect::<Vec<_>>(),
            [1.0, 2.0, 3.0]
        );
    }

    #[test]
    fn explicit_reset_replaces_or_removes_inherited_cubes_but_is_not_sticky() {
        let mut base = bone(1.0);
        let mut child = bone(2.0);
        child.reset = Some(true);
        base.append_inherited_cubes(&child, crate::MAX_ENTITY_GEOMETRY_CUBES)
            .unwrap();
        assert_eq!(base.cubes.len(), 1);
        assert_eq!(base.cubes[0].origin[0].get(), 2.0);
        base.append_inherited_cubes(&bone(3.0), crate::MAX_ENTITY_GEOMETRY_CUBES)
            .unwrap();
        assert_eq!(base.cubes.len(), 2);
        child.cubes = Box::default();
        base.append_inherited_cubes(&child, crate::MAX_ENTITY_GEOMETRY_CUBES)
            .unwrap();
        assert!(base.cubes.is_empty());
    }

    #[test]
    fn inherited_cube_budget_is_checked_before_mutation_and_reset_releases_it() {
        let mut base = bone(1.0);
        let mut child = bone(2.0);
        let maximum = base.cubes.len();
        let unchanged = base.clone();
        assert_eq!(base.append_inherited_cubes(&child, maximum), None);
        assert_eq!(base, unchanged);
        child.reset = Some(true);
        assert_eq!(base.append_inherited_cubes(&child, maximum), Some(maximum));
        assert_eq!(base.cubes, child.cubes);
    }
}
