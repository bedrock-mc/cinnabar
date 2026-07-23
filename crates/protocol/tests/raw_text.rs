use std::sync::Arc;

use protocol::{
    BedrockSession, MAX_RAW_TEXT_COMPONENTS, MAX_RAW_TEXT_DEPTH, MAX_RAW_TEXT_INPUT_BYTES,
    MAX_RAW_TEXT_NODES, MAX_RAW_TEXT_OUTPUT_BYTES, RawTextComponent, RawTextResolution, TextKind,
    TitleAction, UiEvent, UiPacketError, WorldEvent, decode_batch, into_world_event,
    parse_raw_text,
};
use valentine::bedrock::version::v1_26_30::{
    SetTitlePacket, SetTitlePacketType, TextPacket, TextPacketCategory, TextPacketContent,
    TextPacketContentJson, TextPacketType,
};

const OBJECT_FIXTURE: &[u8] = include_bytes!("../fixtures/text_object_rawtext.bin");
const WHISPER_FIXTURE: &[u8] = include_bytes!("../fixtures/text_object_whisper_rawtext.bin");
const ANNOUNCEMENT_FIXTURE: &[u8] =
    include_bytes!("../fixtures/text_object_announcement_rawtext.bin");

fn normalize_json(
    kind: TextPacketType,
    message: String,
) -> Result<protocol::RawTextEvent, UiPacketError> {
    let content = TextPacketContentJson { message };
    let content = match kind {
        TextPacketType::Json => TextPacketContent::Json(content),
        TextPacketType::JsonWhisper => TextPacketContent::JsonWhisper(content),
        TextPacketType::JsonAnnouncement => TextPacketContent::JsonAnnouncement(content),
        _ => panic!("test helper accepts only object text packet kinds"),
    };
    let packet = TextPacket {
        category: TextPacketCategory::MessageOnly,
        type_: kind,
        content: Some(content),
        ..Default::default()
    };
    match into_world_event(packet.into(), 0) {
        Ok(Some(WorldEvent::Ui(UiEvent::RawText(event)))) => Ok(event),
        Ok(other) => panic!("expected normalized text event, got {other:?}"),
        Err(protocol::WorldPacketError::Ui(error)) => Err(error),
        Err(other) => panic!("unexpected world error: {other}"),
    }
}

fn normalize_raw(message: String) -> Result<UiEvent, UiPacketError> {
    let packet = TextPacket {
        category: TextPacketCategory::MessageOnly,
        type_: TextPacketType::Raw,
        content: Some(TextPacketContent::Raw(TextPacketContentJson { message })),
        ..Default::default()
    };
    match into_world_event(packet.into(), 0) {
        Ok(Some(WorldEvent::Ui(event))) => Ok(event),
        Ok(other) => panic!("expected normalized UI event, got {other:?}"),
        Err(protocol::WorldPacketError::Ui(error)) => Err(error),
        Err(other) => panic!("unexpected world error: {other}"),
    }
}

fn decode_fixture(bytes: &'static [u8]) -> protocol::RawTextEvent {
    let mut packets = decode_batch(bytes.into(), &BedrockSession { shield_item_id: 0 }).unwrap();
    assert_eq!(packets.len(), 1);
    match into_world_event(packets.pop().unwrap(), 0).unwrap() {
        Some(WorldEvent::Ui(UiEvent::RawText(event))) => event,
        other => panic!("expected text fixture, got {other:?}"),
    }
}

fn normalize_title_object(
    action: SetTitlePacketType,
    message: &str,
) -> Result<protocol::TitleEvent, UiPacketError> {
    let packet = SetTitlePacket {
        type_: action,
        text: message.to_owned(),
        ..Default::default()
    };
    match into_world_event(packet.into(), 0) {
        Ok(Some(WorldEvent::Ui(UiEvent::Title(event)))) => Ok(event),
        Ok(other) => panic!("expected normalized title event, got {other:?}"),
        Err(protocol::WorldPacketError::Ui(error)) => Err(error),
        Err(other) => panic!("unexpected world error: {other}"),
    }
}

