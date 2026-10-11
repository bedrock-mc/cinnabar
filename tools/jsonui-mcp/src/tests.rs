//! The MCP wire and tools over the authored example packs.

use serde_json::{Value, json};

use crate::tools::Server;
use mcp_stdio::handle;

fn examples() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../jsonui-editor/examples")
}

fn call(server: &mut Server, name: &str, arguments: Value) -> Value {
    let reply = handle(
        server,
        &json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call",
                 "params": { "name": name, "arguments": arguments } }),
    )
    .unwrap();
    assert_eq!(reply["id"], 7);
    let result = &reply["result"];
    assert_eq!(result["isError"], false, "{result}");
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn loaded() -> Server {
    let mut server = Server::new(None);
    let paths = [examples().join("base"), examples().join("overlay")];
    let paths: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    let loaded = call(&mut server, "load_packs", json!({ "paths": paths }));
    assert_eq!(loaded["layers"][1]["ui_files"], 1);
    server
}

#[test]
fn initialize_and_list_tools_follow_the_protocol() {
    let mut server = Server::new(None);
    let reply = handle(
        &mut server,
        &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                 "params": { "protocolVersion": "2024-11-05" } }),
    )
    .unwrap();
    assert_eq!(reply["result"]["protocolVersion"], "2024-11-05");
    assert!(
        handle(
            &mut server,
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })
        )
        .is_none()
    );
    let tools = handle(
        &mut server,
        &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    )
    .unwrap();
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "load_packs",
            "list_screens",
            "resolve",
            "validate",
            "layout",
            "render_png",
            "edit_file",
            "export_pack"
        ]
    );
    let unknown = handle(
        &mut server,
        &json!({ "jsonrpc": "2.0", "id": 3, "method": "nope" }),
    )
    .unwrap();
    assert_eq!(unknown["error"]["code"], -32601);
}

#[test]
fn layout_returns_boxes_and_resolve_names_sources() {
    let mut server = loaded();
    let screens = call(&mut server, "list_screens", json!({}));
    assert_eq!(screens["screens"][0]["reference"], "example.example_screen");
    let layout = call(
        &mut server,
        "layout",
        json!({ "reference": "example.example_screen", "size": [960, 540], "scale": 2, "context": {} }),
    );
    assert_eq!(layout["root"], json!([480.0, 270.0]));
    let footer = layout["boxes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["key"] == "/example_screen/card/content/footer");
    assert!(footer.is_some(), "the overlay's inserted footer lays out");
    let resolved = call(
        &mut server,
        "resolve",
        json!({ "reference": "example.example_screen", "max_nodes": 3 }),
    );
    let card = &resolved["tree"]["children"][1];
    assert_eq!(card["name"], "card");
    let color = card["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["key"] == "color")
        .unwrap();
    assert_eq!(
        color["source"]["via"]["declared"]["path"],
        "ui/example_screen.json"
    );
    assert_eq!(
        resolved["tree"]["children"][1]["children"][0]["truncated"],
        true
    );
}

// An unset flag reads as null, so the empty context decides `(not $touch)`.
#[test]
fn validate_decides_unset_flags_as_null() {
    let mut server = loaded();
    let report = call(
        &mut server,
        "validate",
        json!({ "files": ["ui/example_screen.json"], "context": "Empty" }),
    );
    let messages: Vec<&str> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["message"].as_str().unwrap())
        .collect();
    assert!(
        !messages.iter().any(|m| m.contains("undecidable `ignored`")),
        "{messages:?}"
    );
}

#[test]
fn render_png_writes_a_png_and_returns_it_inline() {
    let mut server = loaded();
    let out = std::env::temp_dir().join(format!("jsonui-mcp-test-{}.png", std::process::id()));
    let reply = handle(
        &mut server,
        &json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": { "name": "render_png",
            "arguments": { "reference": "example.example_screen", "size": [320, 180], "out": out.display().to_string() } } }),
    )
    .unwrap();
    let content = reply["result"]["content"].as_array().unwrap();
    assert_eq!(content[1]["type"], "image");
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let _ = std::fs::remove_file(out);
}

// Re-exporting to the same path keeps both UUIDs and bumps the patch version.
#[test]
fn export_pack_reexports_keep_uuids() {
    let mut server = loaded();
    let edited = call(
        &mut server,
        "edit_file",
        json!({ "path": "ui/new.json", "scratch": true,
                "text": "{ \"namespace\": \"n\", \"s\": { \"type\": \"screen\" } }" }),
    );
    assert_eq!(edited["path"], "ui/scratch.json");
    let out = std::env::temp_dir().join(format!("jsonui-mcp-export-{}.mcpack", std::process::id()));
    let first = call(
        &mut server,
        "export_pack",
        json!({ "out": out.display().to_string(), "name": "Test UI" }),
    );
    assert_eq!(first["pack"]["version"], json!([1, 0, 0]));
    assert!(
        first["files"]
            .as_array()
            .unwrap()
            .contains(&json!("ui/scratch.json"))
    );
    let second = call(
        &mut server,
        "export_pack",
        json!({ "out": out.display().to_string() }),
    );
    assert_eq!(second["pack"]["header_uuid"], first["pack"]["header_uuid"]);
    assert_eq!(second["pack"]["module_uuid"], first["pack"]["module_uuid"]);
    assert_eq!(second["pack"]["version"], json!([1, 0, 1]));
    let _ = std::fs::remove_file(out);
}
