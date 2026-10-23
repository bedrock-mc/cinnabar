//! Standard-library-only Go message bindings for any wire v2 server adapter.

use std::{collections::BTreeSet, fmt::Write};

use anyhow::{Result, ensure};

use super::{FieldType, Project};

/// Emits typed Go IDs, message structs and codecs from the same schema as Rust.
pub fn generate(project: &Project) -> Result<String> {
    let mut out = String::from(
        "// Code generated from src/shared.rs; DO NOT EDIT.\npackage messages\n\nimport (\n\"bytes\"\n\"encoding/json\"\n\"fmt\"\n\"io\"\n\"reflect\"\n\"strconv\"\n\"strings\"\n\"unicode/utf8\"\n)\n\n",
    );
    writeln!(
        out,
        "// PackageID is the owning bundle's stable identity.\nconst PackageID = {:?}",
        project.id
    )?;
    writeln!(
        out,
        "// RegistryFingerprint identifies all shared message and typed ID definitions.\nconst RegistryFingerprint = {:?}\n// RegistryChannel must be confirmed before authored client callbacks start.\nconst RegistryChannel = {:?}",
        project.fingerprint(),
        format!("{}.registry", project.id)
    )?;
    writeln!(
        out,
        "const registryFields = {:?}",
        serde_json::to_string(&project.channels().last().expect("registry channel")["fields"])?
    )?;
    let mut names = BTreeSet::from([
        "PackageID".to_owned(),
        "RegistryConfirmation".to_owned(),
        "RegistryChannel".to_owned(),
        "RegistryFingerprint".to_owned(),
        "Message".to_owned(),
        "EntityID".to_owned(),
        "WorldID".to_owned(),
        "ItemID".to_owned(),
    ]);
    for kind in ["EntityID", "WorldID", "ItemID"] {
        writeln!(
            out,
            "// {kind} is a stable namespaced definition key.\ntype {kind} string\n// MarshalJSON rejects malformed stable identifiers.\nfunc (v {kind}) MarshalJSON() ([]byte,error) {{ if !validID(string(v)) {{ return nil, fmt.Errorf(\"invalid typed ID\") }}; return json.Marshal(string(v)) }}\n// UnmarshalJSON checks the key before assigning its domain type.\nfunc (v *{kind}) UnmarshalJSON(raw []byte) error {{ var s string; if err := json.Unmarshal(raw, &s); err != nil {{ return err }}; if !validID(s) {{ return fmt.Errorf(\"invalid typed ID\") }}; *v = {kind}(s); return nil }}"
        )?;
    }
    for id in &project.definitions {
        let name = exported(&id.name);
        ensure!(
            !name.is_empty() && name.is_ascii(),
            "Go IDs need portable source names"
        );
        ensure!(
            names.insert(name.clone()),
            "generated Go name collision: {name}"
        );
        writeln!(
            out,
            "// {name} is the shared stable definition key.\nconst {name} {} = {:?}",
            id.kind.replace("Id", "ID"),
            id.value
        )?;
    }
    for message in &project.messages {
        let name = exported(&message.source.ident.to_string());
        ensure!(
            !name.is_empty() && name.is_ascii(),
            "Go messages need portable source names"
        );
        ensure!(
            names.insert(name.clone()),
            "generated Go name collision: {name}"
        );
        let mut field_names = BTreeSet::from([
            "ChannelID".to_owned(),
            "SchemaVersion".to_owned(),
            "EncodeRecord".to_owned(),
            "DecodeRecord".to_owned(),
        ]);
        let fields = message
            .source
            .fields
            .iter()
            .map(|field| exported(&field.ident.as_ref().expect("named field").to_string()))
            .collect::<Vec<_>>();
        writeln!(
            out,
            "// {name} is the typed record for {}.\ntype {name} struct {{",
            message.id
        )?;
        for (field, ty) in fields.iter().zip(&message.fields) {
            ensure!(
                !field.is_empty() && field.is_ascii(),
                "Go fields need portable source names"
            );
            ensure!(
                field_names.insert(field.clone()),
                "generated Go field or method collision: {name}.{field}"
            );
            writeln!(out, "{field} {}", go_type(ty))?;
        }
        writeln!(
            out,
            "}}\n// ChannelID selects this message's declared channel.\nfunc ({name}) ChannelID() string {{ return {:?} }}\n// SchemaVersion selects this message's declared revision.\nfunc ({name}) SchemaVersion() uint16 {{ return {} }}",
            message.id, message.schema
        )?;
        let schema = serde_json::to_string(
            &message
                .fields
                .iter()
                .map(FieldType::declaration)
                .collect::<Vec<_>>(),
        )?;
        let values = fields
            .iter()
            .map(|name| format!(", v.{name}"))
            .collect::<String>();
        let pointers = fields
            .iter()
            .map(|name| format!(", &decoded.{name}"))
            .collect::<String>();
        writeln!(
            out,
            "// EncodeRecord validates bounds and returns a wire v2 positional payload.\nfunc (v {name}) EncodeRecord() ([]byte,error) {{ return encodeRecord({schema:?}{values}) }}\n// DecodeRecord assigns this message only after all fields validate.\nfunc (v *{name}) DecodeRecord(raw []byte) error {{ var decoded {name}; if err := decodeRecord(raw, {schema:?}{pointers}); err != nil {{ return err }}; *v = decoded; return nil }}"
        )?;
    }
    out.push_str(include_str!("codec.go.txt"));
    Ok(out)
}

/// Converts a Rust source name to a stable exported Go identifier.
fn exported(name: &str) -> String {
    name.trim_start_matches("r#")
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            if part.bytes().all(|b| !b.is_ascii_lowercase()) {
                let lower = part.to_ascii_lowercase();
                let mut chars = lower.chars();
                chars
                    .next()
                    .map(|c| c.to_ascii_uppercase().to_string())
                    .unwrap_or_default()
                    + chars.as_str()
            } else {
                let mut chars = part.chars();
                chars
                    .next()
                    .map(|c| c.to_ascii_uppercase().to_string())
                    .unwrap_or_default()
                    + chars.as_str()
            }
        })
        .collect()
}

/// The native Go type corresponding to a supported Rust field.
fn go_type(ty: &FieldType) -> String {
    match ty {
        FieldType::Bool => "bool".into(),
        FieldType::Integer { rust, .. } => rust.replacen('i', "int", 1).replacen('u', "uint", 1),
        FieldType::Text { id, .. } => id
            .as_ref()
            .map_or_else(|| "string".into(), |id| id.replace("Id", "ID")),
        FieldType::List { item, .. } => format!("[]{}", go_type(item)),
    }
}
