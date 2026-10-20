//! Selects only cards belonging to the guest that owns the enclosing panel.

use serde_json::{Map, Value};

/// Host renderer for a live card, selected by the bindable `card_id` property.
pub const RENDERER: &str = "cinnabar_hud_card";
/// Preview multipliers use the same scale bounds as HUD cards.
pub const MIN_SCALE: f32 = crate::mod_hud::MIN_CARD_SCALE;
pub const MAX_SCALE: f32 = crate::mod_hud::MAX_CARD_SCALE;

/// Validates literals; bound values receive the same checks after JSON-UI binding.
pub(super) fn validate(object: &Map<String, Value>) -> Result<(), String> {
    if let Some(value) = object.get("card_id")
        && !value
            .as_str()
            .is_some_and(|id| id.is_empty() || crate::mod_hud::valid_card_id(id))
    {
        return Err("HUD preview card_id must be a local card identifier".into());
    }
    if let Some(value) = object.get("scale")
        && !value.as_f64().is_some_and(|scale| {
            scale.is_finite() && (f64::from(MIN_SCALE)..=f64::from(MAX_SCALE)).contains(&scale)
        })
    {
        return Err("HUD preview scale is outside its bounds".into());
    }
    if let Some(value) = object.get("alignment")
        && !value.as_array().is_some_and(|alignment| {
            alignment.len() == 2
                && alignment.iter().all(|axis| {
                    axis.as_f64()
                        .is_some_and(|axis| axis.is_finite() && (0.0..=1.0).contains(&axis))
                })
        })
    {
        return Err("HUD preview alignment must contain two fractions".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hud_card_preview_admission_keeps_rounded_shadows_bounded_on_the_same_surface() {
        use crate::mod_panel::Surface;
        use json_ui::rounded::{MAX_SHADOW_BLUR, MAX_SHADOW_OFFSET, MAX_SHADOW_SPREAD};

        for (blur, scale, accepted) in [
            (MAX_SHADOW_BLUR, f64::from(MAX_SCALE), true),
            (MAX_SHADOW_BLUR + 1., 1., false),
            (0., f64::from(MAX_SCALE) + 1., false),
        ] {
            let document = json!({"namespace":"preview","root":{
                "type":"panel","controls":[
                    {"shadow":{"type":"custom","renderer":json_ui::rounded::RENDERER,
                        "size":[80,40],"shadow_blur":blur,
                        "shadow_offset":[-MAX_SHADOW_OFFSET,MAX_SHADOW_OFFSET],
                        "shadow_spread":MAX_SHADOW_SPREAD,"shadow_color":[0,0,0,0.5]}},
                    {"card":{"type":"custom","renderer":RENDERER,
                        "size":[80,40],"card_id":"fps","scale":scale}}
                ]
            }});
            let surface: Surface = serde_json::from_value(json!({
                "screen":"preview.root","document":document.to_string()
            }))
            .unwrap();
            assert_eq!(
                surface.validate().is_ok(),
                accepted,
                "blur={blur}, scale={scale}"
            );
        }
    }

    #[test]
    fn hud_card_preview_admission_bounds_literals_and_allows_bound_selectors() {
        for node in [
            json!({"card_id":"fps","scale":1.5,"alignment":[0,1]}),
            json!({"bindings":[{"binding_name":"#item_card","binding_name_override":"#card_id"}]}),
        ] {
            assert!(validate(node.as_object().unwrap()).is_ok());
        }
        for node in [
            json!({"card_id":"other.fps"}),
            json!({"scale":0}),
            json!({"scale":6}),
            json!({"alignment":[-1,0]}),
            json!({"alignment":[0]}),
            json!({"card_id":12}),
        ] {
            assert!(validate(node.as_object().unwrap()).is_err());
        }
        let hud: crate::mod_hud::Hud =
            serde_json::from_value(json!({"cards":[{"id":"fps","rows":[]}]})).unwrap();
        assert!(!hud.cards[0].preview_only);
    }
}
