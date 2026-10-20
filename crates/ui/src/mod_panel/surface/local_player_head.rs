//! Read-only host identity bindings and validation for local portrait images.

use serde_json::{Map, Value};

/// The admitted local player name, or the neutral fallback name.
pub const NAME: &str = "#local_player_name";
/// The host-owned local portrait texture, or an empty texture when unavailable.
pub const HEAD: &str = "#local_player_head";
/// Whether the admitted local skin has a usable portrait.
pub const AVAILABLE: &str = "#local_player_head_available";
/// Whether the surface should show its own neutral artwork.
pub const UNAVAILABLE: &str = "#local_player_head_unavailable";
/// Neutral name shown when the host has no usable local identity.
pub const DEFAULT_NAME: &str = "Player";

/// Requires the reserved host texture binding and excludes arbitrary texture sources.
pub(super) fn validate(control: &Map<String, Value>) -> Result<(), String> {
    // Existing pack images keep their texture contract. Local identity images opt in
    // through the reserved binding and cannot mix it with another texture source.
    if !control
        .get("bindings")
        .and_then(Value::as_array)
        .is_some_and(|bindings| {
            bindings
                .iter()
                .any(|binding| binding.get("binding_name").and_then(Value::as_str) == Some(HEAD))
        })
    {
        return Ok(());
    }
    for (key, value) in control {
        if key.starts_with("texture") && !(key == "texture" && value.as_str() == Some("")) {
            return Err("surface images require the host local player head".into());
        }
    }
    let bindings = control
        .get("bindings")
        .and_then(Value::as_array)
        .ok_or("surface images require the host local player head binding")?;
    let mut texture = false;
    for binding in bindings {
        let source = binding
            .get("binding_name")
            .and_then(Value::as_str)
            .ok_or("surface image binding source is missing")?;
        let target = binding
            .get("binding_name_override")
            .and_then(Value::as_str)
            .unwrap_or(source);
        match target {
            "#texture" if source == HEAD && !texture => texture = true,
            "#visible" | "#alpha" | "#color" | "#size" | "#offset" => {}
            _ => return Err("surface image binding target is unsupported".into()),
        }
    }
    if !texture {
        return Err("surface images require the host local player head binding".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn local_player_head_allows_only_host_texture_and_presentation_bindings() {
        let image = json!({"type":"image","bindings":[
            {"binding_name":"#local_player_head","binding_name_override":"#texture"},
            {"binding_name":"#local_player_head_available","binding_name_override":"#visible"}
        ]});
        validate(image.as_object().unwrap()).unwrap();
        for property in ["texture", "texture_file_system", "texture_path"] {
            let mut invalid = image.clone();
            invalid[property] = json!("arbitrary/path");
            assert!(validate(invalid.as_object().unwrap()).is_err());
        }
        for target in ["#texture", "#texture_file_system", "#texture_path"] {
            let mut invalid = image.clone();
            invalid["bindings"].as_array_mut().unwrap().push(json!({
                "binding_name":"#arbitrary_texture", "binding_name_override":target
            }));
            assert!(validate(invalid.as_object().unwrap()).is_err());
        }
        let mut invalid = image.clone();
        invalid["bindings"][0]["binding_name_override"] = json!("#texture_path");
        assert!(validate(invalid.as_object().unwrap()).is_err());
        validate(
            json!({"type":"image","texture":"textures/ui/original"})
                .as_object()
                .unwrap(),
        )
        .unwrap();
    }
}
