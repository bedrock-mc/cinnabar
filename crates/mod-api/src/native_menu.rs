//! Focused native menu input for trusted packet mods; the guest owns menu behavior.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::HashSet;

/// Native menu JSON contract version.
pub const VERSION: u32 = 1;
/// The host can display a retained custom panel and deliver its focused input.
pub const CONTROLS: u32 = 2;
/// Largest focused input batch accepted by the native menu callback.
pub const MAX_INPUT_BYTES: usize = 64 * 1024;
/// Largest output, including an escaped bounded panel document.
pub const MAX_OUTPUT_BYTES: usize = 512 * 1024;

/// One native control edit or action, identified by the guest's declared control ID.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub id: String,
    pub value: f32,
}

/// One focused physical key press and its optional composed text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Last painted bounds of one guest-requested inert gameplay HUD root.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HudGeometry {
    pub rect: [f32; 4],
    pub size: [f32; 2],
}

/// Optional version-one input facts explicitly accepted by a packet guest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputExtension {
    PointerState,
    ShiftState,
    HudGeometry,
    ControlPreviews,
}

/// A retained opt-in list; an empty list restores the original version-one input shape.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InputExtensions(pub Vec<InputExtension>);

impl InputExtensions {
    /// Each known input group may be accepted at most once.
    pub fn validate(&self) -> Result<(), String> {
        let mut seen = HashSet::new();
        if self.0.iter().all(|extension| seen.insert(extension)) {
            Ok(())
        } else {
            Err("native menu input extensions must be unique".into())
        }
    }
}

/// The original wire shape plus only facts a guest has explicitly accepted.
#[derive(Serialize)]
struct WireInput<'a> {
    version: u32,
    open: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    viewport: Option<[f32; 2]>,
    events: &'a [Event],
    keys: &'a [Key],
    #[serde(skip_serializing_if = "Option::is_none")]
    pointer_held: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pointer_cancelled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shift_held: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hud_geometry: Option<&'a BTreeMap<String, HudGeometry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preview_sizes: Option<&'a BTreeMap<String, [f32; 2]>>,
}

impl Key {
    /// Physical key names use the same bounded spelling as the shared controls contract.
    pub fn is_valid(&self) -> bool {
        crate::control_key_valid(&self.key)
            && self
                .text
                .as_ref()
                .is_none_or(|text| text.len() <= 256 && !text.chars().any(char::is_control))
    }
}

/// One input-owning menu sample. Closed menus still poll for guest-requested visibility.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub version: u32,
    pub open: bool,
    /// Focused active menu pointer state, including release samples without control edits.
    #[serde(default)]
    pub pointer_held: bool,
    /// One-shot interruption of the active pointer gesture; ordinary releases leave this false.
    #[serde(default)]
    pub pointer_cancelled: bool,
    /// Latest focused physical left/right Shift state, even when UI input consumes the key.
    #[serde(default)]
    pub shift_held: bool,
    /// Available authored GUI units before optional reference-size fitting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<[f32; 2]>,
    /// Available named HUD roots in the same GUI coordinate basis as the menu viewport.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hud_geometry: BTreeMap<String, HudGeometry>,
    /// Intrinsic, untransformed pack-control preview dimensions in native-panel GUI units.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub preview_sizes: BTreeMap<String, [f32; 2]>,
    pub events: Vec<Event>,
    pub keys: Vec<Key>,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            version: VERSION,
            open: false,
            pointer_held: false,
            pointer_cancelled: false,
            shift_held: false,
            viewport: None,
            hud_geometry: BTreeMap::new(),
            preview_sizes: BTreeMap::new(),
            events: Vec::new(),
            keys: Vec::new(),
        }
    }
}

