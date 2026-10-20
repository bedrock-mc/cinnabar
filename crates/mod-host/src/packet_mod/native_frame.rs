//! Bounded native callback validation, shared by the packet worker's client adapter.

use super::PacketMod;
use anyhow::{Result, ensure};
use mod_api::native_camera::Effects;
use mod_api::native_frame::{Delta, Frame, MAX_BYTES};

impl PacketMod {
    /// Whether this instance negotiated the native player-camera frame capability.
    pub fn has_native_frame(&self) -> bool {
        self.is_active()
            && (self.guest.has_native_frame()
                || self.guest.has_native_camera()
                || self.guest.has_native_render())
    }

    /// Runs one coalesced native frame and validates its once-consumed local output.
    pub fn frame(&mut self, frame: Option<&Frame>) -> Result<Option<Effects>> {
        if !self.has_native_frame() {
            return Ok(None);
        }
        ensure!(frame.is_none_or(Frame::is_valid), "invalid native frame");
        let mut observation = frame.cloned();
        let general = self.guest.has_native_camera();
        if !general && observation.as_ref().is_some_and(|frame| frame.suspended) {
            observation = None;
        }
        if !self.guest.native_camera_players()
            && let Some(frame) = observation.as_mut()
        {
            frame.players.clear();
        }
        let json = serde_json::to_vec(&observation)?;
        ensure!(
            json.len() <= MAX_BYTES,
            "native frame exceeds its byte limit"
        );
        let deadline = self.call_deadline;
        self.call("frame", deadline, |guest| {
            let general = guest.has_native_camera();
            let output = if general {
                guest.camera_frame(&json)?
            } else {
                guest.frame(&json)?
            };
            let output = match output {
                Ok(Some(output)) => output,
                Ok(None) => return Ok(Ok(None)),
                Err(error) => return Ok(Err(error)),
            };
            ensure!(
                output.len() <= MAX_BYTES,
                "native frame returned too many bytes"
            );
            let effects: Option<Effects> = if general {
                serde_json::from_slice(&output)?
            } else {
                let delta: Option<Delta> = serde_json::from_slice(&output)?;
                delta.map(|rotation| Effects {
                    rotation: Some(rotation),
                    ..Default::default()
                })
            };
            ensure!(
                effects.as_ref().is_none_or(Effects::is_valid),
                "invalid native camera effects"
            );
            ensure!(
                frame.is_none_or(|frame| !frame.suspended)
                    || effects
                        .as_ref()
                        .is_none_or(|effects| effects.rotation.is_none()
                            && effects.detached.is_none()
                            && effects.look.is_none()
                            && effects.movement.is_none()),
                "active camera effects without gameplay input"
            );
            ensure!(
                frame.is_some() || effects.is_none(),
                "camera output without a gameplay frame"
            );
            Ok(Ok(effects))
        })
        .map(Option::flatten)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModGrants, packet_mod::CompiledPacketMod};

    /// Optional local integration inputs remain outside the source tree and normal CI.
    fn integration_fixture(name: &str) -> Option<std::path::PathBuf> {
        let Some(path) = std::env::var_os(name).map(std::path::PathBuf::from) else {
            eprintln!("missing fixture: {name}; skipping real Go native reactor integration");
            return None;
        };
        if !path.is_absolute() || !path.is_file() {
            eprintln!(
                "missing fixture: {name} must name an available absolute file; skipping real Go native reactor integration"
            );
            return None;
        }
        Some(path)
    }

