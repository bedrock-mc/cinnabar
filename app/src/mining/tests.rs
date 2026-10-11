use protocol::PlayerInputMode;

use gameplay::mining::{creative_reach, survival_reach};

#[test]
fn creative_reach_is_frozen_per_input_mode() {
    assert_eq!(creative_reach(PlayerInputMode::Mouse), 5.7);
    assert_eq!(creative_reach(PlayerInputMode::GamePad), 5.6);
    assert_eq!(creative_reach(PlayerInputMode::Touch), 12.0);
    assert_eq!(survival_reach(PlayerInputMode::Touch), 6.7);
}

#[test]
fn unknown_selected_slot_mines_as_a_bare_hand() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(7);

    // Before the inventory arrives the slot is Unknown; mining must still work
    // by hand rather than refusing (the "can't break blocks" regression).
    player_runtime.inventory.set_local_selected_slot(0);
    let selection = super::hand_interaction_selection(&player_runtime)
        .expect("unknown selection resolves to an empty hand");
    assert_eq!(selection.item.network_id(), 0);
}
