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
