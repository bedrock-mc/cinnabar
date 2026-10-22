use std::cell::Cell;

use super::*;
use crate::{BlockPhysicsFacts, CollisionRegistry, PaletteWorld, SurfaceResponse, Vec3};
use world::{BlockUpdate, ChunkCollisionRevision, ChunkKey, ChunkStore, SubChunkKey};

struct CountingWorld<W> {
    inner: W,
    carry_facts: bool,
    physics_queries: Cell<usize>,
}

impl<W: CollisionWorld> CollisionWorld for CountingWorld<W> {
    /// Forwards the ordinary collision query without material sampling.
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        self.inner.collision_boxes(query)
    }

    /// Optionally hides carried facts to compare the lazy fallback's results.
    fn collision_boxes_with_provenance(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
        let mut result = self.inner.collision_boxes_with_provenance(query)?;
        if !self.carry_facts {
            for collider in &mut result.value {
                collider.cell_has_scaffolding = None;
            }
        }
        Ok(result)
    }

    /// Counts material samples requested after collision lookup.
    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        self.physics_queries.set(self.physics_queries.get() + 1);
        self.inner.block_physics(block)
    }
}

/// Registers a material with the given shapes and scaffolding classification.
fn register(
    registry: &mut CollisionRegistry,
    runtime_id: u32,
    shapes: impl IntoIterator<Item = Aabb>,
    scaffold: bool,
) {
    registry
        .register_physics(
            runtime_id,
            shapes,
            crate::DEFAULT_SURFACE_FRICTION,
            1.0,
            1.0,
            0.0,
            if scaffold {
                BlockPhysicsFlags::SCAFFOLDING
            } else {
                BlockPhysicsFlags::default()
            },
            SurfaceResponse::None,
        )
        .unwrap();
}

#[test]
fn compound_cells_reuse_any_layer_facts_without_resampling() {
    let shapes = [
        Aabb::new(Vec3::ZERO, Vec3::new(1.0, 0.5, 1.0)),
        Aabb::new(Vec3::new(0.25, 0.5, 0.25), Vec3::new(0.75, 1.0, 0.75)),
    ];
    for scaffold_layer in [None, Some(0), Some(1)] {
        // A non-intersecting or shapeless secondary layer still classifies the cell.
        for secondary_shapes in [
            vec![],
            vec![Aabb::new(Vec3::ZERO, Vec3::ONE)],
            vec![Aabb::new(
                Vec3::new(0.0, 0.0, 1.25),
                Vec3::new(1.0, 1.0, 1.5),
            )],
        ] {
            let mut registry = CollisionRegistry::new();
            registry.register(0, []).unwrap();
            register(&mut registry, 1, shapes, scaffold_layer == Some(0));
            register(
                &mut registry,
                2,
                secondary_shapes,
                scaffold_layer == Some(1),
            );
            register(&mut registry, 3, shapes, false);
            let mut store = ChunkStore::new();
            store.mark_chunk_loaded(ChunkKey::new(0, 0, 0)).unwrap();
            for (layer, runtime_id) in [(0, 1), (1, 2)] {
                store
                    .update_block(
                        SubChunkKey::new(0, 0, 0, 0),
                        BlockUpdate::new(4, 4, 4, layer, runtime_id),
                        0,
                    )
                    .unwrap();
            }
            store
                .update_block(
                    SubChunkKey::new(0, 0, 0, 0),
                    BlockUpdate::new(6, 4, 4, 0, 3),
                    0,
                )
                .unwrap();
            let palette = PaletteWorld::for_movement(&store, &registry, 0);
            let query = Aabb::new(Vec3::new(4.1, 4.1, 4.1), Vec3::new(6.9, 4.9, 4.9));
            let baseline = palette.collision_boxes_with_provenance(query).unwrap();
            assert!(baseline.value.len() >= 2 * shapes.len());
            assert!(baseline.value.iter().all(|collider| match collider.block {
                Some([4, 4, 4]) => collider.cell_has_scaffolding == Some(scaffold_layer.is_some()),
                Some([6, 4, 4]) => collider.cell_has_scaffolding == Some(false),
                _ => false,
            }));
            assert_eq!(
                palette
                    .snapshot()
                    .unwrap()
                    .collision_boxes_with_provenance(query)
                    .unwrap(),
                baseline
            );
            let player = Aabb::player_at(Vec3::new(4.5, 4.0, 4.5));
            let mut expected = baseline.clone();
            if scaffold_layer.is_some() {
                expected
                    .value
                    .retain(|collider| collider.block == Some([6, 4, 4]));
            }
            for carry_facts in [false, true] {
                let world = CountingWorld {
                    inner: PaletteWorld::for_movement(&store, &registry, 0),
                    carry_facts,
                    physics_queries: Cell::new(0),
                };
                let actual = ScaffoldingView::new(&world, player, true)
                    .collision_boxes_with_provenance(query)
                    .unwrap();
                let mut expected = expected.clone();
                if !carry_facts {
                    for collider in &mut expected.value {
                        collider.cell_has_scaffolding = None;
                    }
                }
                assert_eq!(actual, expected);
                assert_eq!(
                    world.physics_queries.get(),
                    if carry_facts { 0 } else { baseline.value.len() }
                );
            }
        }
    }
}

