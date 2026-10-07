use serde::Deserialize;
use sha2::{Digest, Sha256};
use sim::{
    Aabb, CollisionQuery, CollisionWorld, MovementInput, PlayerState, Simulator, Vec3,
    WorldQueryError,
};

const TRACE: &str = include_str!("../../fixtures/bedsim-34d11dc5-controls.jsonl");
const PROVENANCE: &str = include_str!("../../fixtures/bedsim-34d11dc5-controls.provenance.json");

struct Empty;
impl CollisionWorld for Empty {
    fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    name: String,
    input: MovementInput,
    processed: [f32; 2],
}

fn assert_hash(bytes: &[u8], expected: &serde_json::Value) {
    // Canonical source text uses LF, independent of a developer's checkout.
    let text = std::str::from_utf8(bytes).unwrap().replace("\r\n", "\n");
    assert_eq!(
        format!("{:x}", Sha256::digest(text.as_bytes())),
        expected.as_str().unwrap()
    );
}

#[test]
fn pinned_newer_control_oracle_has_exact_source_and_fixture_identity() {
    let provenance: serde_json::Value = serde_json::from_str(PROVENANCE).unwrap();
    assert_eq!(
        provenance["source_commit"],
        "34d11dc576aeb56a7f7f0541dbb5a600cc443488"
    );
    assert_eq!(
        provenance["version"],
        "v0.1.7-0.20260916140831-34d11dc576ae"
    );
    assert_hash(TRACE.as_bytes(), &provenance["sha256"]);
    assert_hash(
        include_bytes!("../../../../tools/bedsimtrace-controls/main.go"),
        &provenance["generator_source_sha256"],
    );
    assert_hash(
        include_bytes!("../../../../tools/bedsimtrace-controls/go.mod"),
        &provenance["go_mod_sha256"],
    );
    assert_hash(
        include_bytes!("../../../../tools/bedsimtrace-controls/go.sum"),
        &provenance["go_sum_sha256"],
    );
    assert!(
        include_str!("../../../../tools/bedsimtrace-controls/go.sum")
            .contains(provenance["module_sum"].as_str().unwrap())
    );
}

#[test]
fn all_fourteen_public_model_controls_match_without_replacing_kinematic_fixtures() {
    let mut names = std::collections::BTreeSet::new();
    for line in TRACE.lines() {
        let record: Record = serde_json::from_str(line).unwrap();
        assert!(names.insert(record.name.clone()));
        let output = Simulator::default()
            .tick_with_controls(&mut PlayerState::new(Vec3::ZERO), record.input, &Empty)
            .unwrap();
        // Only the semantic control result is compared at its wire precision.
        // The simulator's f64 kinematics are deliberately not an f32 oracle.
        // bedsim multiplies item and pose factors first; vanilla scales by pose, then item.
        let expected = if record.name == "raw_nonbinary_item_pose" {
            [0.7_f32 * 0.3 * 0.7, -0.9_f32 * 0.3 * 0.7]
        } else {
            record.processed
        };
        for (actual, expected) in output.controls.move_vector.into_iter().zip(expected) {
            assert_eq!(
                (actual as f32).to_bits(),
                expected.to_bits(),
                "{}",
                record.name
            );
        }
    }
    assert_eq!(names.len(), 14);
}
