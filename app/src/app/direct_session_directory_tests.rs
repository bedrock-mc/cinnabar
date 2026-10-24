use super::*;
use crate::args::ParseOutcome;

/// Direct startup binds its actual addressed target and session dialog without a Core or file read.
#[cfg(feature = "native-networking")]
#[test]
fn native_addressed_startup_retains_its_original_trust_owner_and_optional_port() {
    let layout = launcher::test_support::scratch("native-addressed-startup");
    for address in ["fixture.invalid", "fixture.invalid:19133"] {
        let parsed = run_args(&[
            "client",
            "--address",
            address,
            "--native-networking",
            "addressed",
        ]);
        let endpoint = parsed.session_endpoint().unwrap();
        let (endpoint, trust) =
            crate::session::native_trust::initial(Some(endpoint), &layout).unwrap();
        let Some(protocol::SessionEndpoint::Native(target)) = endpoint else {
            panic!("native addressed startup selected Core");
        };
        assert_eq!(target.transport, protocol::NativeTransport::Addressed);
        assert_eq!(target.address, address);
        target.validate().unwrap();
        assert!(trust.as_ref().unwrap().prompt().is_none());
        let controller = SessionController::default().with_initial_trust(trust);
        assert!(controller.status().connecting);
        assert!(!layout.server_trust_file().exists());
    }
}

/// Native command-line connections use the same endpoint grammar as saved menu destinations.
#[cfg(feature = "native-networking")]
#[test]
fn native_cli_endpoint_normalizes_bare_host_and_ipv6_addresses() {
    for address in [
        "play.example.net:19133",
        "[2001:db8::7]:19134",
        "play.example.net",
        "2001:db8::7",
        " play.example.net ",
    ] {
        let parsed = run_args(&[
            "client",
            "--address",
            address,
            "--native-networking",
            "raknet",
        ]);
        let bridge::ConnectTarget::RakNet(expected) =
            launcher_host::destination::target_for(address)
        else {
            panic!("direct fixture must use the ordinary endpoint owner");
        };
        let protocol::SessionEndpoint::Native(target) = parsed.session_endpoint().unwrap();
        assert_eq!(target.transport, protocol::NativeTransport::RakNet);
        assert_eq!(
            target.address, expected,
            "native command-line endpoint lost existing normalization for {address}"
        );
    }
}

/// Gathering locators remain account-resolved; friend display addresses never grant a selection.
#[cfg(feature = "native-networking")]
#[test]
fn native_cli_services_preserve_account_selection_authority() {
    let address = format!(
        "{}{}",
        launcher::menu::EXPERIENCE_ADDRESS_PREFIX,
        "00000000-0000-0000-0000-000000000007"
    );
    let parsed = run_args(&[
        "client",
        "--address",
        &address,
        "--native-networking",
        "raknet",
    ]);
    let protocol::SessionEndpoint::Native(target) = parsed.session_endpoint().unwrap();
    assert_eq!(target.address, address);
    assert_eq!(target.transport, protocol::NativeTransport::RakNet);
    let friend = format!("{}123", bridge::contract::FRIEND_TARGET_PREFIX);
    let parsed = run_args(&[
        "client",
        "--address",
        &friend,
        "--native-networking",
        "raknet",
    ]);
    assert!(matches!(
        parsed.session_endpoint(),
        Err(args::ArgsError::NativeFriendSelectionRequired)
    ));
}

/// Realm namespaces stay opaque and account-resolved, including an existing invite or case alias.
#[cfg(feature = "native-networking")]
#[test]
fn native_cli_realms_keep_opaque_locators_for_account_resolution() {
    for address in [
        format!("{}7", bridge::contract::REALM_TARGET_PREFIX),
        format!(
            " {}7 ",
            bridge::contract::REALM_TARGET_PREFIX.to_ascii_uppercase()
        ),
        format!("{}synthetic-code", bridge::contract::REALM_CODE_PREFIX),
    ] {
        assert!(launcher_host::native_account::is_realm_target(&address));
        let parsed = run_args(&[
            "client",
            "--address",
            &address,
            "--native-networking",
            "nethernet-lan",
        ]);
        let protocol::SessionEndpoint::Native(target) = parsed.session_endpoint().unwrap();
        assert_eq!(target.address, address.trim());
        assert_eq!(target.transport, protocol::NativeTransport::RakNet);
    }
}

fn run_args(arguments: &[&str]) -> args::ClientArgs {
    match args::ClientArgs::parse_from(arguments.to_vec()) {
        Ok(ParseOutcome::Run(parsed)) => *parsed,
        outcome => panic!("expected run arguments, got {outcome:?}"),
    }
}