struct QueryWorld {
    colliders: CollisionQuery<Vec<ProvenancedCollider>>,
    sample: Result<BlockPhysicsSample, WorldQueryError>,
    physics_queries: Cell<usize>,
}

impl CollisionWorld for QueryWorld {
    /// Projects the fixture's colliders without changing their identity.
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery {
            value: self
                .colliders
                .value
                .iter()
                .map(|collider| collider.aabb)
                .collect(),
            identity: self.colliders.identity.clone(),
        })
    }

    /// Returns the fixture's optional cell facts and complete collision identity.
    fn collision_boxes_with_provenance(
        &self,
        _query: Aabb,
    ) -> Result<CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
        Ok(self.colliders.clone())
    }

    /// Counts lazy classification queries and returns their configured outcome.
    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        self.physics_queries.set(self.physics_queries.get() + 1);
        self.sample.clone()
    }
}

/// Builds a collision identity with explicit revisions for the supplied columns.
fn identity(revisions: &[(i32, u64)]) -> crate::WorldCollisionIdentity {
    crate::WorldCollisionIdentity::new(
        CollisionQuery::synthetic(()).identity.registry,
        revisions
            .iter()
            .map(|&(x, revision)| ChunkCollisionRevision {
                chunk: ChunkKey::new(0, x, 0),
                revision,
            }),
    )
    .unwrap()
}

/// Supplies a cell collider and a failing material query to detect eager reads.
fn query_world(cell_has_scaffolding: Option<bool>) -> QueryWorld {
    QueryWorld {
        colliders: CollisionQuery {
            value: vec![ProvenancedCollider {
                aabb: Aabb::new(Vec3::ZERO, Vec3::ONE),
                block: Some([0; 3]),
                runtime_id: Some(1),
                cell_has_scaffolding,
            }],
            identity: identity(&[(0, 3), (1, 4)]),
        },
        sample: Err(WorldQueryError::UnknownRuntimeId {
            runtime_id: 7,
            block: [0; 3],
        }),
        physics_queries: Cell::new(0),
    }
}

#[test]
fn carried_facts_keep_top_and_descending_rules_and_the_full_identity() {
    for fact in [Some(false), Some(true), None] {
        for descending in [false, true] {
            for feet_y in [0.0, 1.0] {
                let world = query_world(fact);
                let player = Aabb::player_at(Vec3::new(0.5, feet_y, 0.5));
                let result = ScaffoldingView::new(&world, player, descending)
                    .collision_boxes_with_provenance(world.colliders.value[0].aabb);
                let solid_top = !descending && feet_y == 1.0;
                if fact.is_none() && !solid_top {
                    assert_eq!(result, Err(world.sample.clone().unwrap_err()));
                    assert_eq!(world.physics_queries.get(), 1);
                } else {
                    let mut expected = world.colliders.clone();
                    if fact == Some(true) && !solid_top {
                        expected.value.clear();
                    }
                    assert_eq!(result.unwrap(), expected);
                    assert_eq!(world.physics_queries.get(), 0);
                }
            }
        }
    }
}

#[test]
fn fallback_classification_preserves_revision_conflicts_and_merges_new_columns() {
    let mut world = query_world(None);
    let player = Aabb::player_at(Vec3::new(0.5, 0.0, 0.5));
    let query = world.colliders.value[0].aabb;
    for (sample_identity, expected) in [
        (
            identity(&[(0, 9)]),
            Err(WorldQueryError::ChunkRevisionConflict {
                chunk: ChunkKey::new(0, 0, 0),
            }),
        ),
        (
            identity(&[(0, 3), (2, 5)]),
            Ok(identity(&[(0, 3), (1, 4), (2, 5)])),
        ),
    ] {
        world.sample = Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: crate::DEFAULT_SURFACE_FRICTION,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 0.0,
                flags: BlockPhysicsFlags::SCAFFOLDING,
                surface_response: SurfaceResponse::None,
            }]),
            identity: sample_identity,
        });
        let actual =
            ScaffoldingView::new(&world, player, true).collision_boxes_with_provenance(query);
        match expected {
            Err(error) => assert_eq!(actual, Err(error)),
            Ok(identity) => assert_eq!(
                actual.unwrap(),
                CollisionQuery {
                    value: vec![],
                    identity
                }
            ),
        }
    }
    assert_eq!(world.physics_queries.get(), 2);
}

