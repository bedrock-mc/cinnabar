//! Bounded typography, colors and handle geometry for host-owned HUD layout chrome.

use serde::{Deserialize, Serialize};

/// Largest corner radius, in GUI pixels, accepted for the eight-pixel resize handles.
pub const MAX_EDITOR_HANDLE_RADIUS: f32 = 4.;

/// Largest label scale that fits the twelve-GUI-pixel editor caption band.
const MAX_TEXT_SCALE: f32 = 0.8;

/// Styles the labels, outlines, resize handles, alignment guides and grid that the host
/// draws around previews. Omitted fields keep the built-in editor appearance.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EditorStyle {
    /// Draws one measured name-and-scale caption for the selected widget.
    pub selection_caption: bool,
    /// Text on the selected-color caption background.
    pub caption_text: [f32; 4],
    /// Host-owned face used by editor labels and distance annotations.
    pub font: super::CardFont,
    /// Label scale, independent of each widget's gameplay scale.
    pub text_scale: f32,
    /// Top toolbar band, in GUI pixels, reserved from selection and distance captions.
    pub top_clearance: f32,
    /// Outline of a hovered card that is not selected.
    pub outline: [f32; 4],
    /// Outline of the selected card.
    pub selected: [f32; 4],
    /// Resize handle at rest.
    pub handle: [f32; 4],
    /// Resize handle under the pointer.
    pub handle_hover: [f32; 4],
    /// Resize handle while it is dragged.
    pub handle_pressed: [f32; 4],
    /// Corner radius of the five-pixel resize handle mark.
    pub handle_radius: f32,
    /// Temporary alignment guide shown while dragging.
    pub guide: [f32; 4],
    /// Built-in snapping grid lines, drawn only without an extension surface.
    pub grid: [f32; 4],
    /// Fill of the distance measurement pills around the selected card.
    pub measurement: [f32; 4],
}

impl Default for EditorStyle {
    fn default() -> Self {
        Self {
            selection_caption: false,
            caption_text: [0.04, 0.04, 0.04, 1.],
            font: super::CardFont::Native,
            text_scale: MAX_TEXT_SCALE,
            top_clearance: 0.,
            outline: [1., 1., 1., 0.55],
            selected: [0.3, 0.85, 1., 1.],
            handle: [0.3, 0.85, 1., 1.],
            handle_hover: [1.; 4],
            handle_pressed: [0.2, 0.65, 1., 1.],
            handle_radius: 0.,
            guide: [0.2, 0.85, 1., 0.8],
            grid: [1., 1., 1., 0.09],
            measurement: [0.055, 0.065, 0.08, 0.97],
        }
    }
}

impl EditorStyle {
    /// Rejects colors outside zero to one and handle radii outside their bound.
    pub fn validate(&self) -> Result<(), String> {
        for color in [
            self.caption_text,
            self.outline,
            self.selected,
            self.handle,
            self.handle_hover,
            self.handle_pressed,
            self.guide,
            self.grid,
            self.measurement,
        ] {
            super::color(color)?;
        }
        if !self.handle_radius.is_finite()
            || !(0. ..=MAX_EDITOR_HANDLE_RADIUS).contains(&self.handle_radius)
        {
            return Err("HUD editor handle radius exceeds its bound".into());
        }
        if !self.text_scale.is_finite() || !(0.25..=MAX_TEXT_SCALE).contains(&self.text_scale) {
            return Err("HUD editor text scale exceeds its bound".into());
        }
        if !self.top_clearance.is_finite() || !(0. ..=128.).contains(&self.top_clearance) {
            return Err("HUD editor top clearance exceeds its bound".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_styles_keep_builtin_colors_and_reject_unbounded_values() {
        let style: EditorStyle =
            serde_json::from_str(r#"{"selected":[1,0.5,0,1],"handle_radius":2.5}"#).unwrap();
        assert_eq!(style.selected, [1., 0.5, 0., 1.]);
        assert_eq!(style.outline, EditorStyle::default().outline);
        assert_eq!(style.measurement, [0.055, 0.065, 0.08, 0.97]);
        assert!(style.validate().is_ok());
        let style: EditorStyle =
            serde_json::from_str(r#"{"font":"sans","text_scale":0.5}"#).unwrap();
        assert_eq!(style.font, super::super::CardFont::Sans);
        assert_eq!(style.text_scale, 0.5);
        assert!(style.validate().is_ok());
        for invalid in [
            r#"{"guide":[1,1,1,2]}"#,
            r#"{"grid":[-0.1,0,0,1]}"#,
            r#"{"handle_radius":5}"#,
            r#"{"measurement":[0,0,0,1.5]}"#,
            r#"{"caption_text":[0,0,0,1.5]}"#,
            r#"{"text_scale":0.24}"#,
            r#"{"text_scale":0.81}"#,
            r#"{"top_clearance":129}"#,
        ] {
            let style: EditorStyle = serde_json::from_str(invalid).unwrap();
            assert!(style.validate().is_err(), "{invalid}");
        }
        assert!(serde_json::from_str::<EditorStyle>(r#"{"accent":[1,1,1,1]}"#).is_err());
    }
}
