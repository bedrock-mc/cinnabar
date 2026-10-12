//! Diagnostic-only projection of an already-encoded PlayerAuthInput packet.
//!
//! The opt-in outbound movement trace reads exactly what was handed to the
//! transport, so it never recomputes physics or diverges from the wire. Flag
//! names come from the encoder's own [`super::INPUT_FLAG_ITEMS`] table rather
//! than a second spelling.

use bedrock_protocol_1_26_51::enums::{InputData, InputMode};
use bedrock_protocol_1_26_51::packets;

use super::INPUT_FLAG_ITEMS;
use crate::Packet;

/// Wire-visible movement fields of one encoded PlayerAuthInput packet.
///
/// Every value is read straight out of the encoded payload; nothing is
/// inferred or re-derived. This is diagnostic surface only and carries no
/// encode path of its own.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerAuthInputTraceSample {
    pub tick: u64,
    pub position: [f32; 3],
    pub pos_delta: [f32; 3],
    pub move_vector: [f32; 2],
    pub analog_move_vector: [f32; 2],
    pub raw_move_vector: [f32; 2],
    pub pitch: f32,
    pub yaw: f32,
    pub head_yaw: f32,
    pub camera_orientation: [f32; 3],
    /// Flag names in the encoder's ascending-bit (wire) order. Callers that
    /// present the trace may sort them for display.
    pub flag_names: Vec<&'static str>,
    /// The pinned input-mode name (`Mouse`, `Touch`, `GamePad`, ...). Values
    /// outside the named set collapse to `Unknown` because this client only
    /// ever encodes the three named modes.
    pub input_mode: &'static str,
}

/// Projects one encoded PlayerAuthInput packet for the outbound movement
/// trace, or returns `None` for any other packet kind.
#[must_use]
pub fn player_auth_input_trace_sample(packet: &Packet) -> Option<PlayerAuthInputTraceSample> {
    let packets::Packet::PlayerAuthInput(input) = &*packet.data else {
        return None;
    };
    let flag_items = input.input_data.as_slice();
    Some(PlayerAuthInputTraceSample {
        tick: input.client_tick.0,
        position: input.position.to_array(),
        pos_delta: input.pos_delta.to_array(),
        move_vector: input.move_vector.to_array(),
        analog_move_vector: input.analog_move_vector.to_array(),
        raw_move_vector: input.raw_move_vector.to_array(),
        pitch: input.player_rotation.x,
        yaw: input.player_rotation.y,
        head_yaw: input.player_head_rotation.0,
        camera_orientation: input.camera_orientation.to_array(),
        flag_names: flag_names(flag_items),
        input_mode: input_mode_name(input.input_mode),
    })
}

fn flag_names(items: &[InputData]) -> Vec<&'static str> {
    items
        .iter()
        .filter_map(|item| {
            INPUT_FLAG_ITEMS
                .iter()
                .find(|(candidate, _name)| candidate == item)
                .map(|(_candidate, name)| *name)
        })
        .collect()
}

