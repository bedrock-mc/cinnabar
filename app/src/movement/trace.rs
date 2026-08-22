//! Opt-in single-line JSON trace of transmitted movement samples.
//!
//! Set `RUST_MCBE_MOVEMENT_TRACE=1` to emit one compact JSON object per
//! PlayerAuthInput packet actually handed to the transport. Any other value,
//! or an unset variable, keeps the send path byte-identical to the untraced
//! build: the gate is a lazily initialized flag and short-circuits before any
//! allocation. The trace reads only data already present at the send site and
//! never changes what is transmitted; a formatting failure is swallowed so it
//! can never abort the session.

use std::{ffi::OsStr, io::Write as _, sync::OnceLock};

use serde_json::json;

use protocol::{Packet, PlayerAuthInputTraceSample, player_auth_input_trace_sample};

use crate::acceptance::markers::MOVEMENT_TRACE;

/// The exact value of [`MOVEMENT_TRACE`] that enables the outbound trace.
const MOVEMENT_TRACE_ENABLED_VALUE: &str = "1";

const SCHEMA_TAG: &str = "rust-mcbe-pai-trace-v1";

/// Pure enablement rule for one environment-variable observation.
fn enabled_for_env_value(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new(MOVEMENT_TRACE_ENABLED_VALUE))
}

/// Whether this process traces outbound movement. Evaluated at most once.
fn movement_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| enabled_for_env_value(std::env::var_os(MOVEMENT_TRACE).as_deref()))
}

/// Formats the trace line for one packet when tracing is enabled.
///
/// Returns `None` without touching the packet whenever the gate is closed,
/// and for any packet that is not a PlayerAuthInput.
pub(crate) fn pending_trace_line(session_generation: u64, packet: &Packet) -> Option<String> {
    trace_line_if(movement_trace_enabled(), session_generation, packet)
}

fn trace_line_if(enabled: bool, session_generation: u64, packet: &Packet) -> Option<String> {
    if !enabled {
        return None;
    }
    let sample = player_auth_input_trace_sample(packet)?;
    format_trace_line(session_generation, &sample)
}

/// Renders one compact single-line JSON object for one transmitted sample.
///
/// Flag names are sorted alphabetically for stable scanning. Returns `None`
/// only if serialization itself fails; callers must treat that as skippable.
fn format_trace_line(
    session_generation: u64,
    sample: &PlayerAuthInputTraceSample,
) -> Option<String> {
    let mut flags = sample.flag_names.clone();
    flags.sort_unstable();
    let line = json!({
        "schema": SCHEMA_TAG,
        "tick": sample.tick,
        "session_generation": session_generation,
        "position": sample.position,
        "pos_delta": sample.pos_delta,
        "move_vector": sample.move_vector,
        "analog_move_vector": sample.analog_move_vector,
        "raw_move_vector": sample.raw_move_vector,
        "flags": flags,
        "pitch": sample.pitch,
        "yaw": sample.yaw,
        "head_yaw": sample.head_yaw,
        "input_mode": sample.input_mode,
        "camera_orientation": sample.camera_orientation,
    });
    serde_json::to_string(&line).ok()
}

