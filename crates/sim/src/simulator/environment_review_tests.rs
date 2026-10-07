use super::environment::*;
use crate::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    SurfaceResponse, Vec3, WorldQueryError,
};

struct LocalFacts {
    block: [i32; 3],
    facts: BlockPhysicsFacts,
}
impl CollisionWorld for LocalFacts {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(vec![]))
    }
    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Ok(BlockPhysicsSample {
            layers: Box::new([if block == self.block {
                self.facts
            } else {
                plain()
            }]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}
/// Supplies ordinary facts for cells outside the contact fixture.
fn plain() -> BlockPhysicsFacts {
    BlockPhysicsFacts {
        friction: 0.6,
        horizontal_speed_factor: 1.0,
        vertical_speed_factor: 1.0,
        fluid_height_blocks: 0.0,
        flags: BlockPhysicsFlags::default(),
        surface_response: SurfaceResponse::None,
    }
}
#[test]
fn review_support_none_cannot_erase_an_overlapping_bubble_column() {
    let world = LocalFacts {
        block: [-1, 1, 0],
        facts: BlockPhysicsFacts {
            flags: BlockPhysicsFlags::WATER,
            surface_response: SurfaceResponse::BubbleUp,
            fluid_height_blocks: 1.0,
            ..plain()
        },
    };
    let sampled = sample(&world, Vec3::new(0.05, 1.0, 0.5), Vec3::ZERO, 1.8, None).unwrap();
    assert_eq!(sampled.movement.surface_response, SurfaceResponse::BubbleUp);
}
#[test]
fn review_swept_only_cells_cannot_apply_body_contact_effects() {
    for flags in [
        BlockPhysicsFlags::CLIMBABLE,
        BlockPhysicsFlags::POWDER_SNOW,
        BlockPhysicsFlags::SCAFFOLDING,
        BlockPhysicsFlags::PASSABLE,
    ] {
        let world = LocalFacts {
            block: [1, 1, 0],
            facts: BlockPhysicsFacts {
                flags,
                horizontal_speed_factor: 0.25,
                vertical_speed_factor: 0.5,
                ..plain()
            },
        };
        let sampled = sample(
            &world,
            Vec3::new(0.5, 1.0, 0.5),
            Vec3::new(0.5, 0.0, 0.0),
            1.8,
            None,
        )
        .unwrap();
        assert!(!sampled.movement.on_climbable);
        assert!(!sampled.movement.in_powder_snow);
        assert!(!sampled.movement.in_scaffolding);
        assert_eq!(sampled.movement.horizontal_speed_factor, 1.0);
        assert_eq!(sampled.movement.vertical_speed_factor, 1.0);
    }
}
/// Reports one cell as air with the given table friction; other cells are plain.
struct ProbedCell {
    friction: f64,
    air: Option<bool>,
}
impl CollisionWorld for ProbedCell {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(vec![]))
    }
    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: if block == [0, 0, 0] { self.friction } else { 0.6 },
                ..plain()
            }]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
    fn primary_is_air(
        &self,
        block: [i32; 3],
    ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        Ok(self
            .air
            .map(|air| CollisionQuery::synthetic(air && block == [0, 0, 0])))
    }
}
#[test]
fn review_air_under_the_feet_probe_keeps_default_friction() {
    let friction = |world: &ProbedCell| {
        sample(world, Vec3::new(0.5, 1.0, 0.5), Vec3::ZERO, 1.8, None)
            .unwrap()
            .friction
    };
    // Past a block edge the probe hits air, whose 0.9 table friction must not apply.
    let edge = ProbedCell { friction: 0.9, air: Some(true) };
    assert_eq!(friction(&edge), 0.6);
    let ice = ProbedCell { friction: 0.98, air: Some(false) };
    assert_eq!(friction(&ice), 0.98);
    let without_material = ProbedCell { friction: 0.98, air: None };
    assert_eq!(friction(&without_material), 0.98);
}
