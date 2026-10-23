//! Go bindings use the server helper's native record types and canonical wire codec.

use std::{collections::BTreeSet, fmt::Write};

use anyhow::{Result, ensure};

use super::{FieldType, Project};

const HELPER_MODULE: &str = "github.com/hashimthearab/rust-mcbe/tools/localserver";
const HELPER_SOURCE: &str = "github.com/cinnabar-mc/cinnabar/tools/localserver";
const HELPER_VERSION: &str = "v0.0.0-20261023202529-c66a1481fe2a";

/// Pins the public helper contract and its supported upstream Dragonfly dependency.
pub fn module(id: &str) -> String {
    format!(
        "module cinnabar.local/{id}/messages\n\ngo 1.26.1\n\nrequire (\n {HELPER_MODULE} {HELPER_VERSION}\n github.com/df-mc/dragonfly v0.11.5\n)\n\nreplace {HELPER_MODULE} => {HELPER_SOURCE} {HELPER_VERSION}\n"
    )
}

/// Emits IDs, native typed messages, receiver descriptors and canonical payload codecs.
pub fn generate(project: &Project) -> Result<String> {
    let mut out = format!(
        "// Code generated from src/shared.rs; DO NOT EDIT.\npackage messages\n\nimport (\n\"fmt\"\n\"strings\"\n\"unicode/utf8\"\n\"github.com/google/uuid\"\ncinnabarext \"{HELPER_MODULE}/cinnabarext\"\n)\n\n"
    );
    writeln!(
        out,
        "// PackageID is the owning bundle's stable identity.\nconst PackageID = {:?}\n// RegistryFingerprint identifies all shared message and typed ID definitions.\nconst RegistryFingerprint = {:?}\n// RegistryChannel must be confirmed before authored client callbacks start.\nconst RegistryChannel = {:?}",
        project.id,
        project.fingerprint(),
        format!("{}.registry", project.id)
    )?;
    writeln!(
        out,
        "// RegistryRequestChannel starts the generated definition handshake.\nconst RegistryRequestChannel = {:?}",
        format!("{}.registry_request", project.id)
    )?;
    let mut names = BTreeSet::from(
        [
            "PackageID",
            "RegistryConfirmation",
            "RegistryChannel",
            "RegistryRequestChannel",
            "RegistryFingerprint",
            "RegisterRegistry",
            "EntityID",
            "WorldID",
            "ItemID",
        ]
        .map(str::to_owned),
    );
    for kind in ["EntityID", "WorldID", "ItemID"] {
        writeln!(
            out,
            "// {kind} is a stable namespaced definition key.\ntype {kind} string"
        )?;
    }
    for id in &project.definitions {
        let name = exported(&id.name);
        reserve(&mut names, &name)?;
        writeln!(
            out,
            "// {name} is the shared stable definition key.\nconst {name} {} = {:?}",
            id.kind.replace("Id", "ID"),
            id.value
        )?;
    }
    for message in &project.messages {
        let name = exported(&message.source.ident.to_string());
        for symbol in [&name, &format!("Decode{name}"), &format!("{name}Type")] {
            reserve(&mut names, symbol)?;
        }
        let mut field_names =
            BTreeSet::from(["Route", "Encode", "EncodeRecord", "DecodeRecord"].map(str::to_owned));
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
            reserve(&mut field_names, field)?;
            writeln!(out, "{field} {}", go_type(ty))?;
        }
        writeln!(
            out,
            "}}\n// Route selects this message's generated bundle, channel and revision.\nfunc ({name}) Route() cinnabarext.Route {{ return cinnabarext.Route{{Bundle: PackageID, Channel: {:?}, Schema: {}}} }}",
            message.id, message.schema
        )?;
        writeln!(
            out,
            "// Encode validates the typed fields and builds the helper's native record.\nfunc (v {name}) Encode() (cinnabarext.Record,error) {{\nrecord := cinnabarext.Record{{{}}}\nif err := validate{name}(record); err != nil {{ return nil, err }}\nreturn record, nil\n}}",
            fields
                .iter()
                .zip(&message.fields)
                .map(|(field, ty)| scalar(ty, &format!("v.{field}")))
                .collect::<Vec<_>>()
                .join(",")
        )?;
        writeln!(
            out,
            "// Decode{name} validates native records for typed receive callbacks.\nfunc Decode{name}(record cinnabarext.Record) ({name},error) {{\nvar decoded {name}\nif _, err := cinnabarext.Encode(record); err != nil {{ return decoded, err }}\nif err := validate{name}(record); err != nil {{ return decoded, err }}"
        )?;
        for (index, (field, ty)) in fields.iter().zip(&message.fields).enumerate() {
            writeln!(
                out,
                "decoded.{field} = {}",
                decoded(ty, &format!("record[{index}]"))
            )?;
        }
        writeln!(
            out,
            "return decoded, nil\n}}\n// {name}Type registers this generated route and decoder with OnMessage.\nvar {name}Type = cinnabarext.MessageType[{name}]{{Route: {name}{{}}.Route(), Decode: Decode{name}}}\nvar _ cinnabarext.Message = {name}{{}}"
        )?;
        writeln!(
            out,
            "// validate{name} checks the exact positional types and every source-defined bound.\nfunc validate{name}(record cinnabarext.Record) error {{\nif err := checkFieldCount(record, {}); err != nil {{ return err }}",
            fields.len()
        )?;
        for (index, ty) in message.fields.iter().enumerate() {
            writeln!(
                out,
                "if !({}) {{ return fmt.Errorf(\"{} field {}: type or bounds mismatch\") }}",
                admits(ty, &format!("record[{index}]")),
                message.id,
                index
            )?;
        }
        writeln!(
            out,
            "return nil\n}}\n// EncodeRecord returns the canonical wire v2 payload for other transports.\nfunc (v {name}) EncodeRecord() ([]byte,error) {{ record, err := v.Encode(); if err != nil {{ return nil, err }}; return cinnabarext.Encode(record) }}\n// DecodeRecord assigns a message only after canonical parsing and full validation.\nfunc (v *{name}) DecodeRecord(raw []byte) error {{ var record cinnabarext.Record; if err := cinnabarext.Decode(raw, &record); err != nil {{ return err }}; decoded, err := Decode{name}(record); if err != nil {{ return err }}; *v = decoded; return nil }}"
        )?;
    }
    out.push_str(include_str!("codec.go.txt"));
    Ok(out)
}

