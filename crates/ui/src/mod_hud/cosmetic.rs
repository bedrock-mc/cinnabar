//! Validated native actor and food presentation policies, separate from card geometry.

use super::{color, text_scale};
use serde::{Deserialize, Serialize};

/// Native hunger-control and inventory food-tooltip presentation options.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HungerOverlay {
    #[serde(default)]
    pub saturation: bool,
    #[serde(default)]
    pub held_food: bool,
    /// Overrides the legacy combined held-food option when supplied.
    #[serde(default)]
    pub potential_hunger: Option<bool>,
    #[serde(default)]
    pub potential_saturation: Option<bool>,
    #[serde(default)]
    pub inverse_color: bool,
    #[serde(default)]
    pub color: Option<[f32; 4]>,
    #[serde(default)]
    pub tooltips: bool,
}

/// Cosmetic actor presentation; it never alters actor state or native eligibility.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorStyle {
    #[serde(default)]
    pub hurt_color: Option<[f32; 4]>,
    #[serde(default)]
    pub hurt_armor_color: Option<[f32; 4]>,
    /// Cosmetic overlay for the current native admitted attackable player.
    #[serde(default)]
    pub attackable_color: Option<[f32; 4]>,
    #[serde(default)]
    pub nametags: Option<NametagStyle>,
    #[serde(default)]
    pub totems: Option<super::TotemCounterStyle>,
}

/// Styles the already eligible native tag quads without exposing hidden actors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NametagStyle {
    #[serde(default)]
    pub hide: bool,
    #[serde(default)]
    pub hide_players: bool,
    #[serde(default = "text_scale")]
    pub opacity: f32,
    #[serde(default)]
    pub replace_own_text_color: bool,
    #[serde(default)]
    pub replace_text_color: bool,
    #[serde(default)]
    pub shadow: bool,
    #[serde(default)]
    pub show_self: bool,
    #[serde(default)]
    pub show_when_hud_hidden: bool,
    #[serde(default)]
    pub hide_passengers: bool,
    #[serde(default)]
    pub own_text_color: Option<[f32; 4]>,
    #[serde(default = "nametag_border_width")]
    pub border_width: f32,
    #[serde(default)]
    pub border_color: Option<[f32; 4]>,
    #[serde(default = "text_scale")]
    pub scale: f32,
    #[serde(default)]
    pub text_color: Option<[f32; 4]>,
    #[serde(default)]
    pub background_color: Option<[f32; 4]>,
}

/// Uses a half-pixel border when a nametag policy omits its width.
fn nametag_border_width() -> f32 {
    0.5
}

/// Rejects invalid native cosmetics without changing ordinary HUD card limits.
pub(super) fn validate(
    actor: Option<&ActorStyle>,
    hunger: Option<&HungerOverlay>,
) -> Result<(), String> {
    if hunger
        .and_then(|overlay| overlay.color)
        .is_some_and(|rgba| color(rgba).is_err())
    {
        return Err("invalid hunger presentation color".into());
    }
    if let Some(style) = actor {
        let invalid_color = |rgba: &[f32; 4]| color(*rgba).is_err();
        if style.hurt_armor_color.as_ref().is_some_and(invalid_color)
            || style.hurt_color.as_ref().is_some_and(invalid_color)
            || style.attackable_color.as_ref().is_some_and(invalid_color)
            || style
                .totems
                .as_ref()
                .is_some_and(|totems| totems.validate().is_err())
            || style.nametags.as_ref().is_some_and(|tag| {
                !tag.opacity.is_finite()
                    || !(0. ..=1.).contains(&tag.opacity)
                    || !tag.border_width.is_finite()
                    || !(0.0..=8.0).contains(&tag.border_width)
                    || tag.own_text_color.as_ref().is_some_and(invalid_color)
                    || !tag.scale.is_finite()
                    || !(0.5..=3.0).contains(&tag.scale)
                    || tag.border_color.as_ref().is_some_and(invalid_color)
                    || tag.text_color.as_ref().is_some_and(invalid_color)
                    || tag.background_color.as_ref().is_some_and(invalid_color)
            })
        {
            return Err("invalid actor presentation style".into());
        }
    }
    Ok(())
}
