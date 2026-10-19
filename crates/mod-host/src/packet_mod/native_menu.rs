//! Retained panel output validation for the optional native packet-menu callback.

use super::PacketMod;
use anyhow::{Result, ensure};
use mod_api::native_menu::{Input, MAX_INPUT_BYTES, MAX_OUTPUT_BYTES};
use serde::{Deserialize, Serialize};

/// Retained presentation updates; omitted panel or HUD bindings keep their previous values.
/// JSON null leaves the entire presentation unchanged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub open: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<ui::mod_panel::Panel>,
    /// Bounded, trusted globals for the actual resource-pack HUD, independent of menu visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hud_bindings: Option<ui::mod_hud::HudBindings>,
    /// Opaque inert gameplay HUD roots requested independently of menu visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hud_geometry: Option<ui::mod_hud::HudGeometryRequests>,
    /// Version-one input groups accepted on subsequent calls; omission retains the selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_extensions: Option<mod_api::native_menu::InputExtensions>,
    /// Physical keys reserved for guest-owned module actions while gameplay owns input.
    #[serde(default)]
    pub keys: Vec<String>,
}

impl PacketMod {
    /// Whether a live guest negotiated the explicitly granted native controls callback.
    pub fn has_native_menu(&self) -> bool {
        self.is_active() && self.guest.has_native_menu()
    }

    /// Calls the guest once and validates every returned panel before publication.
    pub fn menu(&mut self, input: &Input) -> Result<Option<Output>> {
        if !self.has_native_menu() {
            return Ok(None);
        }
        ensure!(
            input.is_valid()
                && input
                    .events
                    .iter()
                    .all(|event| event.id.len() <= ui::mod_panel::MAX_PANEL_ID_BYTES),
            "invalid native menu input"
        );
        let json = serde_json::to_vec(&input.with_extensions(&self.menu_input_extensions))?;
        ensure!(
            json.len() <= MAX_INPUT_BYTES,
            "native menu input exceeds its byte limit"
        );
        let deadline = self.call_deadline;
        let output = self
            .call("menu", deadline, |guest| {
                let bytes = match guest.menu(&json)? {
                    Ok(Some(bytes)) => bytes,
                    Ok(None) => return Ok(Ok(None)),
                    Err(error) => return Ok(Err(error)),
                };
                ensure!(
                    bytes.len() <= MAX_OUTPUT_BYTES,
                    "native menu output exceeds its byte limit"
                );
                let output: Option<Output> = serde_json::from_slice(&bytes)?;
                if let Some(output) = &output {
                    if let Some(panel) = &output.panel {
                        panel.validate().map_err(anyhow::Error::msg)?;
                        ensure!(
                            !panel.controls.iter().any(|control| matches!(
                                control,
                                ui::mod_panel::Control::Text { .. }
                            )),
                            "native menu text controls are not supported"
                        );
                    }
                    if let Some(bindings) = &output.hud_bindings {
                        bindings.validate().map_err(anyhow::Error::msg)?;
                    }
                    if let Some(requests) = &output.hud_geometry {
                        requests.validate().map_err(anyhow::Error::msg)?;
                    }
                    if let Some(extensions) = &output.input_extensions {
                        extensions.validate().map_err(anyhow::Error::msg)?;
                    }
                    ensure!(
                        output
                            .panel
                            .as_ref()
                            .is_none_or(|panel| mod_api::control_key_valid(&panel.toggle_key))
                            && output.keys.len() <= mod_api::MAX_CONTROL_KEYS
                            && output
                                .keys
                                .iter()
                                .all(|key| mod_api::control_key_valid(key)),
                        "invalid native menu key subscriptions"
                    );
                }
                Ok(Ok(output))
            })
            .map(Option::flatten)?;
        if let Some(extensions) = output
            .as_ref()
            .and_then(|output| output.input_extensions.as_ref())
        {
            self.menu_input_extensions.clone_from(extensions);
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModGrants, packet_mod::CompiledPacketMod};

    /// Appends the optional menu ABI to the ordinary packet-relay fixture.
    fn module(output: &str, length: usize) -> Vec<u8> {
        let mut source = include_str!("tests/guest.wat")
            .replace("$IMPORTS", "")
            .replace("$START", "");
        source = source.trim_end().strip_suffix(')').unwrap().to_owned();
        let output: String = output.bytes().map(|byte| format!("\\{byte:02x}")).collect();
        source.push_str(&format!(
            r#"
            (data (i32.const 2048) "{output}")
            (func (export "native-init") (param i32)
                (i32.store (i32.const 2000) (local.get 0)))
            (func (export "menu") (param i32 i32) (result i32)
                (if (i32.ne (i32.load (i32.const 2000)) (i32.const 2)) (then unreachable))
                (i32.store (i32.const 128) (i32.const 0))
                (i32.store (i32.const 132) (i32.const 2048))
                (i32.store (i32.const 136) (i32.const {length}))
                (i32.const 128)))
        "#
        ));
        wat::parse_str(source).unwrap()
    }

