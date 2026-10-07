use std::cell::Cell;

use bevy::prelude::{EulerRot, Quat, Transform, Vec3};
use semantic_input::PerspectiveMode;
use sim::{
    Aabb, CollisionQuery, CollisionRegistry, CollisionWorld, LenientSkipCounts, PaletteWorld,
    Vec3 as SimVec3, WorldQueryError,
};
use world::{BlockUpdate, ChunkKey, ChunkStore, RawBlockIds, SubChunkKey};

use super::{
    CameraRig, CameraSettingsAuthority, THIRD_PERSON_COLLISION_EPSILON_BLOCKS,
    THIRD_PERSON_COLLISION_RADIUS_BLOCKS, collision_safe_perspective_pose, collision_safe_rig_pose,
    perspective_pose, rig_pose,
};

struct Walls(Vec<Aabb>);

impl CollisionWorld for Walls {
    /// Visits retained fixture shapes without constructing an owned query result.
    fn visit_collision_boxes_camera_lenient(
        &self,
        _query: Aabb,
        visitor: &mut dyn FnMut(Aabb),
    ) -> Result<LenientSkipCounts, WorldQueryError> {
        for shape in &self.0 {
            visitor(*shape);
        }
        Ok(LenientSkipCounts::default())
    }

    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(self.0.clone()))
    }
}

fn shoulder() -> CameraRig {
    CameraRig {
        offset: Vec3::new(0.8, 0.4, 3.0),
        roll_radians: 0.0,
        fov_delta_degrees: 0.0,
    }
}

#[test]
fn rig_forces_third_person_back_and_clearing_restores_the_player_choice() {
    let mut settings = CameraSettingsAuthority::default();
    settings.reset_perspective();
    settings.set_rig(Some(shoulder()));
    assert_eq!(settings.perspective(), PerspectiveMode::ThirdPersonBack);
    settings.set_rig(None);
    assert_eq!(settings.perspective(), PerspectiveMode::FirstPerson);
    settings.set_rig(Some(CameraRig {
        roll_radians: f32::NAN,
        ..shoulder()
    }));
    assert_eq!(settings.rig(), None);
}

#[test]
fn rig_offset_follows_the_eye_look_in_camera_local_axes() {
    let eye = Vec3::new(10.0, 70.0, 10.0);
    let unturned = rig_pose(eye, Quat::IDENTITY, shoulder());
    assert!(
        unturned
            .translation
            .abs_diff_eq(Vec3::new(10.8, 70.4, 13.0), 1e-5)
    );
    assert_eq!(unturned.rotation, Quat::IDENTITY);
    // A quarter turn left moves "right" onto -Z and "back" onto +X.
    let turned = Quat::from_euler(EulerRot::YXZ, std::f32::consts::FRAC_PI_2, 0.0, 0.0);
    let pose = rig_pose(eye, turned, shoulder());
    assert!(
        pose.translation
            .abs_diff_eq(Vec3::new(13.0, 70.4, 9.2), 1e-4)
    );
    assert_eq!(pose.rotation, turned);
}

#[test]
fn rig_boom_stops_before_a_wall_behind_the_shoulder() {
    let eye = Vec3::new(0.0, 2.0, 0.0);
    let rig = CameraRig {
        offset: Vec3::new(0.0, 0.0, 3.0),
        ..shoulder()
    };
    let wall = Walls(vec![Aabb::new(
        SimVec3::new(-1.0, 1.0, 2.0),
        SimVec3::new(1.0, 3.0, 3.0),
    )]);
    let pose = collision_safe_rig_pose(eye, Quat::IDENTITY, rig, &wall);
    assert!(pose.translation.abs_diff_eq(
        Vec3::new(
            0.0,
            2.0,
            4.02_f32.sqrt() - THIRD_PERSON_COLLISION_EPSILON_BLOCKS
        ),
        1e-5
    ));
    let open = collision_safe_rig_pose(eye, Quat::IDENTITY, rig, &Walls(Vec::new()));
    assert!(open.translation.abs_diff_eq(Vec3::new(0.0, 2.0, 3.0), 1e-5));
}

