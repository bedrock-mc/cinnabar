use super::{Nbt, random_offset_component};
use world::random_offset::{RandomOffsetAxis, RandomOffsetComponent};

/// Encodes an authored axis range without involving a visual geometry fixture.
fn axis(min: f64, max: f64, steps: i64) -> Nbt {
    Nbt::Compound(vec![
        (
            "range".into(),
            Nbt::Compound(vec![
                ("min".into(), Nbt::Float(min)),
                ("max".into(), Nbt::Float(max)),
            ]),
        ),
        ("steps".into(), Nbt::Int(steps)),
    ])
}

#[test]
fn random_offset_defaults_and_pixel_ranges_are_admitted_for_all_axes() {
    assert_eq!(
        random_offset_component(&Nbt::Compound(vec![])),
        Some(RandomOffsetComponent::default())
    );
    let component = Nbt::Compound(vec![
        ("x".into(), axis(-4.0, 4.0, 16)),
        ("y".into(), axis(2.0, 2.0, 0)),
        ("z".into(), axis(-8.0, 8.0, 1)),
    ]);
    let expected = RandomOffsetComponent {
        axes: [
            RandomOffsetAxis::from_pixels([-4.0, 4.0], 16),
            RandomOffsetAxis::from_pixels([2.0, 2.0], 0),
            RandomOffsetAxis::from_pixels([-8.0, 8.0], 1),
        ],
    };
    assert_eq!(random_offset_component(&component), Some(expected));
    assert_eq!(expected.offset([-1, 900, -1])[1..], [0.125, 0.0]);
}

#[test]
fn random_offset_malformed_axes_are_skipped() {
    for invalid in [
        axis(4.0, -4.0, 0),
        axis(f64::NAN, 0.0, 0),
        axis(0.0, 1.0, -1),
        Nbt::Byte(1),
    ] {
        assert!(random_offset_component(&Nbt::Compound(vec![("x".into(), invalid)])).is_none());
    }
}
