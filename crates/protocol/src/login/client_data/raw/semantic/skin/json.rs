//! Bounded decoded-skin JSON admission without constructing a recursive Value tree.

use super::super::json_generated::MAX_NESTING_DEPTH;
use serde_json::value::RawValue;

/// Keeps serde's iterative raw parser as the sole JSON grammar owner after bounding its scratch.
pub(super) fn object_or_null(decoded: &[u8]) -> bool {
    let Ok(input) = std::str::from_utf8(decoded) else {
        return false;
    };
    if !within_limits(input) {
        return false;
    }
    serde_json::from_str::<&RawValue>(input)
        .is_ok_and(|raw| raw.get() == "null" || raw.get().starts_with('{'))
}

/// Counts only unquoted containers and checks float representability; serde still checks all syntax.
/// This bounds RawValue's iterative bracket scratch before it can grow, without recursive allocation.
fn within_limits(input: &str) -> bool {
    let bytes = input.as_bytes();
    let mut index = 0;
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    while let Some(&byte) = bytes.get(index) {
        index += 1;
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'[' | b'{' => {
                depth += 1;
                if depth > MAX_NESTING_DEPTH {
                    return false;
                }
            }
            b']' | b'}' => {
                let Some(parent) = depth.checked_sub(1) else {
                    return false;
                };
                depth = parent;
            }
            b'-' | b'0'..=b'9' => {
                let start = index - 1;
                while bytes.get(index).is_some_and(|byte| {
                    matches!(byte, b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                }) {
                    index += 1;
                }
                if !input[start..index]
                    .parse::<f64>()
                    .is_ok_and(|number| number.is_finite())
                {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests;
