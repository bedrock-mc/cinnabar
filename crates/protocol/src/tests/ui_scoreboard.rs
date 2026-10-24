use bedrock_protocol_1_26_51::{packets::SetDisplayObjective, wire};
use protocol::{MAX_UI_TEXT_BYTES, ObjectiveEvent, UiEvent, WorldEvent};

/// Decodes and normalizes a scoreboard packet through the network admission path.
fn objective_event(packet: SetDisplayObjective) -> Result<ObjectiveEvent, String> {
    let bytes = protocol::encode(&protocol::Packet::new(packet)).unwrap();
    let mut decoded = protocol::decode_batch(bytes).map_err(|error| error.to_string())?;
    if decoded.len() != 1 {
        return Err(format!(
            "expected one objective packet, got {}",
            decoded.len()
        ));
    }
    match protocol::into_world_event(decoded.pop().unwrap(), 0) {
        Ok(Some(WorldEvent::Ui(UiEvent::Objective(event)))) => Ok(event),
        other => Err(format!("expected an objective event, got {other:?}")),
    }
}

#[test]
fn clear_display_objective_discards_unused_presentation_text() {
    let event = objective_event(SetDisplayObjective {
        display_slot_name: "sidebar".into(),
        objective_name: "".into(),
        objective_display_name: "x".repeat(MAX_UI_TEXT_BYTES + 1).into(),
        criteria_name: "y".repeat(MAX_UI_TEXT_BYTES + 1).into(),
        sort_order: wire::ZigZag32(i32::MAX),
    })
    .expect("a clear packet only consumes the slot and empty objective name");
    let ObjectiveEvent::Display {
        display_slot,
        objective_name,
        display_name,
        criteria_name,
        ..
    } = event
    else {
        panic!("expected a display event");
    };
    assert_eq!(display_slot.as_ref(), "sidebar");
    assert!(objective_name.is_empty());
    assert!(display_name.is_empty());
    assert!(criteria_name.is_empty());
}

#[test]
fn nonempty_display_objective_still_bounds_presentation_text() {
    assert!(
        objective_event(SetDisplayObjective {
            display_slot_name: "sidebar".into(),
            objective_name: "round".into(),
            objective_display_name: "x".repeat(MAX_UI_TEXT_BYTES + 1).into(),
            criteria_name: "dummy".into(),
            sort_order: wire::ZigZag32(0),
        })
        .is_err()
    );
}

#[test]
fn clear_display_objective_ignores_unused_invalid_utf8() {
    let event = objective_event(SetDisplayObjective {
        display_slot_name: "sidebar".into(),
        objective_display_name: bytes::Bytes::from_static(b"\xff").into(),
        criteria_name: bytes::Bytes::from_static(b"\xfe").into(),
        ..Default::default()
    })
    .expect("a clear packet does not consume presentation text");
    assert!(matches!(
        event,
        ObjectiveEvent::Display { objective_name, display_name, criteria_name, .. }
            if objective_name.is_empty() && display_name.is_empty() && criteria_name.is_empty()
    ));
}
