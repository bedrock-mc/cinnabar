//! Retained hand-toggle classifier.

/// Resolves the earlier identifier-based hand-toggle decision.
pub fn toggled_states(
    identifier: &str,
    canonical_state: &str,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let iron = identifier == "minecraft:iron_trapdoor";
    let (property, press_only) =
        if identifier == "minecraft:lever" || (!iron && identifier.ends_with("_trapdoor")) {
            ("open_bit", false)
        } else if identifier.ends_with("_button") {
            ("button_pressed_bit", true)
        } else {
            return None;
        };
    let mut states =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(canonical_state).ok()?;
    let entry = states.get_mut(property)?;
    // Typed values carry the bit in `value`.
    let bit = match entry {
        serde_json::Value::Object(typed) => typed.get_mut("value")?,
        plain => plain,
    };
    let set = match bit {
        serde_json::Value::Bool(value) => *value,
        serde_json::Value::Number(value) => value.as_u64()? != 0,
        _ => return None,
    };
    if press_only && set {
        return None;
    }
    *bit = match bit {
        serde_json::Value::Bool(_) => serde_json::Value::Bool(!set),
        _ => serde_json::Value::from(u8::from(!set)),
    };
    Some(states)
}
