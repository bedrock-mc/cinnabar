use std::{fs, process::Command};

use super::{Project, generate_at, go};
use crate::{Value, message::WireValue};

const SOURCE: &str = r#"
#![project(id = "space", permissions(scene))]
use experience_sdk::{id, message::WorldId};
pub const HOME: WorldId = id!("space:home");
#[message(id = "space.state", schema = 7, to_client)]
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub enabled: bool,
    pub signed: i64,
    pub count: u32,
    #[field(max_bytes = 12)]
    pub title: String,
    #[field(max_bytes = 96)]
    pub world: WorldId,
    #[field(max_items = 3, min = -4, max = 8)]
    pub samples: Vec<i16>,
}
"#;

#[test]
fn every_shared_wire_change_changes_the_fingerprint() {
    let original = Project::parse(SOURCE).unwrap().fingerprint();
    for changed in [
        SOURCE.replace("space:home", "space:moon"),
        SOURCE.replace("space.state", "space.view"),
        SOURCE.replace("schema = 7", "schema = 8"),
        SOURCE.replace("to_client", "to_server"),
        SOURCE.replace("max_bytes = 12", "max_bytes = 11"),
        SOURCE.replace("pub count: u32", "pub count: i32"),
        SOURCE.replace("pub title:", "pub label:"),
        SOURCE.replace("pub world: WorldId", "pub world: EntityId"),
    ] {
        assert_ne!(original, Project::parse(&changed).unwrap().fingerprint());
    }
    assert_eq!(
        original,
        Project::parse(&format!("{SOURCE}\n// harmless comment\n"))
            .unwrap()
            .fingerprint()
    );
}

#[test]
fn ambiguous_or_unbounded_schemas_fail_generation() {
    for changed in [
        SOURCE.replace("id = \"space\",", "id = \"..\","),
        SOURCE.replace("#[field(max_bytes = 12)]", ""),
        SOURCE.replace("space.state", "foreign.state"),
        SOURCE.replace("pub count: u32", "pub count: f32"),
        SOURCE.replace("pub count: u32", "pub count: u64"),
        SOURCE.replace("max = 8", "max = 40000"),
        SOURCE.replace("pub const HOME", "#[cfg(any())]\npub const HOME"),
        SOURCE.replace("pub enabled: bool,", "#[cfg(any())]\npub enabled: bool,"),
    ] {
        assert!(Project::parse(&changed).is_err(), "{changed}");
    }
}

#[test]
fn digit_leading_go_names_fail_generation() {
    for source in [
        SOURCE.replace("struct State", "struct _2d"),
        SOURCE.replace("const HOME", "const _2D"),
        SOURCE.replace("pub enabled", "pub _2d"),
    ] {
        assert!(go::generate(&Project::parse(&source).unwrap()).is_err());
    }
}

#[test]
fn asset_renames_remove_constants_and_collisions_fail() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join("assets")).unwrap();
    fs::write(root.path().join("src/shared.rs"), SOURCE).unwrap();
    fs::write(root.path().join("assets/star.txt"), b"star").unwrap();
    let out = root.path().join("out");
    generate_at(root.path(), &out).unwrap();
    fs::rename(
        root.path().join("assets/star.txt"),
        root.path().join("assets/moon.txt"),
    )
    .unwrap();
    generate_at(root.path(), &out).unwrap();
    fs::write(root.path().join("assets/a-b.txt"), b"one").unwrap();
    fs::write(root.path().join("assets/a_b.txt"), b"two").unwrap();
    assert!(generate_at(root.path(), &out).is_err());
}

#[test]
fn go_messages_round_trip_rust_records_with_exact_integer_and_text_bounds() {
    let project = Project::parse(SOURCE).unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("go.mod"), super::go_module(&project.id)).unwrap();
    fs::write(
        root.path().join("messages.go"),
        go::generate(&project).unwrap(),
    )
    .unwrap();
    let record = vec![
        Value::Bool(true),
        Value::Integer(i64::MIN),
        Value::Integer(i64::from(u32::MAX)),
        Value::Text("星🌙".into()),
        Value::Text("space:home".into()),
        vec![-4_i16, 8].to_value(),
    ];
    let raw = serde_json::to_string(&record).unwrap();
    let test = format!(
        r#"package messages
import ("os"; "testing"; "encoding/json"; "bytes"; "strings")
// TestRustRecord proves the generated Go codec reads Rust values without float conversion.
func TestRustRecord(t *testing.T) {{
    raw := []byte({raw:?})
    var state State
    if err := state.DecodeRecord(raw); err != nil {{ t.Fatal(err) }}
    if state.World != Home || state.Count != 4294967295 || state.Signed != -9223372036854775808 {{ t.Fatal(state) }}
    native, err := state.Encode(); if err != nil {{ t.Fatal(err) }}
    decoded, err := StateType.Decode(native); if err != nil || decoded.World != state.World {{ t.Fatal(decoded, err) }}
    encoded, err := state.EncodeRecord(); if err != nil {{ t.Fatal(err) }}
    // Compare JSON values; escaping is intentionally left to each standard library.
    var a, b any; da := json.NewDecoder(bytes.NewReader(raw)); da.UseNumber(); db := json.NewDecoder(bytes.NewReader(encoded)); db.UseNumber()
    if da.Decode(&a) != nil || db.Decode(&b) != nil {{ t.Fatal("invalid JSON") }}
    ca, _ := json.Marshal(a); cb, _ := json.Marshal(b); if !bytes.Equal(ca, cb) {{ t.Fatalf("%s != %s", ca, cb) }}
    if err := os.WriteFile("go-record.json", encoded, 0600); err != nil {{ t.Fatal(err) }}
    state.Title = "this text is too long"; if _, err := state.EncodeRecord(); err == nil {{ t.Fatal("unbounded text") }}
    state.Title = "ok"; state.Samples = []int16{{9}}; if _, err := state.EncodeRecord(); err == nil {{ t.Fatal("unbounded integer") }}
    state.Samples = nil; if _, err := state.EncodeRecord(); err != nil {{ t.Fatal(err) }}
    state.World = "invalid"; if _, err := state.EncodeRecord(); err == nil {{ t.Fatal("untyped ID") }}
    if err := state.DecodeRecord([]byte("null")); err == nil {{ t.Fatal("null record") }}
    state.World = Home; state.Title = string([]byte{{255}}); if _, err := state.EncodeRecord(); err == nil {{ t.Fatal("invalid UTF-8") }}
    duplicate := strings.Replace(string(raw), `"type":"bool"`, `"type":"integer","type":"bool"`, 1)
    if err := state.DecodeRecord([]byte(duplicate)); err == nil {{ t.Fatal("duplicate JSON fields") }}
    surrogate := strings.Replace(string(raw), "星🌙", `\ud800`, 1)
    if err := state.DecodeRecord([]byte(surrogate)); err == nil {{ t.Fatal("unpaired surrogate") }}
}}
"#
    );
    fs::write(root.path().join("messages_test.go"), test).unwrap();
    let output = Command::new("go")
        .args(["test", "-mod=mod", "./..."])
        .env("GOWORK", "off")
        .current_dir(root.path())
        .output()
        .expect("Go is required for message codec conformance");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let returned: Vec<Value> =
        serde_json::from_slice(&fs::read(root.path().join("go-record.json")).unwrap()).unwrap();
    assert_eq!(returned, record);
}
