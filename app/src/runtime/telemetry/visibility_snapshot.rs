use render::{VisibilityDiagnosticSnapshot, VisibilityDiagnosticsInput};

/// Excludes a retained startup witness after normal play stops collecting visibility evidence.
pub(super) fn active_snapshot(
    input: &VisibilityDiagnosticsInput,
    snapshot: VisibilityDiagnosticSnapshot,
) -> VisibilityDiagnosticSnapshot {
    if input.enabled() {
        snapshot
    } else {
        VisibilityDiagnosticSnapshot::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopped_startup_probe_does_not_report_its_retained_witness_as_current() {
        let witness = VisibilityDiagnosticSnapshot {
            frame_generation: 123,
            ..Default::default()
        };
        let mut input = VisibilityDiagnosticsInput::new(false);
        input.set_startup_probe_enabled(true);
        assert_eq!(active_snapshot(&input, witness), witness);

        input.set_startup_probe_enabled(false);
        assert_eq!(active_snapshot(&input, witness).frame_generation, 0);
    }

    #[test]
    fn explicit_acceptance_probe_keeps_reporting_after_startup() {
        let witness = VisibilityDiagnosticSnapshot {
            frame_generation: 456,
            ..Default::default()
        };
        let mut input = VisibilityDiagnosticsInput::new(true);
        input.set_startup_probe_enabled(true);
        input.set_startup_probe_enabled(false);
        assert_eq!(active_snapshot(&input, witness), witness);
    }
}
