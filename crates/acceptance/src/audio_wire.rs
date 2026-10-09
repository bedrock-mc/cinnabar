//! Opt-in bounded observation of committed decoded audio, before catalog resolution.
//! Rows establish neither audible output nor backend interpretation of wire fields.

use serde::Serialize;

use client_presentation::audio_ingress::SequencedAudioEvent;

const TARGET: &str = "game.player.attack.critical";
const MAX_ROWS: usize = 4;

fn selected(value: Option<&str>) -> bool {
    value == Some(TARGET)
}

#[derive(Debug, Serialize)]
struct Row {
    schema: &'static str,
    authority: &'static str,
    /// Opaque local lifetime counter, not server protocol identity.
    origin_stream_session_id: u64,
    observed_fifo_sequence: u64,
    position_eighth_blocks: [i32; 3],
    position_blocks: [f64; 3],
    loop_count: i32,
    gain_bits: u32,
    pitch_bits: u32,
    server_sound_handle_present: bool,
}

#[derive(Debug, bevy::prelude::Resource)]
pub struct WireEvidence {
    enabled: bool,
    session: Option<u64>,
    last_sequence: Option<u64>,
    rows: Vec<Row>,
}

impl Default for WireEvidence {
    fn default() -> Self {
        Self::new(selected(
            std::env::var(crate::markers::AUDIO_WIRE_EVIDENCE)
                .ok()
                .as_deref(),
        ))
    }
}

impl WireEvidence {
    /// Selects whether this bounded observation lane records its target.
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            session: None,
            last_sequence: None,
            rows: Vec::new(),
        }
    }

    /// Returns retained stream and FIFO identities for composed observation tests.
    #[cfg(feature = "test-support")]
    pub fn observed_sequences(&self) -> Vec<(u64, u64)> {
        self.rows
            .iter()
            .map(|row| (row.origin_stream_session_id, row.observed_fifo_sequence))
            .collect()
    }

    pub fn bind(&mut self, session: Option<u64>) {
        if self.session != session {
            self.session = session;
            self.last_sequence = None;
            self.rows.clear();
        }
    }

    fn observe(&mut self, session: u64, envelope: &SequencedAudioEvent) -> Option<&Row> {
        if !self.enabled
            || self.session != Some(session)
            || envelope.origin_stream_session_id != session
            || self
                .last_sequence
                .is_some_and(|previous| envelope.sequence <= previous)
        {
            return None;
        }
        // Observe all audio sequence identities, not just the fixed named target.
        self.last_sequence = Some(envelope.sequence);
        if self.rows.len() >= MAX_ROWS {
            return None;
        }
        let protocol::AudioEvent::Play(play) = &envelope.event else {
            return None;
        };
        if play.name.as_ref() != TARGET {
            return None;
        }
        self.rows.push(Row {
            schema: "critical_audio_wire_v1",
            authority: "committed_decoded_ingress_before_catalog_resolution",
            origin_stream_session_id: session,
            observed_fifo_sequence: envelope.sequence,
            position_eighth_blocks: play.position,
            position_blocks: play.position.map(|value| f64::from(value) / 8.0),
            loop_count: play.loop_count,
            gain_bits: play.volume.to_bits(),
            pitch_bits: play.pitch.to_bits(),
            server_sound_handle_present: play.server_sound_handle.is_some(),
        });
        self.rows.last()
    }

    pub fn emit(&mut self, session: u64, envelope: &SequencedAudioEvent) {
        if let Some(row) = self.observe(session, envelope)
            && let Ok(json) = serde_json::to_string(row)
        {
            write_marker(&mut diagnostics::console::stdout(), &json);
        }
    }
}

/// Writes bounded diagnostic evidence without making stdout part of session authority.
fn write_marker(writer: &mut impl std::io::Write, json: &str) {
    let _ = writeln!(writer, "{}={json}", crate::markers::AUDIO_WIRE_EVIDENCE);
}

#[cfg(test)]
#[path = "wire_evidence_tests.rs"]
mod tests;
