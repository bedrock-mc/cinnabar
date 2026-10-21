//! Projects the trusted packet guest's retained menu through the shared native panel owner.

use super::packets::PacketMods;
use client_shell::ui_resources::UiPresentationResource;
use mod_api::native_menu::Input;
use mod_ui::PanelUiOwner;
use std::sync::Arc;

/// Input is collected once, then exchanged without blocking the next frame's physical router.
#[derive(Default)]
pub(super) struct State {
    pub owned: bool,
    relay: Option<u64>,
    session: u64,
    pub pending: Input,
    prepared: Option<Arc<mod_ui::PreparedPanelUi>>,
    pub retained: Option<Arc<client_extensions::modding::packet_relay::MenuOutput>>,
}

#[cfg(test)]
mod runtime_scenario;

/// Applies each guest update once; unchanged polling never rebuilds a catalog or panel.
pub(super) fn poll(
    packets: &mut PacketMods,
    presentation: &mut UiPresentationResource,
    session: u64,
    viewport: Option<[f32; 2]>,
) {
    let relay = packets.native_menu_identity();
    if packets.menu.session != session || packets.menu.relay != relay {
        let owner = PanelUiOwner::PacketSession(packets.menu.relay.unwrap_or(0));
        let _ = presentation.mod_ui_mut().set_mod_panel(owner, None);
        presentation.mod_ui_mut().set_prepared_panel_ui(owner, None);
        let _ = presentation.set_mod_hud_bindings(owner, None);
        packets.menu = State {
            session,
            relay,
            ..Default::default()
        };
    }
    if relay.is_none() {
        return;
    }
    let mut input = std::mem::take(&mut packets.menu.pending);
    input.open = packets.menu.owned && presentation.mod_ui().mod_panel_open();
    input.viewport = viewport;
    input.preview_sizes = relay
        .map(|relay| {
            presentation.mod_control_preview_sizes(PanelUiOwner::PacketSession(relay), session)
        })
        .unwrap_or_default();
    if let Some(requests) = packets
        .menu
        .retained
        .as_ref()
        .and_then(|output| output.output.hud_geometry.as_ref())
    {
        input.hud_geometry = presentation
            .mod_hud_geometry(requests, session)
            .into_iter()
            .map(|(name, geometry)| {
                let ui::mod_hud::HudGeometry { rect, size } = geometry;
                (name, mod_api::native_menu::HudGeometry { rect, size })
            })
            .collect();
    }
    if let Some(output) = packets.native_menu(input) {
        packets.menu.publish(presentation, output);
    }
    if packets.menu.owned && !presentation.mod_ui().mod_panel_painting() {
        packets.menu.owned = false;
    }
}

/// Reopens the last validated panel locally; the next input informs the guest of visibility.
pub(super) fn toggle(packets: &mut PacketMods, presentation: &mut UiPresentationResource) {
    let Some(retained) = packets.menu.retained.clone() else {
        return;
    };
    let open = !(packets.menu.owned && presentation.mod_ui().mod_panel_open());
    if packets.menu.install(presentation, &retained).is_ok() {
        presentation.mod_ui_mut().set_mod_panel_open(open);
        packets.menu.owned = true;
    }
}

impl State {
    /// Closed background updates retain routes and key subscriptions without taking another UI.
    fn publish(
        &mut self,
        presentation: &mut UiPresentationResource,
        mut output: Arc<client_extensions::modding::packet_relay::MenuOutput>,
    ) {
        if output.output.panel.is_none()
            && let Some(previous) = &self.retained
        {
            let mut merged = (*output).clone();
            merged.output.panel.clone_from(&previous.output.panel);
            merged.prepared.clone_from(&previous.prepared);
            output = Arc::new(merged);
        }
        if let Some(bindings) = output.output.hud_bindings.as_ref()
            && let Some(relay) = self.relay
            && let Err(error) = presentation
                .set_mod_hud_bindings(PanelUiOwner::PacketSession(relay), Some(bindings.clone()))
        {
            bevy::log::warn!(%error, "Packet HUD bindings rejected");
            return;
        }
        if output.output.open && output.output.panel.is_some() || self.owned {
            if output.output.open
                && output.output.panel.is_some()
                && let Err(error) = self.install(presentation, &output)
            {
                bevy::log::warn!(%error, "Packet menu presentation rejected");
                if self.owned {
                    presentation.mod_ui_mut().set_mod_panel_open(false);
                }
                self.owned = false;
                return;
            }
            presentation
                .mod_ui_mut()
                .set_mod_panel_open(output.output.open);
            self.owned = true;
        }
        self.retained = Some(output);
    }

