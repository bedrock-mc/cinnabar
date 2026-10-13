use std::{collections::BTreeMap, env, fs, path::Path};

use serde_json::Value;

mod build_support;

/// Generates named contracts and the active registry's fallback air identity.
fn main() {
    let root = Path::new("../..");
    let fixture = read_json("data/protocol-ids.json");
    let extensions = read_json("data/actor-data-extensions.json");
    let mut output = String::new();
    for (name, values) in fixture["groups"].as_object().unwrap() {
        let mut values = values
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, value)| (name.clone(), value.as_i64().unwrap()))
            .collect::<BTreeMap<_, _>>();
        if name == "ActorDataKey" {
            for (name, value) in extensions.as_object().unwrap() {
                assert!(
                    values
                        .insert(name.clone(), value.as_i64().unwrap())
                        .is_none()
                );
            }
        }
        let raw = match name.as_str() {
            "ActorDataKey" | "ActorFlag" | "CommandArgType" | "CommandArgFlag" => "u32",
            "ContainerName" => "u8",
            "WindowType" => "i8",
            "Effect" | "LevelEvent" | "WindowID" => "i32",
            _ => panic!("unhandled ID group {name}"),
        };
        output.push_str(&format!("wire_ids!({name}, {raw}, {{\n"));
        for (variant, value) in values {
            output.push_str(&format!("    {variant} => {value},\n"));
        }
        output.push_str("});\n");
    }

    let vendor_path = "vendor/bedrock-protocol-1-26-51/src/enums.rs";
    println!("cargo:rerun-if-changed={vendor_path}");
    let vendor = fs::read_to_string(vendor_path).unwrap();
    output.push_str("wire_ids!(Enchantment, u8, {\n");
    let body = vendor
        .split_once("impl EnchantType {")
        .unwrap()
        .1
        .split_once("Self::Unknown(value)")
        .unwrap()
        .0;
    for line in body.lines() {
        if let Some(line) = line.trim().strip_prefix("Self::") {
            let (variant, raw) = line.split_once(" => ").unwrap();
            let raw: u8 = raw.trim_end_matches(',').parse().unwrap();
            output.push_str(&format!("    {variant} => {raw},\n"));
        }
    }
    output.push_str("});\n");

    let target = read_json(root.join("assets/bedrock-target.json"));
    let schema = read_json(root.join("crates/assets/data/registry-schema.json"));
    let registry_path = root.join(target["artifacts"]["block_registry"].as_str().unwrap());
    println!("cargo:rerun-if-changed={}", registry_path.display());
    let registry = fs::read(registry_path).unwrap();
    let air = build_support::air_ids(&registry, &schema, &target);
    output.push_str(&format!(
        "/// Sequential air identity derived from the active block registry.\n\
         pub const SEQUENTIAL_AIR_NETWORK_ID: u32 = {};\n\
         /// Hashed air identity derived from the active block registry.\n\
         pub const HASHED_AIR_NETWORK_ID: u32 = {};\n",
        air.0, air.1
    ));
    let out = env::var_os("OUT_DIR").unwrap();
    fs::write(Path::new(&out).join("protocol_ids.rs"), output).unwrap();
}

/// Reads a tracked JSON input and makes Cargo notice its updates.
fn read_json(path: impl AsRef<Path>) -> Value {
    println!("cargo:rerun-if-changed={}", path.as_ref().display());
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
