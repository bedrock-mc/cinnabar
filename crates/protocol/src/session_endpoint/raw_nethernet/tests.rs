use super::*;

/// Uses only the generated wire grammar when constructing original synthetic intents.
fn address(tag: &str, id: &str) -> String {
    format!("{NETHERNET_TARGET_PREFIX}{tag}/{id}")
}

#[test]
fn explicit_raw_targets_preserve_the_selected_dialect_and_legacy_spelling() {
    let peer = Uuid::from_u128(7);
    let input = address(NETHERNET_SIGNALING_JSONRPC, &peer.to_string()).to_uppercase();
    let raw = NativeRawTarget::parse(&format!(" {input} "))
        .unwrap()
        .unwrap();
    assert!(matches!(raw.recipient(), Recipient::Messaging(actual) if *actual == peer));
    assert!(raw.matches_recipient(&Recipient::Messaging(peer)));
    assert!(!raw.matches_recipient(&Recipient::Legacy(peer.to_string())));
    assert_eq!(
        raw.connection_target().transport,
        NativeTransport::NetherNetMessaging
    );
    for id in [
        "0",
        "00042",
        "18446744073709551615",
        "00000000-0000-0000-0000-00000000000A",
    ] {
        let raw = NativeRawTarget::parse(&address(NETHERNET_SIGNALING_WEBSOCKET, id))
            .unwrap()
            .unwrap();
        assert!(matches!(raw.recipient(), Recipient::Legacy(actual) if actual == id));
        assert_eq!(raw.connection_target().address, id);
        assert_eq!(
            raw.connection_target().transport,
            NativeTransport::NetherNetLegacy
        );
    }
}

#[test]
fn raw_service_syntax_never_guesses_signaling_or_accepts_a_host_as_a_recipient() {
    for input in [
        "42".into(),
        Uuid::from_u128(7).to_string(),
        NETHERNET_TARGET_PREFIX.into(),
        address("unknown", "42"),
        address(NETHERNET_SIGNALING_JSONRPC, "42"),
        address(NETHERNET_SIGNALING_JSONRPC, &Uuid::nil().to_string()),
        address(NETHERNET_SIGNALING_WEBSOCKET, "host.invalid:19132"),
        address(NETHERNET_SIGNALING_WEBSOCKET, "42/extra"),
        address(NETHERNET_SIGNALING_WEBSOCKET, " 42"),
        address(NETHERNET_SIGNALING_WEBSOCKET, "+42"),
        address(NETHERNET_SIGNALING_WEBSOCKET, "18446744073709551616"),
    ] {
        assert!(NativeRawTarget::recognizes(&input));
        assert_eq!(
            NativeRawTarget::parse(&input).err().unwrap().kind(),
            io::ErrorKind::InvalidInput
        );
    }
    for direct in ["play.invalid:19132", "[::1]:19132", "realm_id/7"] {
        assert!(!NativeRawTarget::recognizes(direct));
        assert!(NativeRawTarget::parse(direct).unwrap().is_none());
    }
}