    /// Publishes a prepared catalog only when this session actually takes panel ownership.
    fn install(
        &mut self,
        presentation: &mut UiPresentationResource,
        output: &Arc<client_extensions::modding::packet_relay::MenuOutput>,
    ) -> Result<(), String> {
        let relay = self.relay.ok_or("packet menu has no live owner")?;
        let owner = PanelUiOwner::PacketSession(relay);
        let prepared = output
            .prepared
            .as_ref()
            .ok_or("packet menu has no prepared panel")?;
        presentation.mod_ui_mut().set_prepared_mod_panel(
            owner,
            Arc::clone(prepared),
            output
                .output
                .panel
                .as_ref()
                .ok_or("packet menu has no panel")?,
        )?;
        self.prepared = Some(Arc::clone(prepared));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prepares a real retained panel without starting an executable or network session.
    fn output(open: bool) -> Arc<client_extensions::modding::packet_relay::MenuOutput> {
        let mut panel = mod_ui::test_support::sample_panel();
        panel.toggle_key = "F8".into();
        let prepared = mod_ui::PreparedPanelUi::prepare(Arc::new(ui::mod_panel::PanelBundle {
            panel: panel.clone(),
            surfaces: Default::default(),
            control_schemas: Default::default(),
        }))
        .unwrap();
        Arc::new(client_extensions::modding::packet_relay::MenuOutput {
            output: mod_host::packet_mod::native_menu::Output {
                open,
                panel: Some(panel),
                hud_bindings: None,
                hud_geometry: None,
                input_extensions: None,
                keys: Vec::new(),
            },
            prepared: Some(prepared),
        })
    }

    #[test]
    fn closed_background_updates_preserve_another_owners_open_panel() {
        let mut presentation =
            UiPresentationResource(client_ui::test_support::mini_engine_presentation());
        let mut regular = mod_ui::test_support::sample_panel();
        regular.toggle_key = "F10".into();
        presentation
            .mod_ui_mut()
            .set_mod_panel(PanelUiOwner::Environment(0), Some(&regular))
            .unwrap();
        presentation.mod_ui_mut().set_mod_panel_open(true);
        let mut state = State {
            relay: Some(1),
            ..Default::default()
        };
        state.publish(&mut presentation, output(false));
        assert!(presentation.mod_ui().mod_panel_open());
        assert_eq!(presentation.mod_ui().mod_panel_toggle_key(), Some("F10"));
        assert!(!state.owned);
        state.publish(&mut presentation, output(true));
        assert!(state.owned && presentation.mod_ui().mod_panel_open());
        assert_eq!(presentation.mod_ui().mod_panel_toggle_key(), Some("F8"));
    }

    #[test]
    fn stopped_closed_sessions_release_their_registered_catalogs() {
        let mut presentation =
            UiPresentationResource(client_ui::test_support::mini_engine_presentation());
        let output = output(false);
        let retired = Arc::downgrade(output.prepared.as_ref().unwrap());
        presentation.mod_ui_mut().set_prepared_panel_ui(
            PanelUiOwner::PacketSession(7),
            Some(Arc::clone(output.prepared.as_ref().unwrap())),
        );
        let mut packets = PacketMods::default();
        packets.menu = State {
            session: 10,
            relay: Some(7),
            prepared: Some(Arc::clone(output.prepared.as_ref().unwrap())),
            retained: Some(output),
            ..Default::default()
        };
        poll(&mut packets, &mut presentation, 10, None);
        assert!(retired.upgrade().is_none());
        assert!(packets.menu.retained.is_none());
    }

    #[test]
    fn stopped_packet_session_retires_actual_hud_globals_and_preserves_other_owners() {
        use ui::mod_hud::{HudBindings, HudGeometryRequests};
        let mut presentation =
            UiPresentationResource(client_ui::test_support::mini_engine_presentation());
        presentation.set_server_ui_pack(&client_ui::ui_runtime::presentation::ServerUiPack {
            ui_layers: vec![vec![("ui/_ui_defs.json".into(), br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec()),
                ("ui/hud_screen.json".into(), serde_json::json!({"namespace":"hud","hud_screen":{
                    "type":"screen","controls":[{"fixture_root":{"type":"input_panel","size":[40,20],
                        "anchor_from":"top_left","anchor_to":"top_left","bindings":[{
                            "binding_name":"#mod_fixture_position","binding_name_override":"#offset"}]}}]
                }}).to_string().into_bytes())]], ..Default::default()
        });
        let globals = |x| {
            serde_json::from_value::<HudBindings>(
                serde_json::json!({"#mod_fixture_position":[x,20]}),
            )
            .unwrap()
        };
        let root = |presentation: &mut UiPresentationResource| {
            presentation
                .build(
                    &player_state::PlayerState::new(1),
                    &client_ui::ui_runtime::UiRuntime::new(1),
                    0,
                    [1280, 800],
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            presentation.mod_hud_geometry(&HudGeometryRequests(vec!["fixture_root".into()]),1)["fixture_root"].clone()
        };
        presentation
            .set_mod_hud_bindings(PanelUiOwner::Environment(1), Some(globals(10)))
            .unwrap();
        let other_owner = root(&mut presentation);
        presentation
            .set_mod_hud_bindings(PanelUiOwner::PacketSession(7), Some(globals(100)))
            .unwrap();
        assert!(root(&mut presentation).rect[0] > other_owner.rect[0]);
        let mut packets = PacketMods::default();
        packets.menu = State {
            session: 1,
            relay: Some(7),
            ..Default::default()
        };
        poll(&mut packets, &mut presentation, 1, None);
        assert_eq!(root(&mut presentation), other_owner);
    }

    /// Uses the same worker-prepared catalog publication as the live packet menu.
    fn moving_output(
        screen: &str,
        target: f64,
        recolor: bool,
        open: bool,
    ) -> Arc<client_extensions::modding::packet_relay::MenuOutput> {
        use ui::mod_panel::{Surface, SurfaceTransition, SurfaceValue};

        let mut panel = mod_ui::test_support::sample_panel();
        panel.motion = true;
        panel.surface = Some(Surface {
            collections: Default::default(),
            previews: Default::default(),
            screen: format!("temporal.{screen}"),
            document: serde_json::json!({"namespace":"temporal",screen:{
                "type":"screen","size":["100%","100%"],"controls":[{"moving":{
                    "type":"button","size":[40,20],"offset":[0,40],
                    "anchor_from":"top_left","anchor_to":"top_left",
                    "bindings":[{"binding_name":"#position","binding_name_override":"#offset"}],
                    "button_mappings":[{"from_button_id":"button.menu_select",
                        "to_button_id":"mod.control:0","mapping_type":"pressed"}],
                    "controls":[{"art":{"type":"custom",
                        "renderer":"cinnabar_rounded_rectangle","radius":0,
                        "size":["100%","100%"],
                        "color":if recolor {[0,1,0,1]} else {[1,0,0,1]}}}]
                }}]
            }})
            .to_string(),
            bindings: std::collections::BTreeMap::from([(
                "#position".into(),
                SurfaceValue::Vector(vec![target, 40.0]),
            )]),
            transitions: std::collections::BTreeMap::from([(
                "#position".into(),
                SurfaceTransition {
                    duration: 0.25,
                    easing: Default::default(),
                    from: Some(SurfaceValue::Vector(vec![0.0, 40.0])),
                    exit: Some(SurfaceValue::Vector(vec![0.0, 40.0])),
                },
            )]),
        });
        let prepared = mod_ui::PreparedPanelUi::prepare(Arc::new(ui::mod_panel::PanelBundle {
            panel: panel.clone(),
            surfaces: Default::default(),
            control_schemas: Default::default(),
        }))
        .unwrap();
        Arc::new(client_extensions::modding::packet_relay::MenuOutput {
            output: mod_host::packet_mod::native_menu::Output {
                open,
                panel: Some(panel),
                hud_bindings: None,
                hud_geometry: None,
                input_extensions: None,
                keys: Vec::new(),
            },
            prepared: Some(prepared),
        })
    }

    /// Advances the production presentation clock and reads the painted button geometry.
    fn moving_x(presentation: &mut UiPresentationResource, millis: u64) -> f64 {
        presentation
            .build(
                &player_state::PlayerState::new(1),
                &client_ui::ui_runtime::UiRuntime::new(1),
                millis,
                [1280, 800],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        presentation
            .mod_ui()
            .mod_panel_state()
            .unwrap()
            .frame
            .unwrap()
            .hits
            .iter()
            .find(|hit| hit.pressed.as_deref() == Some("mod.control:0"))
            .unwrap()
            .rect
            .x
    }

    #[test]
    fn packet_publication_preserves_live_tweens_and_plays_route_and_visibility_transitions() {
        let mut presentation =
            UiPresentationResource(client_ui::test_support::mini_engine_presentation());
        let mut state = State {
            relay: Some(1),
            ..Default::default()
        };
        state.publish(
            &mut presentation,
            moving_output("first", 100.0, false, true),
        );
        let origin = moving_x(&mut presentation, 1_000);
        assert_eq!(moving_x(&mut presentation, 1_125) - origin, 50.0);
        state.publish(&mut presentation, moving_output("first", 200.0, true, true));
        assert_eq!(moving_x(&mut presentation, 1_125) - origin, 50.0);
        assert_eq!(moving_x(&mut presentation, 1_250) - origin, 125.0);
        state.publish(&mut presentation, moving_output("first", 0.0, false, true));
        assert_eq!(moving_x(&mut presentation, 1_250) - origin, 125.0);
        assert_eq!(moving_x(&mut presentation, 1_375) - origin, 62.5);

        state.publish(&mut presentation, moving_output("next", 200.0, true, true));
        assert_eq!(moving_x(&mut presentation, 1_500) - origin, 0.0);
        assert_eq!(moving_x(&mut presentation, 1_625) - origin, 100.0);
        assert_eq!(moving_x(&mut presentation, 1_750) - origin, 200.0);
        // Closing may carry a future route; the currently displayed surface exits first.
        state.publish(
            &mut presentation,
            moving_output("future", 100.0, false, false),
        );
        assert!(!presentation.mod_ui().mod_panel_open());
        assert_eq!(moving_x(&mut presentation, 1_750) - origin, 200.0);
        assert_eq!(moving_x(&mut presentation, 1_875) - origin, 100.0);
        state.publish(&mut presentation, moving_output("next", 200.0, true, true));
        assert_eq!(moving_x(&mut presentation, 1_875) - origin, 100.0);
        assert_eq!(moving_x(&mut presentation, 2_000) - origin, 150.0);
        state.publish(&mut presentation, moving_output("next", 200.0, true, false));
        moving_x(&mut presentation, 2_000);
        presentation
            .build(
                &player_state::PlayerState::new(1),
                &client_ui::ui_runtime::UiRuntime::new(1),
                2_250,
                [1280, 800],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        assert!(!presentation.mod_ui().mod_panel_painting());
    }
}
