use super::*;

#[test]
fn mining_negotiation_distinguishes_false_from_unknown_and_resets_by_session() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut runtime = UiRuntime::new(1);
    assert_eq!(
        runtime.server_authoritative_block_breaking(&player_runtime),
        None
    );
    runtime.install_block_breaking_mode(&mut player_runtime, 1, true, true);
    assert_eq!(
        runtime.server_authoritative_block_breaking(&player_runtime),
        Some(true)
    );
    runtime.install_block_breaking_mode(&mut player_runtime, 1, false, true);
    assert_eq!(
        runtime.server_authoritative_block_breaking(&player_runtime),
        Some(false)
    );
    runtime.install_block_breaking_mode(&mut player_runtime, 0, true, true);
    assert_eq!(
        runtime.server_authoritative_block_breaking(&player_runtime),
        Some(false)
    );
    runtime.begin_session(&mut player_runtime, 2);
    assert_eq!(
        runtime.server_authoritative_block_breaking(&player_runtime),
        None
    );
    runtime.install_block_breaking_mode(&mut player_runtime, 2, true, true);
    runtime.begin_session(&mut player_runtime, 2);
    assert_eq!(
        runtime.server_authoritative_block_breaking(&player_runtime),
        Some(true)
    );
    // Accepted repeated setup clears explicitly, independently of begin_session.
    runtime.clear_block_breaking_mode(&mut player_runtime);
    runtime.install_block_breaking_mode(&mut player_runtime, 2, false, false);
    assert_eq!(
        runtime.server_authoritative_block_breaking(&player_runtime),
        None
    );
}