#[test]
fn camera_corner_rays_leave_thin_geometry_between_the_rays_and_allocate_nothing() {
    let eye = Vec3::ZERO;
    let rig = CameraRig {
        offset: Vec3::new(0.0, 0.0, 4.0),
        ..shoulder()
    };
    let thin = Walls(vec![Aabb::new(
        SimVec3::new(-0.02, -0.02, 1.0),
        SimVec3::new(0.02, 0.02, 2.0),
    )]);
    assert_eq!(
        collision_safe_rig_pose(eye, Quat::IDENTITY, rig, &thin)
            .translation
            .z,
        4.0
    );
    let before = crate::test_allocations::count();
    for _ in 0..1000 {
        let _ = collision_safe_rig_pose(eye, Quat::IDENTITY, rig, &thin);
    }
    assert_eq!(crate::test_allocations::count() - before, 0);
}

#[test]
fn camera_collision_keeps_the_minimum_avoidance_distance() {
    let wall = Walls(vec![Aabb::new(
        SimVec3::new(-1.0, -1.0, 0.1),
        SimVec3::new(1.0, 1.0, 1.0),
    )]);
    let rig = CameraRig {
        offset: Vec3::new(0.0, 0.0, 4.0),
        ..shoulder()
    };
    let result = collision_safe_rig_pose(Vec3::ZERO, Quat::IDENTITY, rig, &wall);
    assert!((result.translation.z - 0.25).abs() < 1e-6);
}

/// Air-filled sub-chunk 0 for the 3x3 columns around the origin column, with `blocks` placed.
fn loaded_palette_store(blocks: &[([u8; 3], u32)]) -> ChunkStore {
    let mut store = ChunkStore::new();
    let air = [9, 1, 0, 1, 0];
    for x in -1..=1 {
        for z in -1..=1 {
            store
                .apply_level_chunk(ChunkKey::new(0, x, z), 0, 1, &air, &RawBlockIds { air: 0 })
                .unwrap();
        }
    }
    let sub = SubChunkKey::from_chunk(ChunkKey::new(0, 0, 0), 0);
    for &([x, y, z], id) in blocks {
        store
            .update_block(sub, BlockUpdate::new(x, y, z, 0, id), 0)
            .unwrap();
    }
    store
}

fn camera_registry() -> CollisionRegistry {
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    registry
        .register(1, [Aabb::new(SimVec3::ZERO, SimVec3::ONE)])
        .unwrap();
    registry
        .register(2, [Aabb::new(SimVec3::ZERO, SimVec3::new(1.0, 0.5, 1.0))])
        .unwrap();
    registry
        .register(
            3,
            [Aabb::new(
                SimVec3::new(0.375, 0.0, 0.375),
                SimVec3::new(0.625, 1.5, 0.625),
            )],
        )
        .unwrap();
    registry
}

/// The previous boom: every collider in the corner box's swept volume against all eight rays.
fn volume_scan_boom(subject: Vec3, mut pose: Transform, world: &impl CollisionWorld) -> Transform {
    let delta = pose.translation - subject;
    let origin = SimVec3::new(subject.x.into(), subject.y.into(), subject.z.into());
    let sweep = SimVec3::new(delta.x.into(), delta.y.into(), delta.z.into());
    let radius = f64::from(THIRD_PERSON_COLLISION_RADIUS_BLOCKS);
    let near_clip = f64::from(THIRD_PERSON_COLLISION_EPSILON_BLOCKS);
    let camera = Aabb::new(
        origin - SimVec3::new(radius, radius, radius),
        origin + SimVec3::new(radius, radius, radius),
    );
    let distance = f64::from(delta.length());
    let mut safe = distance;
    let _ = world.visit_collision_boxes_camera_lenient(camera.swept(sweep), &mut |collision| {
        for corner in 0..8 {
            let offset = SimVec3::new(
                if corner & 1 == 0 { -radius } else { radius },
                if corner & 2 == 0 { -radius } else { radius },
                if corner & 4 == 0 { -radius } else { radius },
            );
            if let Some(entry) = collision.segment_entry(origin + offset, sweep) {
                let hit = offset + sweep * entry;
                safe = safe.min((hit.length_squared().sqrt() - near_clip).max(near_clip));
            }
        }
    });
    if safe < distance {
        pose.translation = subject + delta.normalize_or_zero() * safe.max(0.25) as f32;
    }
    pose
}