    /// Appends optional native exports to the existing canonical packet fixture.
    fn module_source(output: &str, length: usize) -> String {
        let mut source = include_str!("tests/guest.wat")
            .replace("$IMPORTS", "")
            .replace("$START", "");
        source = source.trim_end().strip_suffix(')').unwrap().to_owned();
        let output: String = output.bytes().map(|byte| format!("\\{byte:02x}")).collect();
        source.push_str(&format!(
            r#"
            (data (i32.const 2048) "{output}")
            (data (i32.const 4096) "null")
            (func (export "native-init") (param i32)
                (i32.store (i32.const 2000) (local.get 0)))
            (func (export "frame") (param $input i32) (param $length i32) (result i32)
                (if (i32.ne (i32.load (i32.const 2000)) (i32.const 1)) (then unreachable))
                (i32.store (i32.const 128) (i32.const 0))
                (i32.store (i32.const 132) (i32.const 2048))
                (i32.store (i32.const 136) (i32.const {length}))
                (if (i32.eq (i32.load8_u (local.get $input)) (i32.const 110)) (then
                    (i32.store (i32.const 132) (i32.const 4096))
                    (i32.store (i32.const 136) (i32.const 4))))
                (i32.const 128)))
        "#
        ));
        source
    }

    /// Compiles the established legacy callback fixture.
    fn module(output: &str, length: usize) -> Vec<u8> {
        wat::parse_str(module_source(output, length)).unwrap()
    }

    /// Creates a capable fixture without granting WASI or any unrelated API.
    fn instance(output: &str, length: usize, camera: bool) -> PacketMod {
        let grants = ModGrants {
            packets: true,
            players: true,
            camera,
            ..Default::default()
        };
        CompiledPacketMod::compile(&module(output, length), &grants)
            .unwrap()
            .instantiate()
            .unwrap()
    }

    /// Uses a finite frame without any selected target or feature behavior.
    fn frame() -> Frame {
        Frame {
            version: 1,
            session: 1,
            dimension: 0,
            eye: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            attack_held: false,
            seconds: 0.015625,
            players: Vec::new(),
            suspended: false,
        }
    }

    #[test]
    fn native_support_is_negotiated_and_null_resets_without_output() {
        let output = r#"{"yaw":0.125,"pitch":0.0}"#;
        let mut guest = instance(output, output.len(), true);
        assert!(guest.has_native_frame());
        assert_eq!(guest.frame(None).unwrap(), None);
        assert_eq!(
            guest.frame(Some(&frame())).unwrap(),
            Some(Effects {
                rotation: Some(Delta {
                    yaw: 0.125,
                    pitch: 0.0
                }),
                ..Default::default()
            })
        );
        assert!(!instance(output, output.len(), false).has_native_frame());
    }

    #[test]
    fn native_return_bounds_and_camera_validation_quarantine_the_guest() {
        for output in [
            r#"{"yaw":0.251,"pitch":0.0}"#,
            r#"{"yaw":0,"pitch":0,"extra":1}"#,
        ] {
            let mut guest = instance(output, output.len(), true);
            assert!(guest.frame(Some(&frame())).is_err());
            assert!(!guest.is_active());
        }
        let mut guest = instance("null", MAX_BYTES + 1, true);
        assert!(guest.frame(Some(&frame())).is_err());
        assert!(!guest.is_active());
    }

    #[test]
    fn general_camera_callback_negotiates_without_player_observation() {
        let output = r#"{"detached":{"kind":"orbit","distance":4.0},"hurt_intensity":0.0,"preserve_teleport_rotation":true}"#;
        let mut wat = module_source(output, output.len());
        wat = wat.replace("\"frame\"", "\"camera-frame\"");
        // The legacy fixture checks the init bit before returning any effect.
        wat = wat.replace(
            "(i32.const 1)) (then unreachable)",
            "(i32.const 68)) (then unreachable)",
        );
        // A camera-only guest must receive an empty player list even when the host has nearby players.
        wat = wat.replace("(i32.store (i32.const 128)", "(if (i32.ne (i32.load8_u (i32.add (local.get $input) (i32.sub (local.get $length) (i32.const 3)))) (i32.const 91)) (then (if (i32.ne (i32.load8_u (local.get $input)) (i32.const 110)) (then unreachable)))) (i32.store (i32.const 128)");
        let bytes = wat::parse_str(wat).unwrap();
        let grants = ModGrants {
            packets: true,
            camera: true,
            movement: true,
            ..Default::default()
        };
        let mut guest = CompiledPacketMod::compile(&bytes, &grants)
            .unwrap()
            .instantiate()
            .unwrap();
        assert!(guest.has_native_frame());
        let mut observation = frame();
        observation.players.push(mod_api::native_frame::Player {
            runtime_id: 77,
            feet: [1.0; 3],
        });
        let effects = guest.frame(Some(&observation)).unwrap().unwrap();
        assert_eq!(
            effects.detached,
            Some(mod_api::native_camera::DetachedView::Orbit { distance: 4.0 })
        );
        assert_eq!(effects.hurt_intensity, Some(0.0));
        assert!(effects.preserve_teleport_rotation);
        assert_eq!(guest.frame(None).unwrap(), None);
    }

    #[test]
    fn legacy_core_packet_mods_keep_their_existing_relay_abi() {
        let source = include_str!("tests/guest.wat")
            .replace("$IMPORTS", "")
            .replace("$START", "");
        let grants = ModGrants {
            packets: true,
            players: true,
            camera: true,
            ..Default::default()
        };
        let mut guest = CompiledPacketMod::compile(&wat::parse_str(source).unwrap(), &grants)
            .unwrap()
            .instantiate()
            .unwrap();
        assert!(!guest.has_native_frame());
        assert_eq!(guest.frame(Some(&frame())).unwrap(), None);
        assert!(guest.is_active());
    }

    #[test]
    fn real_go_reactor_negotiates_and_exchanges_native_frames() {
        let Some(wasm) = integration_fixture("PACKET_MOD_NATIVE_WASM") else {
            return;
        };
        let Some(capture) = integration_fixture("PACKET_MOD_NATIVE_CAPTURE") else {
            return;
        };
        let records = crate::packet_mod::capture::parse(
            &std::fs::read(capture).expect("read PACKET_MOD_NATIVE_CAPTURE fixture"),
        )
        .expect("parse PACKET_MOD_NATIVE_CAPTURE fixture");
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
            .expect("PACKET_MOD_NATIVE_CAPTURE fixture needs a start record");
        for (movement, drawing) in [(false, false), (true, false), (true, true)] {
            let grants = ModGrants {
                movement,
                world_draw: drawing,
                render_through_walls: drawing,
                packets: true,
                wasi: true,
                players: true,
                camera: true,
                ..Default::default()
            };
            let mut guest = CompiledPacketMod::load(&wasm, &grants)
                .expect("compile PACKET_MOD_NATIVE_WASM fixture")
                .instantiate()
                .expect("initialize real Go reactor");
            assert!(guest.has_native_frame());
            guest
                .start(identity, client_data, startup)
                .expect("start real Go reactor");
            assert_eq!(guest.frame(None).expect("reset native Go aiming"), None);
            assert_eq!(
                guest
                    .frame(Some(&frame()))
                    .expect("exchange native Go frame"),
                movement.then(Effects::default)
            );
            assert!(
                guest
                    .render_frame(Some(&frame()))
                    .expect("exchange native Go drawing frame")
                    .is_none()
            );
            assert!(guest.is_active());

            guest.stop();
            assert!(!guest.is_active());
        }
    }
}