    /// Only the controls grant activates the menu export, independently of camera support.
    fn instance(output: &str, length: usize, controls: bool) -> PacketMod {
        CompiledPacketMod::compile(
            &module(output, length),
            &ModGrants {
                packets: true,
                controls,
                ..Default::default()
            },
        )
        .unwrap()
        .instantiate()
        .unwrap()
    }

    /// A minimal authored packet menu using the production panel schema.
    fn output() -> String {
        serde_json::json!({"open":true,"keys":["KeyR"],"panel":{
            "title":"Menu","toggle_key":"ShiftRight","dark":true,
            "controls":[{"kind":"button","id":"action","label":"Action"}],
            "surface":{"screen":"fixture.root","document":serde_json::json!({
                "namespace":"fixture","root":{"type":"panel","controls":[{"action":{
                    "type":"button","size":[60,20],"button_mappings":[{
                        "from_button_id":"button.menu_select","mapping_type":"pressed",
                        "to_button_id":"mod.control:0"}]
                }}]}}).to_string()}
        }})
        .to_string()
    }

    #[test]
    fn menu_only_reactors_negotiate_controls_and_retain_valid_panels() {
        let output = output();
        let mut guest = instance(&output, output.len(), true);
        assert!(guest.has_native_menu());
        assert!(!guest.has_native_frame());
        let menu = guest.menu(&Input::default()).unwrap().unwrap();
        assert!(menu.open);
        assert_eq!(menu.keys, ["KeyR"]);
        assert_eq!(menu.panel.as_ref().unwrap().controls[0].id(), "action");
        let mut denied = instance(&output, output.len(), false);
        assert!(!denied.has_native_menu());
        assert_eq!(denied.menu(&Input::default()).unwrap(), None);
        assert!(denied.is_active());
    }

    #[test]
    fn text_controls_are_rejected_before_publication() {
        let mut output: serde_json::Value = serde_json::from_str(&output()).unwrap();
        output["panel"]["controls"][0] = serde_json::json!({
            "kind":"text","id":"name","label":"Name","value":"Main",
            "min_chars":1,"max_chars":8
        });
        let panel: ui::mod_panel::Panel = serde_json::from_value(output["panel"].clone()).unwrap();
        panel.validate().unwrap();
        let output = output.to_string();
        let mut guest = instance(&output, output.len(), true);
        let error = guest.menu(&Input::default()).unwrap_err();
        assert!(format!("{error:#}").contains("text controls"), "{error:#}");
        assert!(!guest.is_active());
    }

    #[test]
    fn unchanged_menus_return_null_without_clearing_the_guest() {
        let mut guest = instance("null", 4, true);
        assert_eq!(guest.menu(&Input::default()).unwrap(), None);
        assert!(guest.is_active());
    }

