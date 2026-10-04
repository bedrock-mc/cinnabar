//! Respawn-anchor block use before the held block item's placement fallback.

use super::UseSurroundings;

/// Glowstone always uses the anchor: below full charge it charges; at full charge
/// it activates. A charged anchor also accepts other items or an empty hand.
/// Charging works in every dimension; charged
/// activation sets the Nether spawn or explodes elsewhere.
/// Those effects, including consuming glowstone, run only on the server, so this
/// successful interaction predicts no block or inventory change.
/// The Nether's non-glowstone retry at an already-selected spawn needs spawn
/// authority that the client does not yet retain; the server resolves it.
pub(super) fn uses_block(surroundings: &UseSurroundings) -> bool {
    if surroundings.held_block_identifier.as_deref() == Some("minecraft:glowstone") {
        return true;
    }
    let Some(state) = surroundings
        .clicked_canonical_state
        .as_deref()
        .and_then(|state| serde_json::from_str::<serde_json::Value>(state).ok())
    else {
        return false;
    };
    let charge = &state["respawn_anchor_charge"];
    charge
        .as_i64()
        .or_else(|| charge["value"].as_i64())
        .is_some_and(|charge| charge > 0)
}
