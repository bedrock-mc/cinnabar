use super::*;

#[test]
fn basic_parameter_shapes_follow_the_pinned_wire_ids() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../data/protocol-ids.json")).unwrap();
    let values = fixture["groups"]["CommandArgType"].as_object().unwrap();
    for (names, shape) in [
        (&["Int", "WildcardInt"][..], CommandParamKind::Int),
        (&["Float"][..], CommandParamKind::Float),
        (
            &[
                "Target",
                "StandaloneTarget",
                "WildcardTarget",
                "NonIDTarget",
            ][..],
            CommandParamKind::Target,
        ),
        (
            &["BlockPosition", "Position"][..],
            CommandParamKind::Position,
        ),
        (
            &["Message", "RawText", "TextContinuation", "JSON"][..],
            CommandParamKind::Rest,
        ),
    ] {
        for name in names {
            let raw = values[*name].as_u64().unwrap() as u32;
            assert_eq!(param_kind(raw, &[], &[]), shape, "{name}");
            assert_eq!(
                param_kind(raw | CommandArgFlag::Valid.wire_value(), &[], &[]),
                shape,
                "{name}"
            );
        }
    }
    assert_eq!(
        param_kind(u16::MAX.into(), &[], &[]),
        CommandParamKind::Word
    );
}

#[test]
fn permission_names_resolve_through_generated_levels() {
    for (name, expected) in [
        ("any", CommandPermissionLevel::Any),
        ("gamedirectors", CommandPermissionLevel::Gamedirectors),
        ("admin", CommandPermissionLevel::Admin),
        ("host", CommandPermissionLevel::Host),
        ("owner", CommandPermissionLevel::Owner),
        ("internal", CommandPermissionLevel::Internal),
    ] {
        assert_eq!(permission_level(name), expected.to_raw());
    }
    assert_eq!(
        permission_level("unknown"),
        CommandPermissionLevel::Any.to_raw()
    );
}
