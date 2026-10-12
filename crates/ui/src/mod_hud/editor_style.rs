//! Optional colors for the host-owned HUD layout editor chrome.

use serde::{Deserialize, Serialize};

/// Largest corner radius, in GUI pixels, accepted for the eight-pixel resize handles.
pub const MAX_EDITOR_HANDLE_RADIUS: f32 = 4.;

/// Recolors the outlines, resize handles, alignment guides and grid that the host
/// draws around previews. Omitted fields keep the built-in editor appearance.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EditorStyle {
    /// Outline of a card that is not selected.
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
}

impl Default for EditorStyle {
    fn default() -> Self {
        Self {
            outline: [1., 1., 1., 0.55],
            selected: [0.3, 0.85, 1., 1.],
            handle: [0.3, 0.85, 1., 1.],
            handle_hover: [1.; 4],
            handle_pressed: [0.2, 0.65, 1., 1.],
            handle_radius: 0.,
            guide: [0.2, 0.85, 1., 0.8],
            grid: [1., 1., 1., 0.09],
        }
    }
}

impl EditorStyle {
    /// Rejects colors outside zero to one and handle radii outside their bound.
    pub fn validate(&self) -> Result<(), String> {
        for color in [
            self.outline,
            self.selected,
            self.handle,
            self.handle_hover,
            self.handle_pressed,
            self.guide,
            self.grid,
        ] {
            super::color(color)?;
        }
        if !self.handle_radius.is_finite()
            || !(0. ..=MAX_EDITOR_HANDLE_RADIUS).contains(&self.handle_radius)
        {
            return Err("HUD editor handle radius exceeds its bound".into());
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
        assert!(style.validate().is_ok());
        for invalid in [
            r#"{"guide":[1,1,1,2]}"#,
            r#"{"grid":[-0.1,0,0,1]}"#,
            r#"{"handle_radius":5}"#,
        ] {
            let style: EditorStyle = serde_json::from_str(invalid).unwrap();
            assert!(style.validate().is_err(), "{invalid}");
        }
        assert!(serde_json::from_str::<EditorStyle>(r#"{"accent":[1,1,1,1]}"#).is_err());
    }
}
