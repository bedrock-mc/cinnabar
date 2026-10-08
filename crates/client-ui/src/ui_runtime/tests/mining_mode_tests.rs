use super::*;

#[test]
fn mining_negotiation_distinguishes_false_from_unknown_and_resets_by_session() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = UiRuntime::new(1);
    assert_eq!(
        player_runtime.facts.server_authoritative_block_breaking(),
        None
    );
    player_runtime
        .facts
        .install_block_breaking_mode(1, true, true);
    assert_eq!(
        player_runtime.facts.server_authoritative_block_breaking(),
        Some(true)
    );
    player_runtime
        .facts
        .install_block_breaking_mode(1, false, true);
    assert_eq!(
        player_runtime.facts.server_authoritative_block_breaking(),
        Some(false)
    );
    player_runtime
        .facts
        .install_block_breaking_mode(0, true, true);
    assert_eq!(
        player_runtime.facts.server_authoritative_block_breaking(),
        Some(false)
    );
    player_runtime.begin_session(2);
    runtime.begin_session(2);
    assert_eq!(
        player_runtime.facts.server_authoritative_block_breaking(),
        None
    );
    player_runtime
        .facts
        .install_block_breaking_mode(2, true, true);
    player_runtime.begin_session(2);
    runtime.begin_session(2);
    assert_eq!(
        player_runtime.facts.server_authoritative_block_breaking(),
        Some(true)
    );
    // Accepted repeated setup clears explicitly, independently of begin_session.
    player_runtime.facts.clear_block_breaking_mode();
    player_runtime
        .facts
        .install_block_breaking_mode(2, false, false);
    assert_eq!(
        player_runtime.facts.server_authoritative_block_breaking(),
        None
    );
}
