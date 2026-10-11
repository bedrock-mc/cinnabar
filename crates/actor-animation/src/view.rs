use assets::SkinGeometryBounds;

impl super::ActorRigSnapshot<'_> {
    /// Uses the displayed model's authored visibility box independently of collision metadata.
    #[must_use]
    pub fn culling_bounds(&self) -> SkinGeometryBounds {
        if let Some(geometry) = self.skin_geometry {
            return geometry.visible_bounds.unwrap_or_default();
        }
        self.geometry_source()
            .and_then(|(assets, geometry)| assets.geometry_visible_bounds(geometry))
            .unwrap_or_default()
    }
}

/// The view animation runs for: actors outside it hold their pose, since vanilla evaluates
/// pre-animation, animation and render controllers only for actors it renders.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorAnimationView {
    /// Clip-space half-spaces `[a, b, c, d]`, inside where `a*x + b*y + c*z + d >= 0`.
    pub planes: [[f32; 4]; 6],
    pub camera: [f32; 3],
    /// Players farther than this from the camera hold their pose.
    pub player_distance: f32,
    /// Other actors farther than this from the camera on any axis hold their pose.
    pub entity_radius: f32,
}

impl ActorAnimationView {
    /// Keeps authored model bounds inside the guard-banded animation frustum.
    #[must_use]
    pub fn admits(
        &self,
        feet: [f32; 3],
        scale: f32,
        player: bool,
        bounds: SkinGeometryBounds,
    ) -> bool {
        if feet.iter().any(|value| !value.is_finite()) {
            return true;
        }
        let offset: [f32; 3] = std::array::from_fn(|axis| feet[axis] - self.camera[axis]);
        if player {
            let head = [offset[0], offset[1] + 1.0, offset[2]];
            if head.iter().map(|value| value * value).sum::<f32>()
                > self.player_distance * self.player_distance
            {
                return false;
            }
        } else if offset.iter().any(|value| value.abs() > self.entity_radius) {
            return false;
        }
        let (low, high) = bounds.at(feet, scale);
        self.planes.iter().all(|plane| {
            let corner: [f32; 3] = std::array::from_fn(|axis| {
                if plane[axis] >= 0.0 {
                    high[axis]
                } else {
                    low[axis]
                }
            });
            plane[0] * corner[0] + plane[1] * corner[1] + plane[2] * corner[2] + plane[3] >= 0.0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ActorAnimationView;

    /// The half-space `x <= 10` alone, with generous distances.
    fn wall() -> ActorAnimationView {
        ActorAnimationView {
            planes: [[-1.0, 0.0, 0.0, 10.0]; 6],
            camera: [0.0; 3],
            player_distance: 100.0,
            entity_radius: 72.0,
        }
    }

    #[test]
    fn a_box_straddling_a_plane_is_admitted_and_one_past_it_is_not() {
        assert!(wall().admits([10.4, 0.0, 0.0], 1.0, false, Default::default()));
        assert!(!wall().admits([10.6, 0.0, 0.0], 1.0, false, Default::default()));
        // A scaled box reaches farther across the plane.
        assert!(wall().admits([11.5, 0.0, 0.0], 4.0, false, Default::default()));
    }

    #[test]
    fn entities_use_the_candidate_cube_and_players_the_distance() {
        let view = ActorAnimationView {
            planes: [[0.0, 0.0, 0.0, 1.0]; 6],
            ..wall()
        };
        assert!(!view.admits([0.0, 0.0, 73.0], 1.0, false, Default::default()));
        assert!(view.admits([0.0, 0.0, 73.0], 1.0, true, Default::default()));
        assert!(!view.admits([0.0, 0.0, 101.0], 1.0, true, Default::default()));
    }

    #[test]
    fn authored_visibility_box_keeps_edge_skin_animation_running() {
        let bounds = assets::SkinGeometryBounds {
            center: [0.0, 2.0, 0.0],
            half_extents: [1.5, 2.0, 1.5],
        };
        assert!(!wall().admits([11.0, 0.0, 0.0], 1.0, true, Default::default()));
        assert!(wall().admits([11.0, 0.0, 0.0], 1.0, true, bounds));
        assert!(!wall().admits([11.6, 0.0, 0.0], 1.0, true, bounds));
        let above = ActorAnimationView {
            planes: [[0.0, 1.0, 0.0, -3.0]; 6],
            ..wall()
        };
        assert!(!above.admits([0.0; 3], 1.0, true, Default::default()));
        assert!(above.admits([0.0; 3], 1.0, true, bounds));
    }
}