#[test]
fn actors_and_unloaded_boundaries_keep_their_shapes_without_material_queries() {
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    let mut store = ChunkStore::new();
    store.mark_chunk_loaded(ChunkKey::new(0, 0, 0)).unwrap();
    let actor = Aabb::new(Vec3::new(15.5, 4.0, 7.5), Vec3::new(15.9, 5.0, 8.5));
    let world = CountingWorld {
        inner: PaletteWorld::for_movement(&store, &registry, 0).with_actor_colliders(vec![actor]),
        carry_facts: true,
        physics_queries: Cell::new(0),
    };
    let query = Aabb::new(Vec3::new(15.5, 4.0, 7.5), Vec3::new(16.5, 5.0, 8.5));
    let expected = world.collision_boxes_with_provenance(query).unwrap();
    assert_eq!(expected.value.len(), 2);
    assert!(
        expected.value.iter().all(|collider| {
            collider.block.is_none() && collider.cell_has_scaffolding.is_none()
        })
    );
    assert!(expected.value.iter().any(|collider| collider.aabb == actor));
    let actual = ScaffoldingView::new(&world, query, true)
        .collision_boxes_with_provenance(query)
        .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(world.physics_queries.get(), 0);
}

#[test]
fn collision_lookup_still_rejects_unknown_secondary_layers_before_filtering() {
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    register(&mut registry, 1, [Aabb::new(Vec3::ZERO, Vec3::ONE)], true);
    let mut store = ChunkStore::new();
    store.mark_chunk_loaded(ChunkKey::new(0, 0, 0)).unwrap();
    for (layer, runtime_id) in [(0, 1), (1, 99)] {
        store
            .update_block(
                SubChunkKey::new(0, 0, 0, 0),
                BlockUpdate::new(4, 4, 4, layer, runtime_id),
                0,
            )
            .unwrap();
    }
    let world = CountingWorld {
        inner: PaletteWorld::for_movement(&store, &registry, 0),
        carry_facts: true,
        physics_queries: Cell::new(0),
    };
    let query = Aabb::new(Vec3::new(4.1, 4.1, 4.1), Vec3::new(4.9, 4.9, 4.9));
    assert_eq!(
        ScaffoldingView::new(&world, query, true).collision_boxes(query),
        Err(WorldQueryError::UnknownRuntimeId {
            runtime_id: 99,
            block: [4, 4, 4]
        })
    );
    assert_eq!(world.physics_queries.get(), 0);
}

/// A stone floor with a three-high scaffolding column at `x = 0, z = 0` on it.
fn scaffold_column() -> (ChunkStore, CollisionRegistry) {
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    register(&mut registry, 1, [Aabb::new(Vec3::ZERO, Vec3::ONE)], false);
    register(&mut registry, 2, [Aabb::new(Vec3::ZERO, Vec3::ONE)], true);
    let mut store = ChunkStore::new();
    store.mark_chunk_loaded(ChunkKey::new(0, 0, 0)).unwrap();
    for (y, runtime_id) in [(0, 1), (1, 2), (2, 2), (3, 2)] {
        store
            .update_block(
                SubChunkKey::new(0, 0, 0, 0),
                BlockUpdate::new(0, y, 0, 0, runtime_id),
                0,
            )
            .unwrap();
    }
    (store, registry)
}

/// The mover's contextual shapes around `feet`, for a player with no retained contact.
fn mover_shapes(world: &PaletteWorld<'_>, feet: Vec3, input: &MovementInput) -> Vec<Aabb> {
    let player = Aabb::player_at(feet);
    mover_collision_boxes_with_provenance(
        world,
        &PlayerState::new(feet),
        input,
        player,
        player.grown(1.0),
    )
    .unwrap()
    .value
    .into_iter()
    .map(|collider| collider.aabb)
    .collect()
}

#[test]
fn mover_shapes_follow_the_move_scaffolding_rules() {
    let (store, registry) = scaffold_column();
    let world = PaletteWorld::for_movement(&store, &registry, 0);
    let idle = MovementInput::default();
    let top = Aabb::new(Vec3::new(0.0, 3.0, 0.0), Vec3::new(1.0, 4.0, 1.0));

    // Inside the column the body overlaps nothing it would collide with,
    // although the ordinary query reports the column's unit cubes there.
    let inside = Vec3::new(0.5, 1.5, 0.5);
    let player = Aabb::player_at(inside);
    assert!(
        mover_shapes(&world, inside, &idle)
            .iter()
            .all(|shape| !shape.intersects(player))
    );
    assert!(
        world
            .collision_boxes(player)
            .unwrap()
            .value
            .iter()
            .any(|shape| shape.intersects(player))
    );

    // Standing on the column, its top scaffold is solid; a descent sinks through it.
    let on_top = Vec3::new(0.5, 4.0, 0.5);
    assert!(mover_shapes(&world, on_top, &idle).contains(&top));
    let descending = MovementInput {
        sneaking: true,
        ..MovementInput::default()
    };
    assert!(!mover_shapes(&world, on_top, &descending).contains(&top));
}