    #[test]
    fn extended_input_is_opted_in_retained_and_explicitly_cleared_per_instance() {
        let legacy = r#"{"version":1,"open":false,"events":[],"keys":[]}"#;
        let extended = r#"{"version":1,"open":false,"events":[],"keys":[],"pointer_held":true,"pointer_cancelled":true,"shift_held":true,"hud_geometry":{}}"#;
        let outputs = [
            r#"{"open":false,"input_extensions":["pointer_state","shift_state","hud_geometry"]}"#,
            "null",
            r#"{"open":false,"input_extensions":[]}"#,
            r#"{"open":false}"#,
        ];
        let mut source = include_str!("tests/guest.wat")
            .replace("$IMPORTS", "")
            .replace("$START", "");
        source = source.trim_end().strip_suffix(')').unwrap().to_owned();
        source.push_str("(global $calls (mut i32) (i32.const 0)) (func (export \"native-init\") (param i32) (if (i32.ne (local.get 0) (i32.const 2)) (then unreachable)))");
        for (index, output) in outputs.iter().enumerate() {
            let data: String = output.bytes().map(|byte| format!("\\{byte:02x}")).collect();
            source.push_str(&format!(
                "(data (i32.const {}) \"{data}\")",
                4096 + index * 1024
            ));
        }
        source.push_str("(func (export \"menu\") (param i32 i32) (result i32)");
        for (index, output) in outputs.iter().enumerate() {
            let input_length = if index == 0 || index == 3 {
                legacy.len()
            } else {
                extended.len()
            };
            source.push_str(&format!(
                "(if (i32.eq (global.get $calls) (i32.const {index})) (then
                (if (i32.ne (local.get 1) (i32.const {input_length})) (then unreachable))
                (global.set $calls (i32.add (global.get $calls) (i32.const 1)))
                (i32.store (i32.const 128) (i32.const 0))
                (i32.store (i32.const 132) (i32.const {}))
                (i32.store (i32.const 136) (i32.const {}))
                (return (i32.const 128))))",
                4096 + index * 1024,
                output.len()
            ));
        }
        source.push_str("unreachable))");
        let compiled = CompiledPacketMod::compile(
            &wat::parse_str(source).unwrap(),
            &ModGrants {
                packets: true,
                controls: true,
                ..Default::default()
            },
        )
        .unwrap();
        let input = Input {
            pointer_held: true,
            pointer_cancelled: true,
            shift_held: true,
            ..Default::default()
        };
        for _ in 0..2 {
            let mut guest = compiled.instantiate().unwrap();
            for output in outputs {
                assert_eq!(guest.menu(&input).unwrap().is_none(), output == "null");
            }
            assert!(guest.is_active());
        }
    }

    #[test]
    fn unknown_or_duplicate_input_extension_requests_quarantine_the_guest() {
        for extensions in [
            serde_json::json!(["pointer_state", "pointer_state"]),
            serde_json::json!(["unknown_input"]),
        ] {
            let output =
                serde_json::json!({"open":false,"input_extensions":extensions}).to_string();
            let mut guest = instance(&output, output.len(), true);
            assert!(guest.menu(&Input::default()).is_err());
            assert!(!guest.is_active());
        }
    }

    #[test]
    fn hud_bindings_can_update_before_a_panel_and_reject_untrusted_globals() {
        let output = serde_json::json!({"open":false,"hud_bindings":{
            "#mod_fixture_position":[0.25,0.75],"#mod_fixture_scale":1.2,
            "#mod_fixture_active":true
        }})
        .to_string();
        let mut guest = instance(&output, output.len(), true);
        let update = guest.menu(&Input::default()).unwrap().unwrap();
        assert!(!update.open && update.panel.is_none());
        assert!(update.hud_bindings.is_some());
        for invalid in [
            output.replace("#mod_fixture_scale", "#world_blur"),
            output.replace("1.2", "\"untyped\""),
        ] {
            let mut guest = instance(&invalid, invalid.len(), true);
            assert!(guest.menu(&Input::default()).is_err());
            assert!(!guest.is_active());
        }
    }

    #[test]
    fn named_hud_geometry_requests_are_bounded_and_independent_of_menu_visibility() {
        let output = serde_json::json!({"open":false,"hud_geometry":["fixture.clock"]}).to_string();
        let mut guest = instance(&output, output.len(), true);
        let update = guest.menu(&Input::default()).unwrap().unwrap();
        assert!(update.panel.is_none());
        assert_eq!(update.hud_geometry.unwrap().0, ["fixture.clock"]);
        for names in [
            serde_json::json!(["duplicate", "duplicate"]),
            serde_json::json!(["bad/name"]),
        ] {
            let output = serde_json::json!({"open":false,"hud_geometry":names}).to_string();
            let mut guest = instance(&output, output.len(), true);
            assert!(guest.menu(&Input::default()).is_err());
            assert!(!guest.is_active());
        }
    }

    #[test]
    fn invalid_panel_actions_and_output_sizes_quarantine_the_guest() {
        let invalid = output().replace("mod.control:0", "mod.control:1");
        let mut guest = instance(&invalid, invalid.len(), true);
        assert!(guest.menu(&Input::default()).is_err());
        assert!(!guest.is_active());
        let mut guest = instance("null", MAX_OUTPUT_BYTES + 1, true);
        assert!(guest.menu(&Input::default()).is_err());
        assert!(!guest.is_active());
        let invalid = output().replace("KeyR", "not a physical key");
        let mut guest = instance(&invalid, invalid.len(), true);
        assert!(guest.menu(&Input::default()).is_err());
        assert!(!guest.is_active());
    }
    #[test]
    fn real_go_reactor_opens_edits_and_closes_a_native_menu_with_only_controls_granted() {
        let (Some(wasm), Some(capture)) = (
            std::env::var_os("LUNAR_NATIVE_WASM"),
            std::env::var_os("LUNAR_NATIVE_CAPTURE"),
        ) else {
            eprintln!(
                "missing fixture: LUNAR_NATIVE_WASM and LUNAR_NATIVE_CAPTURE; skipping real Go native menu integration"
            );
            return;
        };
        let records = crate::packet_mod::capture::parse(
            &std::fs::read(capture).expect("read synthetic packet capture"),
        )
        .unwrap();
        let (identity, client_data, startup) = records
            .iter()
            .find_map(|record| match record {
                crate::packet_mod::capture::Record::Start {
                    identity,
                    client_data,
                    startup,
                } => Some((identity, client_data, startup)),
                _ => None,
            })
            .expect("capture needs a start record");
        let mut guest = CompiledPacketMod::load(
            std::path::Path::new(&wasm),
            &ModGrants {
                packets: true,
                wasi: true,
                controls: true,
                ..Default::default()
            },
        )
        .unwrap()
        .instantiate()
        .unwrap();
        assert!(guest.has_native_menu());
        assert!(
            !guest.has_native_frame(),
            "camera support is independently denied"
        );
        guest.start(identity, client_data, startup).unwrap();
        if let Some(initial) = guest.menu(&Input::default()).unwrap() {
            assert!(!initial.open);
        }
        let mut input = Input {
            open: true,
            ..Default::default()
        };
        let menu = guest
            .menu(&input)
            .unwrap()
            .expect("opening produces a native panel");
        assert!(menu.open);
        let modules = menu
            .panel
            .as_ref()
            .unwrap()
            .controls
            .iter()
            .find(|control| control.label() == "Modules")
            .unwrap()
            .id()
            .to_owned();
        input.events.push(mod_api::native_menu::Event {
            id: modules,
            value: 1.0,
        });
        let modules = guest.menu(&input).unwrap().unwrap();
        assert!(modules.open);
        assert!(
            modules
                .panel
                .as_ref()
                .unwrap()
                .surface
                .as_ref()
                .unwrap()
                .document
                .contains("cinnabar_glass")
        );
        input.events.clear();
        input.keys.push(mod_api::native_menu::Key {
            key: "Escape".into(),
            text: None,
        });
        let _ = guest.menu(&input).unwrap();
        let closed = guest
            .menu(&input)
            .unwrap()
            .expect("second Escape closes the launcher");
        assert!(!closed.open);
        if let Some(command) = std::env::var_os("LUNAR_NATIVE_OPEN_COMMAND") {
            let command = std::fs::read(command).expect("read encoded native menu command fixture");
            guest
                .on_batch(crate::packet_mod::Direction::FromClient, &command)
                .unwrap();
            let reopened = guest
                .menu(&Input::default())
                .unwrap()
                .expect("/.ui opens the native override");
            assert!(reopened.open);
        }
        assert!(guest.is_active());
        guest.stop();
        assert!(!guest.has_native_menu());
    }
}
