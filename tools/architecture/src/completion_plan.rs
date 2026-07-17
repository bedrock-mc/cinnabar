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
    let mut failures = Vec::new();

    for required_id in REQUIRED_COMPLETION_IDS {
        let roadmap_count = roadmap.matches(required_id).count();
        let ledger_heading = format!("## {required_id}");
        let ledger_count = ledger
            .lines()
            .filter(|line| line.trim() == ledger_heading)
            .count();
        if roadmap_count != 1 || ledger_count != 1 {
            failures.push(format!(
                "{required_id}: roadmap occurrences={roadmap_count}, ledger headings={ledger_count}"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "completion requirement IDs must occur once in each contract:\n{}",
        failures.join("\n")
    );
}