impl Input {
    /// Projects input onto the guest's accepted wire fields without changing physical state.
    pub fn with_extensions(&self, extensions: &InputExtensions) -> impl Serialize + '_ {
        let Self {
            version,
            open,
            pointer_held,
            pointer_cancelled,
            shift_held,
            viewport,
            hud_geometry,
            preview_sizes,
            events,
            keys,
        } = self;
        let mut wire = WireInput {
            version: *version,
            open: *open,
            viewport: *viewport,
            events,
            keys,
            pointer_held: None,
            pointer_cancelled: None,
            shift_held: None,
            hud_geometry: None,
            preview_sizes: None,
        };
        for extension in &extensions.0 {
            match extension {
                InputExtension::PointerState => {
                    wire.pointer_held = Some(*pointer_held);
                    wire.pointer_cancelled = Some(*pointer_cancelled);
                }
                InputExtension::ShiftState => wire.shift_held = Some(*shift_held),
                InputExtension::HudGeometry => wire.hud_geometry = Some(hud_geometry),
                InputExtension::ControlPreviews => wire.preview_sizes = Some(preview_sizes),
            }
        }
        wire
    }

    /// Rejects unbounded or malformed input before invoking a packet guest.
    pub fn is_valid(&self) -> bool {
        self.version == VERSION
            && self.viewport.is_none_or(|size| {
                size.iter()
                    .all(|value| value.is_finite() && *value > 0.0 && *value <= 1_000_000.0)
            })
            && self.events.len() <= crate::MAX_CONTROL_KEYS
            && self.keys.len() <= crate::MAX_CONTROL_KEYS
            && self.preview_sizes.len() <= crate::MAX_CONTROL_KEYS
            && self.preview_sizes.iter().all(|(name, size)| {
                !name.is_empty()
                    && name.len() <= 96
                    && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
                    && size.iter().all(|value| value.is_finite() && *value > 0.0 && *value <= 1_000_000.0)
            })
            // The presenter bounds requested roots; the callback bounds serialized input bytes.
            && self.hud_geometry.iter().all(|(name, geometry)| {
                !name.is_empty()
                    && name.len() <= 256
                    && geometry
                        .rect
                        .iter()
                        .chain(&geometry.size)
                        .all(|value| value.is_finite())
                    && geometry.rect[2..]
                        .iter()
                        .chain(&geometry.size)
                        .all(|value| *value > 0.0)
            })
            && self.events.iter().all(|event| {
                !event.id.is_empty()
                    && event.id.len() <= 256
                    && event
                        .id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_-.".contains(&c))
                    && event.value.is_finite()
            })
            && self.keys.iter().all(Key::is_valid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_preview_measurements_require_their_own_frozen_input_group() {
        let input = Input {
            preview_sizes: BTreeMap::from([("example".into(), [48.0, 28.0])]),
            ..Default::default()
        };
        assert!(input.is_valid());
        let legacy =
            serde_json::to_value(input.with_extensions(&InputExtensions::default())).unwrap();
        assert!(legacy.get("preview_sizes").is_none());
        let opted = serde_json::to_value(
            input.with_extensions(&InputExtensions(vec![InputExtension::ControlPreviews])),
        )
        .unwrap();
        assert_eq!(
            opted["preview_sizes"]["example"],
            serde_json::json!([48.0, 28.0])
        );
        let mut invalid = input;
        invalid
            .preview_sizes
            .insert("bad/name".into(), [48.0, 28.0]);
        assert!(!invalid.is_valid());
    }

    #[test]
    fn legacy_wire_never_receives_unaccepted_extension_fields() {
        let input = Input {
            pointer_held: true,
            pointer_cancelled: true,
            shift_held: true,
            ..Default::default()
        };
        let legacy =
            serde_json::to_value(input.with_extensions(&InputExtensions::default())).unwrap();
        assert_eq!(
            legacy,
            serde_json::json!({"version":1,"open":false,"events":[],"keys":[]})
        );
        let extensions = InputExtensions(vec![
            InputExtension::PointerState,
            InputExtension::HudGeometry,
        ]);
        assert!(extensions.validate().is_ok());
        let extended = serde_json::to_value(input.with_extensions(&extensions)).unwrap();
        assert_eq!(extended["pointer_held"], true);
        assert_eq!(extended["pointer_cancelled"], true);
        assert_eq!(extended["hud_geometry"], serde_json::json!({}));
        assert!(extended.get("shift_held").is_none());
        let shifted = serde_json::to_value(
            input.with_extensions(&InputExtensions(vec![InputExtension::ShiftState])),
        )
        .unwrap();
        assert_eq!(shifted["shift_held"], true);
        assert!(shifted.get("pointer_held").is_none());
        assert!(
            InputExtensions(vec![
                InputExtension::PointerState,
                InputExtension::PointerState
            ])
            .validate()
            .is_err()
        );
        assert!(serde_json::from_str::<InputExtensions>(r#"["future_unknown"]"#).is_err());
    }

    #[test]
    fn pointer_cancellation_and_shift_snapshot_default_for_older_input_json() {
        let input: Input =
            serde_json::from_str(r#"{"version":1,"open":true,"events":[],"keys":[]}"#).unwrap();
        assert!(!input.pointer_held && !input.pointer_cancelled && !input.shift_held);
        let input = Input {
            pointer_cancelled: true,
            shift_held: true,
            ..Default::default()
        };
        let encoded = serde_json::to_value(&input).unwrap();
        assert_eq!(encoded["pointer_cancelled"], true);
        assert_eq!(encoded["shift_held"], true);
        assert!(input.is_valid());
    }

    #[test]
    fn geometry_reports_do_not_share_the_unrelated_key_subscription_limit() {
        let mut input = Input::default();
        for index in 0..128 {
            input.hud_geometry.insert(
                format!("example.root{index}"),
                HudGeometry {
                    rect: [0.0, 0.0, 80.0, 27.0],
                    size: [80.0, 27.0],
                },
            );
        }
        assert!(input.is_valid());
        assert!(serde_json::to_vec(&input).unwrap().len() < MAX_INPUT_BYTES);
        input.hud_geometry.get_mut("example.root0").unwrap().rect[0] = f32::NAN;
        assert!(!input.is_valid());
    }
}
