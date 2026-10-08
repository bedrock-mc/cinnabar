//! Native portal contact uses the actor body and full block cells, independently of visual axis.

use sim::Aabb;

// Contacts exclude exact block boundaries without growing the actor body.
const CONTACT_INSET: f32 = 0.001;

pub(super) fn touches_portal(body: Aabb, mut is_portal: impl FnMut([i32; 3]) -> bool) -> bool {
    let minimum =
        [body.min.x, body.min.y, body.min.z].map(|value| (value as f32 + CONTACT_INSET).floor());
    let maximum =
        [body.max.x, body.max.y, body.max.z].map(|value| (value as f32 - CONTACT_INSET).floor());
    if minimum.into_iter().chain(maximum).any(|value| {
        !value.is_finite()
            || f64::from(value) < f64::from(i32::MIN)
            || f64::from(value) > f64::from(i32::MAX)
    }) {
        return false;
    }
    let minimum = minimum.map(|value| value as i32);
    let maximum = maximum.map(|value| value as i32);
    // Contact uses complete cells rather than the portal's thin visual bounds.
    for x in minimum[0]..=maximum[0] {
        for y in minimum[1]..=maximum[1] {
            for z in minimum[2]..=maximum[2] {
                if is_portal([x, y, z]) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use sim::{MovementMode, Vec3};

    use super::*;

    fn touches(body: Aabb, portal: [i32; 3]) -> bool {
        touches_portal(body, |block| block == portal)
    }

    #[test]
    fn shoulder_contact_counts_before_the_body_center_enters_the_block() {
        let body = Aabb::player_at(Vec3::new(-0.2, 0.0, 0.5));
        assert!(touches(body, [0, 0, 0]));
        // Full portal cell contact also precedes its centered visual plane.
        assert!(body.max.x < 0.375);
        assert!(!touches(
            Aabb::player_at(Vec3::new(-0.31, 0.0, 0.5)),
            [0, 0, 0]
        ));
    }

    #[test]
    fn contact_at_an_intermediate_body_height_is_not_missed() {
        let body = Aabb::player_at(Vec3::new(0.5, 0.9, 0.5));
        assert_eq!(body.min.y.floor(), 0.0);
        assert_eq!(
            (body.min.y + f64::from(protocol::STANDING_PLAYER_EYE_HEIGHT)).floor(),
            2.0
        );
        assert!(touches(body, [0, 1, 0]));
    }

    #[test]
    fn exact_faces_and_the_native_inset_do_not_trigger_contact() {
        let min = Vec3::new(-sim::PLAYER_WIDTH, 0.0, 0.0);
        for edge in [0.0, f64::from(CONTACT_INSET) * 0.5] {
            let body = Aabb::new(min, Vec3::new(edge, sim::PLAYER_HEIGHT, 1.0));
            assert!(!touches(body, [0, 0, 0]));
        }
        let body = Aabb::new(
            min,
            Vec3::new(f64::from(CONTACT_INSET) * 2.0, sim::PLAYER_HEIGHT, 1.0),
        );
        assert!(touches(body, [0, 0, 0]));
    }

    #[test]
    fn low_poses_only_visit_their_actual_body_height_and_negative_cells() {
        let feet = Vec3::new(-0.1, 0.0, -0.1);
        let standing = Aabb::player_at(feet);
        let swimming =
            Aabb::player_with_height_at(feet, MovementMode::Swimming.hitbox_height(false));
        assert!(touches(standing, [-1, 1, -1]));
        assert!(!touches(swimming, [-1, 1, -1]));
        assert!(touches(swimming, [-1, 0, -1]));
    }

    #[test]
    fn nonfinite_body_never_queries_world_cells() {
        let body = Aabb::new(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
        assert!(!touches_portal(body, |_| panic!(
            "invalid body must not query world"
        )));
    }
}