/// Writes one already-formatted trace line to stdout without buffering.
///
/// Write failures are ignored on purpose: losing diagnostic output must never
/// abort the session.
pub(crate) fn write_trace_line(line: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use serde_json::Value;

    use super::{SCHEMA_TAG, enabled_for_env_value, format_trace_line, trace_line_if};
    use protocol::{
        PlayerAuthInputSnapshot, PlayerInputFlags, PlayerInputMode, player_auth_input,
        player_auth_input_trace_sample, request_sub_chunk_column,
    };

    fn snapshot() -> PlayerAuthInputSnapshot {
        PlayerAuthInputSnapshot {
            tick: 9_183,
            position: [10.5, 72.0, -3.75],
            // Dyadic fractions so f32 -> JSON float comparisons are exact.
            delta: [0.25, 0.0625, -0.125],
            move_vector: [0.0, 1.0],
            analogue_move_vector: [0.0, 1.0],
            raw_move_vector: [0.0, 1.0],
            pitch: -4.25,
            yaw: 88.5,
            head_yaw: 90.0,
            camera_orientation: [0.25, -0.5, 0.75],
            flags: PlayerInputFlags::JUMPING
                | PlayerInputFlags::SPRINTING
                | PlayerInputFlags::HORIZONTAL_COLLISION,
            input_mode: PlayerInputMode::Mouse,
        }
    }

    #[test]
    fn env_gate_requires_exactly_the_digit_one() {
        assert!(enabled_for_env_value(Some(&OsString::from("1"))));
        assert!(!enabled_for_env_value(None));
        for disabled in ["", "0", "true", "yes", "01", "1 ", " 1", "one"] {
            assert!(
                !enabled_for_env_value(Some(&OsString::from(disabled))),
                "value {disabled:?} must disable the trace"
            );
        }
    }

    #[test]
    fn gated_send_path_formats_nothing_while_disabled() {
        let packet = player_auth_input(snapshot()).expect("valid snapshot");
        // The production send path consults exactly this composition; the
        // disabled branch must return before projecting or formatting.
        assert!(trace_line_if(false, 11, &packet).is_none());
    }

    #[test]
    fn non_movement_packets_never_format_even_when_enabled() {
        let packet = request_sub_chunk_column(0, 3, -2, -5, 2).expect("bounded request");
        assert!(trace_line_if(true, 11, &packet).is_none());
    }

    #[test]
    fn every_expected_field_roundtrips_through_one_json_line() {
        let packet = player_auth_input(snapshot()).expect("valid snapshot");
        let line = trace_line_if(true, 42, &packet).expect("enabled trace formats the sample");

        assert!(!line.contains('\n'), "trace lines must stay single-line");
        let parsed: Value = serde_json::from_str(&line).expect("trace output must be valid JSON");
        let object = parsed.as_object().expect("trace output must be an object");

        assert_eq!(object["schema"], SCHEMA_TAG);
        assert_eq!(object["tick"], 9_183);
        assert_eq!(object["session_generation"], 42);
        assert_eq!(object["position"], serde_json::json!([10.5, 72.0, -3.75]));
        assert_eq!(
            object["pos_delta"],
            serde_json::json!([0.25, 0.0625, -0.125])
        );
        assert_eq!(object["move_vector"], serde_json::json!([0.0, 1.0]));
        assert_eq!(object["analog_move_vector"], serde_json::json!([0.0, 1.0]));
        assert_eq!(object["raw_move_vector"], serde_json::json!([0.0, 1.0]));
        // Names are the encoder table's spellings, sorted alphabetically.
        assert_eq!(
            object["flags"],
            serde_json::json!(["HorizontalCollision", "Jumping", "Sprinting"])
        );
        assert_eq!(object["pitch"], -4.25);
        assert_eq!(object["yaw"], 88.5);
        assert_eq!(object["head_yaw"], 90.0);
        assert_eq!(object["input_mode"], "Mouse");
        assert_eq!(
            object["camera_orientation"],
            serde_json::json!([0.25, -0.5, 0.75])
        );
        assert_eq!(object.len(), 14, "schema tag plus thirteen movement fields");
    }

    #[test]
    fn empty_flags_render_as_an_empty_array() {
        let mut quiet = snapshot();
        quiet.flags = PlayerInputFlags::NONE;
        let packet = player_auth_input(quiet).expect("valid snapshot");
        let projected = player_auth_input_trace_sample(&packet).expect("PlayerAuthInput projects");
        let line = format_trace_line(7, &projected).expect("formatter renders the sample");
        let parsed: Value = serde_json::from_str(&line).expect("valid JSON");
        assert_eq!(parsed["flags"], serde_json::json!([]));
        assert_eq!(parsed["session_generation"], 7);
    }
}
