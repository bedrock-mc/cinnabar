use super::{
    CollisionRegistry, CompiledBlockShapes, OutlineContext, PaletteWorld, SelectionContext,
    ShapeBox, ShapeFlags, box_to_aabb,
};
use crate::{CollisionIdSpace, CollisionRegistryIdentity, Vec3};
use assets::block_shapes::{PISTON_DIRECTIONS, PickRule};
use world::{
    BlockEntityKey, BlockEntityNbt, BlockUpdate, ChunkStore, NbtCompound, NbtValue, SubChunkKey,
};

mod contextual;
mod rays;
mod seams;

const UNIT: ShapeBox = ShapeBox::new([0.0; 3], [1.0; 3]);

/// Supplies a complete compiled record for a focused runtime-context test.
fn shapes(
    context: SelectionContext,
    selection: Vec<ShapeBox>,
    alternate: Vec<ShapeBox>,
) -> CompiledBlockShapes {
    CompiledBlockShapes {
        state_identity: [7; 32],
        actor_facing: None,
        actor_visual: Some(UNIT),
        selection: selection.into_boxed_slice(),
        selection_context: context,
        contextual_selection: alternate.into_boxed_slice(),
        outline: None,
        connected_outline: None,
        outline_context: OutlineContext::Static,
        pick_rule: PickRule::Always,
        flags: ShapeFlags::empty(),
        door: None,
        flow: None,
        tags: Box::new([]),
    }
}

/// Creates a loaded air neighbourhood and registers a target in either ID space.
fn fixture(
    space: CollisionIdSpace,
    target: &CompiledBlockShapes,
) -> (ChunkStore, CollisionRegistry, u32) {
    let id = if space == CollisionIdSpace::Sequential {
        7
    } else {
        0x8f32_9017
    };
    let mut store = ChunkStore::new();
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                let key = SubChunkKey::new(0, x, y, z);
                store.apply_request_mode_air(key).unwrap();
                store.mark_sub_chunk_loaded(key).unwrap();
            }
        }
    }
    set_block(&mut store, [0; 3], id);
    let mut registry = CollisionRegistry::with_identity(CollisionRegistryIdentity {
        protocol: 2193,
        id_space: space,
        preg_sha256: [5; 32],
    });
    registry.register(0, []).unwrap();
    registry.register(id, [box_to_aabb(UNIT)]).unwrap();
    assert!(registry.set_compiled_selection(id, target));
    (store, registry, id)
}

/// Writes a primary block without assuming its ID encoding.
fn set_block(store: &mut ChunkStore, position: [i32; 3], id: u32) {
    store
        .update_block(
            SubChunkKey::new(
                0,
                position[0].div_euclid(16),
                position[1].div_euclid(16),
                position[2].div_euclid(16),
            ),
            BlockUpdate::new(
                position[0].rem_euclid(16) as u8,
                position[1].rem_euclid(16) as u8,
                position[2].rem_euclid(16) as u8,
                0,
                id,
            ),
            0,
        )
        .unwrap();
}

/// Ingests a finite piston snapshot through the network NBT decoder.
fn set_piston(store: &mut ChunkStore, position: [i32; 3], progress: f32) {
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("PistonArm".into()));
    root.insert("Progress", NbtValue::Float(progress));
    let actor = BlockEntityNbt::decode_prefix(&root.encode_root().unwrap())
        .unwrap()
        .0;
    store
        .commit_block_entity_update(
            BlockEntityKey::new(0, position[0], position[1], position[2]),
            actor,
        )
        .unwrap();
}

#[test]
fn unattached_vines_resolve_compiled_support_choices_in_both_id_spaces() {
    let top = ShapeBox::new([0.0, 0.9375, 0.0], [1.0; 3]);
    let floor = ShapeBox::new([0.0; 3], [1.0, 0.0625, 1.0]);
    let target = shapes(
        SelectionContext::VineSupport,
        vec![],
        vec![top, floor, UNIT],
    );
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        let (mut store, mut registry, id) = fixture(space, &target);
        let mut support = shapes(SelectionContext::Static, vec![UNIT], vec![]);
        support.state_identity = [9; 32];
        support.flags = ShapeFlags::VINE_SUPPORT;
        registry.register(9, [box_to_aabb(UNIT)]).unwrap();
        registry.set_compiled_selection(9, &support);
        for (above, below, expected) in [
            (false, false, None),
            (true, false, Some(top)),
            (false, true, Some(floor)),
            (true, true, Some(UNIT)),
        ] {
            set_block(&mut store, [0, 1, 0], if above { 9 } else { 0 });
            set_block(&mut store, [0, -1, 0], if below { 9 } else { 0 });
            let view = PaletteWorld::new(&store, &registry, 0, Default::default());
            let boxes = view
                .selection_shapes_at([0; 3], registry.physics(id).unwrap())
                .unwrap();
            assert_eq!(boxes.as_ref(), expected.map(box_to_aabb).as_slice());
        }
    }
}

#[test]
fn piston_arm_pieces_require_the_owner_actor_and_follow_all_six_directions() {
    let piece = ShapeBox::new([0.25; 3], [0.75; 3]);
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        for (facing, direction) in PISTON_DIRECTIONS.into_iter().enumerate() {
            let mut target = shapes(SelectionContext::PistonArm, vec![piece; 3], vec![]);
            target.actor_facing = Some(facing as u8);
            let (mut store, registry, id) = fixture(space, &target);
            assert!(
                PaletteWorld::new(&store, &registry, 0, Default::default())
                    .selection_shapes_at([0; 3], registry.physics(id).unwrap())
                    .unwrap()
                    .is_empty()
            );
            let owner = direction.map(|value| -value);
            set_piston(&mut store, owner, 0.5);
            let view = PaletteWorld::new(&store, &registry, 0, Default::default());
            let boxes = view
                .selection_shapes_at([0; 3], registry.physics(id).unwrap())
                .unwrap();
            let offset = Vec3::new(
                f64::from(direction[0]) * -0.5,
                f64::from(direction[1]) * -0.5,
                f64::from(direction[2]) * -0.5,
            );
            assert_eq!(boxes.as_ref(), [box_to_aabb(piece).translated(offset); 3]);
        }
    }
}

/// Ingests a moving-block snapshot with a typed carried-state reference.
fn set_moving(store: &mut ChunkStore, name: &str, piston: [i32; 3], expanding: bool) {
    let mut block = NbtCompound::default();
    block.insert("name", NbtValue::String(name.into()));
    block.insert("states", NbtValue::Compound(NbtCompound::default()));
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String("MovingBlock".into()));
    root.insert("movingBlock", NbtValue::Compound(block));
    root.insert("expanding", NbtValue::Byte(i8::from(expanding)));
    for (key, value) in ["pistonPosX", "pistonPosY", "pistonPosZ"]
        .into_iter()
        .zip(piston)
    {
        root.insert(key, NbtValue::Int(value));
    }
    let actor = BlockEntityNbt::decode_prefix(&root.encode_root().unwrap())
        .unwrap()
        .0;
    store
        .commit_block_entity_update(BlockEntityKey::new(0, 0, 0, 0), actor)
        .unwrap();
}