#[test]
fn protocol_1001_object_text_fixtures_emit_human_text_without_json_leakage() {
    let object = decode_fixture(OBJECT_FIXTURE);
    assert_eq!(object.text.kind, TextKind::Json);
    assert_eq!(object.text.message.as_ref(), "\u{a7}aLBSG human chat");
    assert_eq!(object.document.resolution(), RawTextResolution::LiteralOnly);
    assert!(!object.document.has_unresolved_components());

    let whisper = decode_fixture(WHISPER_FIXTURE);
    assert_eq!(whisper.text.kind, TextKind::JsonWhisper);
    assert_eq!(whisper.text.message.as_ref(), "private ");
    assert!(!whisper.text.message.contains("rawtext"));
    assert_eq!(
        whisper.document.resolution(),
        RawTextResolution::RequiresResolver
    );
    assert!(whisper.document.has_unresolved_components());

    let announcement = decode_fixture(ANNOUNCEMENT_FIXTURE);
    assert_eq!(announcement.text.kind, TextKind::JsonAnnouncement);
    assert_eq!(announcement.text.message.as_ref(), "Announcement");
    assert_eq!(
        announcement.document.resolution(),
        RawTextResolution::LiteralOnly
    );
}

#[test]
fn legacy_raw_packet_with_exact_rawtext_envelope_emits_human_text() {
    let event =
        normalize_raw(r#"  { "rawtext" : [{"text":"Transferring to SM3"}]}  "#.to_owned()).unwrap();
    let UiEvent::RawText(event) = event else {
        panic!("expected legacy RawText envelope to retain typed semantics")
    };

    assert_eq!(event.text.kind, TextKind::Raw);
    assert_eq!(event.text.message.as_ref(), "Transferring to SM3");
    assert!(!event.text.message.contains("rawtext"));
    assert_eq!(event.document.resolution(), RawTextResolution::LiteralOnly);
}

#[test]
fn ordinary_raw_json_text_is_not_reclassified_as_rawtext() {
    let UiEvent::Text(event) = normalize_raw(r#"{"status":"ok"}"#.to_owned()).unwrap() else {
        panic!("ordinary JSON text must remain an ordinary raw message")
    };

    assert_eq!(event.kind, TextKind::Raw);
    assert_eq!(event.message.as_ref(), r#"{"status":"ok"}"#);
}

#[test]
fn malformed_legacy_rawtext_envelope_fails_closed() {
    assert!(matches!(
        normalize_raw(r#"{"rawtext":[{"text":"unterminated}]}"#.to_owned()),
        Err(UiPacketError::InvalidRawText)
    ));
}

#[test]
fn escaped_legacy_rawtext_key_is_classified_semantically() {
    let UiEvent::RawText(event) =
        normalize_raw(r#"{"raw\u0074ext":[{"text":"Escaped key"}]}"#.to_owned()).unwrap()
    else {
        panic!("escaped rawtext key must retain typed semantics")
    };

    assert_eq!(event.text.message.as_ref(), "Escaped key");
    assert!(!event.text.message.contains("rawtext"));
}

#[test]
fn later_rawtext_member_fails_closed_instead_of_leaking_json() {
    for value in [
        r#"{"metadata":true,"rawtext":[{"text":"later"}]}"#,
        r#"{"metadata":true,"raw\u0074ext":[{"text":"escaped later"}]}"#,
        r#"{"metadata":true,"rawtext":[{"text":"unterminated}]}"#,
        r#"{"metadata":true,"rawtext":[{"text":"ok"}],"extra":false}"#,
        r#"{"rawtext":[{"text":"ok"}],"extra":"unterminated}"#,
    ] {
        assert!(
            matches!(
                normalize_raw(value.to_owned()),
                Err(UiPacketError::InvalidRawText)
            ),
            "rawtext intent leaked as ordinary JSON: {value}"
        );
    }
}

#[test]
fn malformed_earlier_member_cannot_hide_later_rawtext_intent() {
    for value in [
        r#"{"metadata":truX,"rawtext":[{"text":"later"}]}"#,
        r#"{"metadata":truX,"raw\u0074ext":[{"text":"escaped later"}]}"#,
    ] {
        assert!(
            matches!(
                normalize_raw(value.to_owned()),
                Err(UiPacketError::InvalidRawText)
            ),
            "rawtext intent after malformed syntax leaked as ordinary JSON: {value}"
        );
    }
}

#[test]
fn duplicate_rawtext_members_fail_closed() {
    assert!(matches!(
        normalize_raw(
            r#"{"rawtext":[{"text":"first"}],"raw\u0074ext":[{"text":"second"}]}"#.to_owned()
        ),
        Err(UiPacketError::InvalidRawText)
    ));
}

#[test]
fn unrelated_json_with_similar_or_escaped_keys_remains_literal() {
    for value in [
        r#"{"raw\u0074extish":[{"text":"ordinary"}]}"#,
        r#"{"metadata":{"label":"rawtext"}}"#,
        r#"{"status":"unterminated}"#,
        r#"{"metadata":truX,"note":"rawtext"}"#,
        r#"{"metadata":truX,"note":"\"rawtext\":[]"}"#,
        r#"{"metadata":truX,"nested":{"rawtext":[]}}"#,
        r#"{"metadata":truX,"nested":[{"raw\u0074ext":[]}]}"#,
    ] {
        let UiEvent::Text(event) = normalize_raw(value.to_owned()).unwrap() else {
            panic!("unrelated JSON must remain ordinary text: {value}")
        };
        assert_eq!(event.message.as_ref(), value);
    }
}

#[test]
fn raw_text_preserves_nested_translation_and_unresolved_components_without_guessing() {
    let document = parse_raw_text(
        r#"{"rawtext":[{"text":"\u00a76Round "},{"rawtext":[{"text":"one"}]},{"translate":"chat.type.text","with":["Alice",{"rawtext":[{"text":"hello"}]}]},{"score":{"name":"*","objective":"kills"}},{"selector":"@a"}]}"#,
    )
    .unwrap();

    assert_eq!(document.literal_text(), "\u{a7}6Round one");
    assert_eq!(document.resolution(), RawTextResolution::RequiresResolver);
    assert!(document.has_unresolved_components());
    assert_eq!(document.components().len(), 5);
    let RawTextComponent::Translate { key, with } = &document.components()[2] else {
        panic!("expected typed translation component")
    };
    assert_eq!(key.as_ref(), "chat.type.text");
    assert_eq!(with.len(), 2);
    assert!(matches!(&with[0], RawTextComponent::Text(value) if value.as_ref() == "Alice"));
    assert!(
        matches!(&with[1], RawTextComponent::Sequence(values) if matches!(&values[0], RawTextComponent::Text(value) if value.as_ref() == "hello"))
    );
    assert!(
        matches!(&document.components()[3], RawTextComponent::Score { name, objective } if name.as_ref() == "*" && objective.as_ref() == "kills")
    );
    assert!(
        matches!(&document.components()[4], RawTextComponent::Selector(value) if value.as_ref() == "@a")
    );
}

#[test]
fn malformed_ambiguous_and_unknown_raw_text_fail_closed() {
    for value in [
        r#"{"rawtext":[{"text":"unterminated}]}"#,
        r#"{"rawtext":[{"text":"ok","selector":"@a"}]}"#,
        r#"{"rawtext":[{"text":"ok","clickEvent":{"action":"run_command"}}]}"#,
        r#"{"rawtext":[42]}"#,
        r#"{"rawtext":"not-an-array"}"#,
    ] {
        assert!(parse_raw_text(value).is_err(), "accepted {value}");
        assert!(normalize_json(TextPacketType::Json, value.to_owned()).is_err());
    }
}

#[test]
fn raw_text_input_depth_component_and_output_limits_are_explicit() {
    let minimal = r#"{"rawtext":[]}"#;
    let exact_input = format!(
        "{minimal}{}",
        " ".repeat(MAX_RAW_TEXT_INPUT_BYTES - minimal.len())
    );
    assert_eq!(exact_input.len(), MAX_RAW_TEXT_INPUT_BYTES);
    parse_raw_text(&exact_input).unwrap();
    assert!(matches!(
        parse_raw_text(&(exact_input + " ")),
        Err(UiPacketError::RawTextInputTooLarge {
            bytes,
            max: MAX_RAW_TEXT_INPUT_BYTES,
        }) if bytes == MAX_RAW_TEXT_INPUT_BYTES + 1
    ));

    let oversized = format!(
        "{{\"rawtext\":[{{\"text\":\"{}\"}}]}}",
        "x".repeat(MAX_RAW_TEXT_INPUT_BYTES)
    );
    assert!(matches!(
        parse_raw_text(&oversized),
        Err(UiPacketError::RawTextInputTooLarge { .. })
    ));

    let mut nested = r#"{"rawtext":[{"text":"leaf"}]}"#.to_owned();
    for _ in 1..MAX_RAW_TEXT_DEPTH {
        nested = format!(r#"{{"rawtext":[{nested}]}}"#);
    }
    parse_raw_text(&nested).unwrap();
    nested = format!(r#"{{"rawtext":[{nested}]}}"#);
    assert!(matches!(
        parse_raw_text(&nested),
        Err(UiPacketError::RawTextDepthExceeded {
            depth,
            max: MAX_RAW_TEXT_DEPTH,
        }) if depth == MAX_RAW_TEXT_DEPTH + 1
    ));

    let components = std::iter::repeat_n(r#"{"text":"x"}"#, MAX_RAW_TEXT_COMPONENTS + 1)
        .collect::<Vec<_>>()
        .join(",");
    assert!(matches!(
        parse_raw_text(&format!(r#"{{"rawtext":[{components}]}}"#)),
        Err(UiPacketError::RawTextComponentLimitExceeded { .. })
    ));

    let scores = std::iter::repeat_n(
        r#"{"score":{"name":"*","objective":"kills"}}"#,
        MAX_RAW_TEXT_NODES / 4 + 1,
    )
    .collect::<Vec<_>>()
    .join(",");
    assert!(matches!(
        parse_raw_text(&format!(r#"{{"rawtext":[{scores}]}}"#)),
        Err(UiPacketError::RawTextNodeLimitExceeded { .. })
    ));

    let exact_output = format!(
        "{{\"rawtext\":[{{\"text\":\"{}\"}}]}}",
        "x".repeat(MAX_RAW_TEXT_OUTPUT_BYTES)
    );
    assert_eq!(
        parse_raw_text(&exact_output).unwrap().literal_text().len(),
        MAX_RAW_TEXT_OUTPUT_BYTES
    );
    let output = format!(
        "{{\"rawtext\":[{{\"text\":\"{}\"}}]}}",
        "x".repeat(MAX_RAW_TEXT_OUTPUT_BYTES + 1)
    );
    assert!(matches!(
        parse_raw_text(&output),
        Err(UiPacketError::RawTextOutputTooLarge { .. })
    ));
}

#[test]
fn raw_text_with_document_counts_every_retained_component() {
    // A with-document retains one component per child (each child is its
    // own placeholder argument), so a document with N children costs N
    // components on top of the translate itself.
    let wide = |children: usize| {
        format!(
            r#"{{"rawtext":[{{"translate":"key","with":{{"rawtext":[{}]}}}}]}}"#,
            std::iter::repeat_n(r#"{"text":"x"}"#, children)
                .collect::<Vec<_>>()
                .join(",")
        )
    };

    let exact = parse_raw_text(&wide(MAX_RAW_TEXT_COMPONENTS - 1)).unwrap();
    assert_eq!(exact.components().len(), 1);
    let [RawTextComponent::Translate { with, .. }] = exact.components() else {
        panic!("one translate component with the document children as arguments");
    };
    assert_eq!(with.len(), MAX_RAW_TEXT_COMPONENTS - 1);

    assert!(matches!(
        parse_raw_text(&wide(MAX_RAW_TEXT_COMPONENTS)),
        Err(UiPacketError::RawTextComponentLimitExceeded {
            count,
            max: MAX_RAW_TEXT_COMPONENTS,
        }) if count == MAX_RAW_TEXT_COMPONENTS + 1
    ));
}

#[test]
fn raw_text_with_document_obeys_the_exact_node_boundary() {
    let scores = std::iter::repeat_n(r#"{"score":{"name":"*","objective":"kills"}}"#, 190)
        .collect::<Vec<_>>()
        .join(",");
    let value = format!(
        r#"{{"rawtext":[{scores},{{"translate":"key","with":{{"rawtext":[]}}}},{{"selector":"@a"}}]}}"#
    );

    parse_raw_text(&value).unwrap();
}

#[test]
fn nested_sequence_objects_are_counted_once_at_the_component_boundary() {
    let sequences = std::iter::repeat_n(r#"{"rawtext":[]}"#, MAX_RAW_TEXT_COMPONENTS)
        .collect::<Vec<_>>()
        .join(",");
    let document = parse_raw_text(&format!(r#"{{"rawtext":[{sequences}]}}"#)).unwrap();

    assert_eq!(document.components().len(), MAX_RAW_TEXT_COMPONENTS);
    assert_eq!(document.resolution(), RawTextResolution::LiteralOnly);
}

#[test]
fn raw_text_rejects_explicit_null_translation_arguments() {
    assert!(matches!(
        parse_raw_text(r#"{"rawtext":[{"translate":"key","with":null}]}"#),
        Err(UiPacketError::InvalidRawText)
    ));
}

#[test]
fn json_packet_translation_remains_typed_and_never_becomes_source_json() {
    let event = normalize_json(
        TextPacketType::Json,
        r#"{"rawtext":[{"translate":"multiplayer.player.joined","with":["Alice"]}]}"#.to_owned(),
    )
    .unwrap();
    assert_eq!(event.text.message, Arc::<str>::from(""));
    assert_eq!(
        event.document.resolution(),
        RawTextResolution::RequiresResolver
    );
    assert!(event.document.has_unresolved_components());
    assert!(matches!(
        &event.document.components()[0],
        RawTextComponent::Translate { key, .. } if key.as_ref() == "multiplayer.player.joined"
    ));
}

#[test]
fn protocol_1001_title_object_actions_retain_typed_raw_text_without_json_leakage() {
    for (wire, expected) in [
        (SetTitlePacketType::SetTitleJson, TitleAction::SetTitleJson),
        (
            SetTitlePacketType::SetSubtitleJson,
            TitleAction::SetSubtitleJson,
        ),
        (
            SetTitlePacketType::ActionBarMessageJson,
            TitleAction::ActionBarJson,
        ),
    ] {
        let literal =
            normalize_title_object(wire, r#"{"rawtext":[{"text":"Human title"}]}"#).unwrap();
        assert_eq!(literal.action, expected);
        assert_eq!(literal.text.as_ref(), "Human title");
        assert!(!literal.text.contains("rawtext"));
        assert_eq!(
            literal
                .document
                .as_ref()
                .expect("object action retains RawText")
                .resolution(),
            RawTextResolution::LiteralOnly
        );

        let unresolved = normalize_title_object(
            wire,
            r#"{"rawtext":[{"text":"Human title"},{"selector":"@a"}]}"#,
        )
        .unwrap();
        assert_eq!(unresolved.action, expected);
        assert_eq!(unresolved.text.as_ref(), "Human title");
        assert!(!unresolved.text.contains("rawtext"));
        let document = unresolved
            .document
            .as_ref()
            .expect("object action retains RawText");
        assert_eq!(document.resolution(), RawTextResolution::RequiresResolver);
    }
}

#[test]
fn malformed_title_object_raw_text_fails_closed() {
    assert!(matches!(
        normalize_title_object(
            SetTitlePacketType::SetTitleJson,
            r#"{"rawtext":[{"text":"ok","selector":"@a"}]}"#,
        ),
        Err(UiPacketError::InvalidRawText)
    ));
}

#[test]
fn resolver_substitutes_scores_translations_and_skips_selectors() {
    use std::sync::Arc;

    let document = parse_raw_text(
        r#"{"rawtext":[{"text":"= "},{"translate":"greet.pair","with":[{"text":"Hashim"},{"score":{"name":"*","objective":"coins"}}]},{"selector":"@a"},{"translate":"missing.key"}]}"#,
    )
    .unwrap();
    assert!(document.has_unresolved_components());

    let translate = |key: &str| -> Option<Arc<str>> {
        (key == "greet.pair").then(|| Arc::from("hello %s, you hold %2 coins (%%)"))
    };
    let score = |owner: &str, objective: &str| -> Option<i32> {
        (owner == "Reader" && objective == "coins").then_some(41)
    };
    let selector = |_: &str| -> Option<Arc<str>> { None };
    let resolved = document.resolve(&protocol::RawTextResolver {
        reader_name: "Reader",
        translate: &translate,
        score: &score,
        selector: &selector,
    });

    assert_eq!(
        resolved.text,
        "= hello Hashim, you hold 41 coins (%)missing.key"
    );
    assert_eq!(resolved.unknown_translations, 1);
    assert_eq!(resolved.skipped_selectors, 1);
    assert_eq!(resolved.unresolved_scores, 0);
    assert!(!resolved.truncated);
}

#[test]
fn resolver_output_is_bounded_and_missing_scores_degrade_to_empty_counted_text() {
    use std::sync::Arc;

    let document = parse_raw_text(
        r#"{"rawtext":[{"score":{"name":"Nobody","objective":"none"}},{"text":"tail"}]}"#,
    )
    .unwrap();
    let translate = |_: &str| -> Option<Arc<str>> { None };
    let score = |_: &str, _: &str| -> Option<i32> { None };
    let selector = |_: &str| -> Option<Arc<str>> { None };
    let resolver = protocol::RawTextResolver {
        reader_name: "Reader",
        translate: &translate,
        score: &score,
        selector: &selector,
    };
    let resolved = document.resolve(&resolver);
    assert_eq!(resolved.text, "tail");
    assert_eq!(resolved.unresolved_scores, 1);

    // A pathological translation expansion cannot exceed the output budget.
    let long = parse_raw_text(&format!(
        r#"{{"rawtext":[{{"text":"{}"}},{{"translate":"big"}}]}}"#,
        "a".repeat(protocol::MAX_RAW_TEXT_OUTPUT_BYTES - 16)
    ))
    .unwrap();
    let expand = |_: &str| -> Option<Arc<str>> { Some(Arc::from("b".repeat(64).as_str())) };
    let resolved = long.resolve(&protocol::RawTextResolver {
        reader_name: "Reader",
        translate: &expand,
        score: &score,
        selector: &selector,
    });
    assert!(resolved.text.len() <= protocol::MAX_RAW_TEXT_OUTPUT_BYTES);
    assert!(resolved.truncated);
}

#[test]
fn with_document_children_each_become_their_own_placeholder_argument() {
    use std::sync::Arc;

    // The nested-document `with` form is positionally identical to the list
    // form: every child component fills its own %N slot.
    let document = parse_raw_text(
        r#"{"rawtext":[{"translate":"pair.order","with":{"rawtext":[{"text":"first"},{"text":"second"}]}}]}"#,
    )
    .unwrap();
    let translate =
        |key: &str| -> Option<Arc<str>> { (key == "pair.order").then(|| Arc::from("%2 then %1")) };
    let score = |_: &str, _: &str| -> Option<i32> { None };
    let selector = |_: &str| -> Option<Arc<str>> { None };
    let resolved = document.resolve(&protocol::RawTextResolver {
        reader_name: "Reader",
        translate: &translate,
        score: &score,
        selector: &selector,
    });
    assert_eq!(resolved.text, "second then first");
    assert_eq!(resolved.unknown_translations, 0);
}

#[test]
fn fixed_precision_forms_format_numeric_arguments_exactly() {
    use std::sync::Arc;

    let document = parse_raw_text(
        r#"{"rawtext":[{"translate":"stat.precise","with":[{"text":"3.14159"},{"text":"not a number"}]}]}"#,
    )
    .unwrap();
    let translate = |key: &str| -> Option<Arc<str>> {
        (key == "stat.precise").then(|| Arc::from("%.2f and %.2f and %.f"))
    };
    let score = |_: &str, _: &str| -> Option<i32> { None };
    let selector = |_: &str| -> Option<Arc<str>> { None };
    let resolved = document.resolve(&protocol::RawTextResolver {
        reader_name: "Reader",
        translate: &translate,
        score: &score,
        selector: &selector,
    });
    // The numeric argument rounds to two decimals, the non-numeric argument
    // presents verbatim, and a malformed precision keeps its literal form.
    assert_eq!(resolved.text, "3.14 and not a number and %.f");
}

#[test]
fn selectors_resolve_from_lent_authority_and_otherwise_count_as_skipped() {
    use std::sync::Arc;

    let document = parse_raw_text(
        r#"{"rawtext":[{"selector":"@s"},{"text":" | "},{"selector":"@a"},{"text":" | "},{"selector":"@e[type=cow]"}]}"#,
    )
    .unwrap();
    let translate = |_: &str| -> Option<Arc<str>> { None };
    let score = |_: &str, _: &str| -> Option<i32> { None };
    let selector = |selector: &str| -> Option<Arc<str>> {
        match selector {
            "@s" => Some(Arc::from("Reader")),
            "@a" => Some(Arc::from("Reader, Steve")),
            _ => None,
        }
    };
    let resolved = document.resolve(&protocol::RawTextResolver {
        reader_name: "Reader",
        translate: &translate,
        score: &score,
        selector: &selector,
    });
    assert_eq!(resolved.text, "Reader | Reader, Steve | ");
    assert_eq!(resolved.skipped_selectors, 1);
}