#[test]
fn per_ray_boom_matches_the_volume_scan_around_walls_slabs_and_fences() {
    let mut blocks = Vec::new();
    for y in 3..9 {
        blocks.push(([5, y, 6], 1));
        blocks.push(([11, y, 9], 1));
    }
    for (x, z) in [(7, 11), (8, 11), (9, 4), (6, 9), (10, 6)] {
        blocks.push(([x, 4, z], 3));
        blocks.push(([x, 8, z], 2));
    }
    blocks.push(([8, 9, 8], 1));
    blocks.push(([9, 2, 7], 1));
    let store = loaded_palette_store(&blocks);
    let registry = camera_registry();
    let world = PaletteWorld::new(&store, &registry, 0);
    let eye = Vec3::new(8.3, 5.62, 8.4);
    let mut clipped = 0;
    for yaw_step in 0..32 {
        for pitch_step in -6..=6 {
            let rotation = Quat::from_euler(
                EulerRot::YXZ,
                yaw_step as f32 * std::f32::consts::TAU / 32.0,
                pitch_step as f32 * 0.25,
                0.0,
            );
            for perspective in [
                PerspectiveMode::ThirdPersonBack,
                PerspectiveMode::ThirdPersonFront,
            ] {
                let expected =
                    volume_scan_boom(eye, perspective_pose(eye, rotation, perspective), &world);
                let actual = collision_safe_perspective_pose(eye, rotation, perspective, &world);
                assert!(
                    actual.translation.abs_diff_eq(expected.translation, 1e-5),
                    "yaw {yaw_step} pitch {pitch_step} {perspective:?}: {actual:?} vs {expected:?}"
                );
                clipped += usize::from(
                    expected.translation.distance(eye) < super::THIRD_PERSON_RADIUS_BLOCKS - 1e-3,
                );
            }
        }
    }
    assert!(
        clipped > 100,
        "fixture must exercise collisions, clipped {clipped}"
    );
}

/// Delegates to a live palette and tallies every cell the boom inspects.
struct CountingPalette<'a> {
    world: PaletteWorld<'a>,
    inspected: Cell<u64>,
}

impl CountingPalette<'_> {
    fn tally(&self, skipped: LenientSkipCounts) {
        self.inspected.set(
            self.inspected.get()
                + u64::from(skipped.unloaded_chunk)
                + u64::from(skipped.unknown_runtime_id),
        );
    }
}

impl CollisionWorld for CountingPalette<'_> {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        self.world.collision_boxes(query)
    }

    fn visit_collision_boxes_camera_lenient(
        &self,
        query: Aabb,
        visitor: &mut dyn FnMut(Aabb),
    ) -> Result<LenientSkipCounts, WorldQueryError> {
        let skipped = self
            .world
            .visit_collision_boxes_camera_lenient(query, visitor)?;
        self.tally(skipped);
        Ok(skipped)
    }

    fn camera_segment_entry(
        &self,
        origin: SimVec3,
        delta: SimVec3,
    ) -> Result<(Option<f64>, LenientSkipCounts), WorldQueryError> {
        let result = self.world.camera_segment_entry(origin, delta)?;
        self.tally(result.1);
        Ok(result)
    }
}

#[test]
fn easing_boom_across_a_hundred_unloaded_blocks_inspects_only_cells_along_its_rays() {
    let store = ChunkStore::new();
    let registry = camera_registry();
    let world = CountingPalette {
        world: PaletteWorld::new(&store, &registry, 0),
        inspected: Cell::new(0),
    };
    let rig = CameraRig {
        offset: Vec3::new(60.0, 40.0, 70.0),
        ..shoulder()
    };
    let eye = Vec3::new(0.3, 70.62, 0.45);
    let pose = collision_safe_rig_pose(eye, Quat::IDENTITY, rig, &world);
    assert!(pose.translation.abs_diff_eq(eye + rig.offset, 1e-4));
    // Eight rays, each crossing at most 170 boundaries, inspecting a two-cell halo per cell.
    let bound = 8 * 2 * (60 + 40 + 70 + 1 + 2);
    assert!(
        world.inspected.get() <= bound,
        "inspected {} cells for one frame",
        world.inspected.get()
    );
}