/// Rejects names that would produce ambiguous or invalid generated Go declarations.
fn reserve(names: &mut BTreeSet<String>, name: &str) -> Result<()> {
    ensure!(
        name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
            && name.bytes().all(|byte| byte.is_ascii_alphanumeric()),
        "invalid generated Go identifier {name:?}: names must start with a letter and contain only ASCII letters and digits"
    );
    ensure!(
        names.insert(name.to_owned()),
        "generated Go name collision: {name}"
    );
    Ok(())
}

/// Converts a Rust source name to a stable exported Go identifier.
fn exported(name: &str) -> String {
    name.trim_start_matches("r#")
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let normalized = if part.bytes().all(|b| !b.is_ascii_lowercase()) {
                part.to_ascii_lowercase()
            } else {
                part.to_owned()
            };
            let mut chars = normalized.chars();
            chars
                .next()
                .map(|c| c.to_ascii_uppercase().to_string())
                .unwrap_or_default()
                + chars.as_str()
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

/// Builds the helper's scalar variant directly, without a second JSON representation.
fn scalar(ty: &FieldType, value: &str) -> String {
    let field = match ty {
        FieldType::Bool => format!("Bool: pointer({value})"),
        FieldType::Integer { .. } => format!("Integer: pointer(int64({value}))"),
        FieldType::Text { .. } => format!("Text: pointer(string({value}))"),
        FieldType::List { item, .. } => {
            return format!(
                "func() cinnabarext.Scalar {{ items := make([]cinnabarext.Scalar,len({value})); for i,item := range {value} {{ items[i] = {} }}; return cinnabarext.Scalar{{List: &items}} }}()",
                scalar(item, "item")
            );
        }
    };
    format!("cinnabarext.Scalar{{{field}}}")
}

/// Decodes a scalar only after its generated validator checked the tag and bounds.
fn decoded(ty: &FieldType, value: &str) -> String {
    match ty {
        FieldType::Bool => format!("*{value}.Bool"),
        FieldType::Integer { .. } => format!("{}(*{value}.Integer)", go_type(ty)),
        FieldType::Text { id: None, .. } => format!("*{value}.Text"),
        FieldType::Text { .. } => format!("{}(*{value}.Text)", go_type(ty)),
        FieldType::List { item, .. } => format!(
            "func() {} {{ items := make({},len(*{value}.List)); for i,item := range *{value}.List {{ items[i] = {} }}; return items }}()",
            go_type(ty),
            go_type(ty),
            decoded(item, "item")
        ),
    }
}

/// Emits the bounds predicate from the same field model used by Rust and the signed manifest.
fn admits(ty: &FieldType, value: &str) -> String {
    match ty {
        FieldType::Bool => format!("{value}.Bool != nil"),
        FieldType::Integer { min, max, .. } => format!(
            "{value}.Integer != nil && *{value}.Integer >= {min} && *{value}.Integer <= {max}"
        ),
        FieldType::Text { max_bytes, id } => format!(
            "{value}.Text != nil && validText(*{value}.Text,{max_bytes}){}",
            if id.is_some() {
                format!(" && validID(*{value}.Text)")
            } else {
                String::new()
            }
        ),
        FieldType::List { item, max_items } => format!(
            "{value}.List != nil && len(*{value}.List) <= {max_items} && func() bool {{ for _,item := range *{value}.List {{ if !({}) {{ return false }} }}; return true }}()",
            admits(item, "item")
        ),
    }
}
