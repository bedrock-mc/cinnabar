use super::{UNIT, compile, fixture};
use assets::{ModelFamily, block_shapes::ShapeBox};

#[test]
fn fence_gate_selection_uses_the_directional_outline_for_every_state() {
    let (records, seed) = fixture();
    let mut checked = 0;
    for record in records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Gate)
    {
        let direction = super::super::state::State::read(record)
            .unwrap()
            .direction()
            .unwrap();
        let expected = if direction % 2 == 0 {
            ShapeBox::new([0.0, 0.0, 0.375], [1.0, 1.0, 0.625])
        } else {
            ShapeBox::new([0.375, 0.0, 0.0], [0.625, 1.0, 1.0])
        };
        let input = seed.by_sequential_id(record.sequential_id).unwrap();
        let output = compile(record, input);
        let shapes = output.shapes.unwrap();
        assert_eq!(
            shapes.selection.as_ref(),
            [expected],
            "{} {}",
            record.name,
            record.canonical_state
        );
        assert_eq!(shapes.outline, Some(expected));
        assert_eq!(shapes.actor_visual, Some(UNIT));
        assert_eq!(output.boxes, input.boxes);
        checked += 1;
    }
    assert!(checked > 0, "missing fence gate states");
}
