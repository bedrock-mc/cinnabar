use super::{
    CompiledBlockShapes, CompiledDoor, CompiledFlow, OutlineContext, PickRule, Reader,
    SelectionContext, ShapeBox, ShapeFlags, read, write,
};

/// Supplies a complete record with nontrivial geometry and optional facts.
fn fixture() -> CompiledBlockShapes {
    let shape = ShapeBox::new([0.125, -0.0625, 0.25], [0.875, 1.125, 0.75]);
    CompiledBlockShapes {
        state_identity: [23; 32],
        actor_facing: Some(5),
        actor_visual: Some(shape),
        selection: Box::new([shape]),
        selection_context: SelectionContext::Static,
        contextual_selection: Box::new([]),
        outline: Some(shape),
        connected_outline: None,
        outline_context: OutlineContext::Static,
        pick_rule: PickRule::HeldLight,
        flags: ShapeFlags::LIGHT_BLOCK,
        door: Some(CompiledDoor {
            family: [42; 32],
            facing: 2,
            upper: true,
            open: false,
            hinge_right: true,
        }),
        flow: Some(CompiledFlow {
            blocks_motion: true,
            is_solid: false,
            blocked_faces: 3,
            allowed_faces: 12,
            liquid_depth: Some(7),
        }),
        tags: Box::new(["test:first".into(), "test:second".into()]),
    }
}

#[test]
fn compiled_facts_round_trip_and_reject_every_truncated_prefix() {
    let original = fixture();
    let mut bytes = Vec::new();
    write(&mut bytes, &original).unwrap();
    let mut reader = Reader::new(&bytes);
    assert_eq!(read(&mut reader).unwrap(), original);
    assert_eq!(reader.remaining(), 0);
    for end in 0..bytes.len() {
        assert!(
            read(&mut Reader::new(&bytes[..end])).is_err(),
            "prefix {end}"
        );
    }
}

#[test]
fn incomplete_context_and_invalid_geometry_cannot_be_encoded() {
    for case in 0..8 {
        let mut shapes = fixture();
        match case {
            0 => shapes.selection[0].max[0] = f64::NAN,
            1 => shapes.actor_visual.as_mut().unwrap().max[1] = f64::INFINITY,
            2 => shapes.selection = Box::new([]),
            3 => shapes.selection_context = SelectionContext::PortalAxis,
            4 => shapes.actor_facing = Some(6),
            5 => shapes.tags[1] = shapes.tags[0].clone(),
            6 => shapes.door.as_mut().unwrap().facing = 4,
            _ => shapes.flow.as_mut().unwrap().liquid_depth = Some(16),
        }
        assert!(write(&mut Vec::new(), &shapes).is_err(), "case {case}");
    }
}

#[test]
fn actor_only_geometry_has_an_explicit_context() {
    let mut shapes = fixture();
    shapes.selection = Box::new([]);
    assert!(write(&mut Vec::new(), &shapes).is_err());
    shapes.selection_context = SelectionContext::MovingBlock;
    let mut bytes = Vec::new();
    write(&mut bytes, &shapes).unwrap();
    assert_eq!(read(&mut Reader::new(&bytes)).unwrap(), shapes);
}