fn input_mode_name(mode: InputMode) -> &'static str {
    match mode {
        InputMode::Undefined => "Undefined",
        InputMode::Mouse => "Mouse",
        InputMode::Touch => "Touch",
        InputMode::Gamepad => "GamePad",
        InputMode::Motioncontroller => "MotionController",
        InputMode::Count => "Count",
        InputMode::Unknown(_) => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        PlayerAuthInputSnapshot, PlayerInputFlags, PlayerInputMode, player_auth_input,
    };
    use super::{PlayerAuthInputTraceSample, player_auth_input_trace_sample};

    fn sample() -> PlayerAuthInputSnapshot {
        PlayerAuthInputSnapshot {
            tick: 4_096,
            position: [8.5, 65.0, -12.25],
            delta: [0.125, -0.5, 0.0],
            move_vector: [1.0, -1.0],
            analogue_move_vector: [0.5, -0.75],
            raw_move_vector: [1.0, -1.0],
            pitch: -12.5,
            yaw: 179.5,
            head_yaw: 181.0,
            camera_orientation: [0.0, 1.0, 0.0],
            flags: PlayerInputFlags::UP | PlayerInputFlags::SPRINTING,
            input_mode: PlayerInputMode::GamePad,
        }
    }

    #[test]
    fn projects_every_encoded_field_for_the_trace() {
        let packet = player_auth_input(sample()).expect("valid snapshot");
        let projected = player_auth_input_trace_sample(&packet).expect("PlayerAuthInput projects");

        assert_eq!(projected.tick, 4_096);
        assert_eq!(projected.position, [8.5, 65.0, -12.25]);
        assert_eq!(projected.pos_delta, [0.125, -0.5, 0.0]);
        assert_eq!(projected.move_vector, [1.0, -1.0]);
        assert_eq!(projected.analog_move_vector, [0.5, -0.75]);
        assert_eq!(projected.raw_move_vector, [1.0, -1.0]);
        assert_eq!(projected.pitch, -12.5);
        assert_eq!(projected.yaw, 179.5);
        assert_eq!(projected.head_yaw, 181.0);
        assert_eq!(projected.camera_orientation, [0.0, 1.0, 0.0]);
        // Wire order: Up is bit 10, Sprinting bit 20.
        assert_eq!(projected.flag_names, vec!["Up", "Sprinting"]);
        assert_eq!(projected.input_mode, "GamePad");
    }

    #[test]
    fn flag_names_follow_the_encoder_table_in_ascending_bit_order() {
        let mut snapshot = sample();
        snapshot.flags = PlayerInputFlags::HORIZONTAL_COLLISION
            | PlayerInputFlags::JUMP_DOWN
            | PlayerInputFlags::START_SNEAKING;
        let packet = player_auth_input(snapshot).expect("valid snapshot");
        let projected = player_auth_input_trace_sample(&packet).expect("PlayerAuthInput projects");
        // JumpDown bit 3, StartSneaking bit 27, HorizontalCollision bit 49.
        assert_eq!(
            projected.flag_names,
            vec!["JumpDown", "StartSneaking", "HorizontalCollision"]
        );
    }

    #[test]
    fn other_packet_kinds_do_not_project() {
        let packet = crate::request_sub_chunk_column(0, 0, 0, -4, 1).expect("bounded request");
        assert!(player_auth_input_trace_sample(&packet).is_none());
    }

    #[test]
    fn projection_is_none_safe_when_no_flags_are_set() {
        let mut snapshot = sample();
        snapshot.flags = PlayerInputFlags::NONE;
        let packet = player_auth_input(snapshot).expect("valid snapshot");
        let projected = player_auth_input_trace_sample(&packet).expect("PlayerAuthInput projects");
        assert_eq!(projected.flag_names, Vec::<&'static str>::new());
    }

    #[test]
    fn trace_sample_type_documents_wire_shape() {
        // Guards accidental field-type drift against the wire contract the
        // formatter relies on.
        let PlayerAuthInputTraceSample {
            tick,
            position,
            pos_delta,
            move_vector,
            analog_move_vector,
            raw_move_vector,
            pitch: _,
            yaw: _,
            head_yaw: _,
            camera_orientation,
            flag_names,
            input_mode,
        } = PlayerAuthInputTraceSample {
            tick: 1,
            position: [0.0; 3],
            pos_delta: [0.0; 3],
            move_vector: [0.0; 2],
            analog_move_vector: [0.0; 2],
            raw_move_vector: [0.0; 2],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            camera_orientation: [0.0; 3],
            flag_names: Vec::new(),
            input_mode: "Mouse",
        };
        let _: u64 = tick;
        let _: [f32; 3] = position;
        let _: [f32; 3] = pos_delta;
        let _: [f32; 2] = move_vector;
        let _: [f32; 2] = analog_move_vector;
        let _: [f32; 2] = raw_move_vector;
        let _: [f32; 3] = camera_orientation;
        let _: Vec<&'static str> = flag_names;
        assert_eq!(input_mode, "Mouse");
    }
}
