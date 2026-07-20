use std::{fs, path::Path};

pub const REQUIRED_COMPLETION_IDS: &[&str] = &[
    "P2.5-NATIVE-BIOME",
    "P2-CHUNK-PUBLICATION",
    "P2.7-ATMOSPHERE",
    "P3-MOVEMENT",
    "P3.4-INPUT-CAMERA",
    "P4.3-RIGS",
    "P4.4-LIVE-ACTOR",
    "P4.5-ITEM-ACTIONS",
    "P5.1-UI",
    "P5.2-HUD",
    "P5.3-CHAT",
    "P5.4-SCOREBOARD",
    "P5.5-INTERACTION-COMBAT-INVENTORY",
    "P5.6-FORMS",
    "P5.7-PARITY-PERF",
    "P5.8-SETTINGS",
];

#[test]
fn completion_plan_tracks_each_required_id_once() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("architecture crate must live beneath the repository root");
    let roadmap = fs::read_to_string(root.join("plan.md")).expect("plan.md must be readable");
    let ledger = fs::read_to_string(root.join("docs/evidence/phases-2-5-completion-ledger.md"))
        .unwrap_or_default();
    let failures = completion_contract_failures(&roadmap, &ledger);

    assert!(
        failures.is_empty(),
        "completion requirement IDs must occur once in each contract:\n{}",
        failures.join("\n")
    );
}

fn completion_contract_failures(roadmap: &str, ledger: &str) -> Vec<String> {
    let mut failures = Vec::new();

    for required_id in REQUIRED_COMPLETION_IDS {
        let owning_phase = owning_phase(required_id);
        let (roadmap_count, owning_phase_count) =
            roadmap_tag_counts(roadmap, required_id, owning_phase);
        let ledger_heading = format!("## {required_id}");
        let ledger_count = ledger
            .lines()
            .filter(|line| line.trim() == ledger_heading)
            .count();
        if roadmap_count != 1 || owning_phase_count != 1 || ledger_count != 1 {
            failures.push(format!(
                "{required_id}: exact roadmap tags={roadmap_count}, phase {owning_phase} tags={owning_phase_count}, ledger headings={ledger_count}"
            ));
        }
    }

    failures
}

fn owning_phase(required_id: &str) -> u8 {
    match required_id.as_bytes().get(1) {
        Some(phase @ b'2'..=b'5') => phase - b'0',
        _ => unreachable!("required completion IDs must identify Phase 2 through Phase 5"),
    }
}

fn roadmap_tag_counts(roadmap: &str, required_id: &str, owning_phase: u8) -> (usize, usize) {
    let mut current_phase = None;
    let mut fenced = false;
    let mut exact_count = 0;
    let mut owning_phase_count = 0;

    for line in roadmap.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if trimmed.starts_with("## ") {
            current_phase = phase_heading(trimmed);
        }
        let line_count = exact_inline_code_count(line, required_id);
        exact_count += line_count;
        if current_phase == Some(owning_phase) {
            owning_phase_count += line_count;
        }
    }

    (exact_count, owning_phase_count)
}

fn phase_heading(line: &str) -> Option<u8> {
    let suffix = line.strip_prefix("## Phase ")?;
    let phase = suffix.as_bytes().first().copied()?;
    let boundary = suffix.as_bytes().get(1).copied();
    if matches!(phase, b'2'..=b'5') && boundary.is_none_or(|byte| !byte.is_ascii_digit()) {
        Some(phase - b'0')
    } else {
        None
    }
}

fn exact_inline_code_count(line: &str, required_id: &str) -> usize {
    let parts = line.split('`').collect::<Vec<_>>();
    if parts.len() < 3 {
        return 0;
    }
    (1..parts.len() - 1)
        .step_by(2)
        .filter(|index| parts[*index] == required_id)
        .count()
}

#[test]
fn suffixed_requirement_id_does_not_satisfy_roadmap_contract() {
    let (roadmap, ledger) = valid_contract();
    let roadmap = roadmap.replace("`P3-MOVEMENT`", "`P3-MOVEMENT-OLD`");

    let failures = completion_contract_failures(&roadmap, &ledger);

    assert!(
        failures
            .iter()
            .any(|failure| failure.contains("P3-MOVEMENT"))
    );
}

#[test]
fn misplaced_prose_reference_does_not_satisfy_roadmap_contract() {
    let (roadmap, ledger) = valid_contract();
    let roadmap = roadmap.replace("`P3-MOVEMENT`", "").replacen(
        "## Phase 2",
        "## Phase 2\nHistorical prose reference: `P3-MOVEMENT`.",
        1,
    );

    let failures = completion_contract_failures(&roadmap, &ledger);

    assert!(
        failures
            .iter()
            .any(|failure| failure.contains("P3-MOVEMENT"))
    );
}

fn valid_contract() -> (String, String) {
    let roadmap = [
        "## Phase 2",
        "`P2.5-NATIVE-BIOME`",
        "`P2-CHUNK-PUBLICATION`",
        "`P2.7-ATMOSPHERE`",
        "## Phase 3",
        "`P3-MOVEMENT`",
        "`P3.4-INPUT-CAMERA`",
        "## Phase 4",
        "`P4.3-RIGS`",
        "`P4.4-LIVE-ACTOR`",
        "`P4.5-ITEM-ACTIONS`",
        "## Phase 5",
        "`P5.1-UI`",
        "`P5.2-HUD`",
        "`P5.3-CHAT`",
        "`P5.4-SCOREBOARD`",
        "`P5.5-INTERACTION-COMBAT-INVENTORY`",
        "`P5.6-FORMS`",
        "`P5.7-PARITY-PERF`",
        "`P5.8-SETTINGS`",
    ]
    .join("\n");
    let ledger = REQUIRED_COMPLETION_IDS
        .iter()
        .map(|required_id| format!("## {required_id}"))
        .collect::<Vec<_>>()
        .join("\n");
    (roadmap, ledger)
}
