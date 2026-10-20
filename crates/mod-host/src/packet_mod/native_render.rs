//! Independent drawing output on the same serialized packet worker.
use super::PacketMod;
use anyhow::{Result, ensure};
use mod_api::{
    native_frame::{Frame, MAX_BYTES},
    native_render::Scene,
};

impl PacketMod {
    /// Drawing needs neither camera authority nor gameplay input capture.
    pub fn render_frame(&mut self, frame: Option<&Frame>) -> Result<Option<Scene>> {
        if !self.is_active() || !self.guest.has_native_render() {
            return Ok(None);
        }
        ensure!(frame.is_none_or(Frame::is_valid), "invalid render frame");
        let observation = frame.map(|frame| mod_api::native_render::Frame {
            version: frame.version,
            session: frame.session,
            dimension: frame.dimension,
        });
        let input = serde_json::to_vec(&observation)?;
        ensure!(
            input.len() <= MAX_BYTES,
            "render input exceeds its byte limit"
        );
        self.call("render-frame", self.call_deadline, |guest| {
            let output = match guest.render_frame(&input)? {
                Ok(Some(output)) => output,
                Ok(None) => return Ok(Ok(None)),
                Err(error) => return Ok(Err(error)),
            };
            ensure!(
                output.len() <= mod_api::native_render::MAX_BYTES,
                "render output exceeds its byte limit"
            );
            let scene: Option<Scene> = serde_json::from_slice(&output)?;
            ensure!(
                scene.as_ref().is_none_or(|scene| scene.is_valid()
                    && frame.is_some_and(|frame| (scene.session, scene.dimension)
                        == (frame.session, frame.dimension))),
                "invalid native render scene"
            );
            if let Some(scene) = &scene {
                let bits = guest.native_render_capabilities();
                ensure!(
                    scene.entities.is_empty()
                        || bits & mod_api::native_render::ENTITY_HIGHLIGHTS != 0,
                    "actor highlight authority denied"
                );
                ensure!(
                    scene.blocks.is_empty() || bits & mod_api::native_render::BLOCK_HIGHLIGHTS != 0,
                    "block highlight authority denied"
                );
            }
            Ok(Ok(scene))
        })
        .map(Option::flatten)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModGrants, packet_mod::CompiledPacketMod};
    /// Creates a canonical renderer without granting camera, players or input authority.
    fn guest(scene: &str, walls: bool, highlights: bool) -> PacketMod {
        let mut source = include_str!("tests/guest.wat")
            .replace("$IMPORTS", "")
            .replace("$START", "");
        source = source.trim_end().strip_suffix(')').unwrap().to_owned();
        let bytes: String = scene.bytes().map(|byte| format!("\\{byte:02x}")).collect();
        source.push_str(&format!(r#"
          (data (i32.const 2048) "{bytes}") (data (i32.const 4096) "null")
          (func (export "native-init") (param i32)
            (if (i32.ne (local.get 0) (i32.const {mask})) (then unreachable)))
          (func (export "render-frame") (param $input i32) (param $length i32) (result i32)
            (i32.store (i32.const 128) (i32.const 0))
            (i32.store (i32.const 132) (i32.const 2048))
            (i32.store (i32.const 136) (i32.const {length}))
            (if (i32.eq (i32.load8_u (local.get $input)) (i32.const 110)) (then
              (i32.store (i32.const 132) (i32.const 4096)) (i32.store (i32.const 136) (i32.const 4))))
            (i32.const 128)))"#,length=scene.len(),mask=if walls {8 | if highlights {16 | 32} else {0}}else{0}));
        CompiledPacketMod::compile(
            &wat::parse_str(source).unwrap(),
            &ModGrants {
                packets: true,
                world_draw: true,
                render_through_walls: walls,
                entity_observe: highlights,
                entity_highlight: highlights,
                block_highlights: highlights,
                ..Default::default()
            },
        )
        .unwrap()
        .instantiate()
        .unwrap()
    }
    #[test]
    fn drawing_is_independent_of_camera_and_suspended_input_but_fenced_to_world() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../mod-api/native-render-v1.json")).unwrap();
        let scene = serde_json::to_string(&fixture["scene"]).unwrap();
        let mut guest = guest(&scene, true, true);
        let frame = Frame {
            version: 1,
            session: 1,
            dimension: 0,
            eye: [0.; 3],
            yaw: 0.,
            pitch: 0.,
            attack_held: false,
            seconds: 0.016,
            players: Vec::new(),
            suspended: true,
        };
        assert!(guest.has_native_frame());
        assert!(guest.frame(Some(&frame)).unwrap().is_none());
        assert!(guest.render_frame(None).unwrap().is_none());
        let out = guest.render_frame(Some(&frame)).unwrap().unwrap();
        assert_eq!(out.session, 1);
        let mut other = frame;
        other.dimension = 1;
        assert!(guest.render_frame(Some(&other)).is_err());
        assert!(!guest.is_active());
    }
    #[test]
    fn missing_depth_grant_cannot_negotiate_rendering() {
        // An unnegotiated export never receives native-init and cannot emit a scene.
        let mut guest = guest("null", false, false);
        assert!(!guest.has_native_frame());
        assert!(guest.render_frame(None).unwrap().is_none());
    }
    #[test]
    fn ungranted_actor_or_block_payload_quarantines_instead_of_publishing() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../mod-api/native-render-v1.json")).unwrap();
        for rejected in ["entities", "blocks"] {
            let mut scene = fixture["scene"].clone();
            let other = if rejected == "entities" {
                "blocks"
            } else {
                "entities"
            };
            scene[other] = serde_json::json!([]);
            let mut guest = guest(&scene.to_string(), true, false);
            let frame = Frame {
                version: 1,
                session: 1,
                dimension: 0,
                eye: [0.; 3],
                yaw: 0.,
                pitch: 0.,
                attack_held: false,
                seconds: 0.016,
                players: Vec::new(),
                suspended: true,
            };
            assert!(guest.render_frame(Some(&frame)).is_err(), "{rejected}");
            assert!(!guest.is_active());
        }
    }
}
