//! Rust bindings generated from the checked project model.

use std::fmt::Write;

use anyhow::Result;
use quote::ToTokens;

use super::{FieldType, Project};

/// Emits source definitions, exact channel schemas and bounded positional codecs.
pub fn generate(project: &Project) -> Result<String> {
    let mut out = String::from("// Generated from src/shared.rs; do not edit.\n");
    writeln!(
        out,
        "/// The owning bundle's stable identity.\npub const PACKAGE_ID: &str = {:?};",
        project.id
    )?;
    for item in &project.items {
        writeln!(out, "{}", item.to_token_stream())?;
    }
    for message in &project.messages {
        writeln!(out, "{}", message.source.to_token_stream())?;
        let name = &message.source.ident;
        let fields = message
            .fields
            .iter()
            .map(field)
            .collect::<Vec<_>>()
            .join(",");
        writeln!(
            out,
            "impl experience_sdk::message::Message for {name} {{\nconst CHANNEL: experience_sdk::Channel = experience_sdk::Channel {{ id: {:?}, schema: {}, direction: experience_sdk::Direction::{}, fields: &[{fields}] }};",
            message.id,
            message.schema,
            if message.to_server {
                "ToServer"
            } else {
                "ToClient"
            }
        )?;
        writeln!(
            out,
            "/// Encodes and validates this message's positional fields.\nfn encode(&self) -> Result<Vec<experience_sdk::Value>, String> {{\nlet record = vec![{}];\nexperience_sdk::message::validate(&Self::CHANNEL, &record)?;\nOk(record)\n}}",
            message
                .source
                .fields
                .iter()
                .map(|f| format!(
                    "experience_sdk::message::WireValue::to_value(&self.{})",
                    f.ident.as_ref().expect("named fields")
                ))
                .collect::<Vec<_>>()
                .join(",")
        )?;
        let iterator = if message.fields.is_empty() {
            ""
        } else {
            "let mut fields = record.into_iter();"
        };
        writeln!(out, "/// Validates and decodes exactly this message's positional fields.\nfn decode(record: Vec<experience_sdk::Value>) -> Result<Self, String> {{\nexperience_sdk::message::validate(&Self::CHANNEL, &record)?;\n{iterator}\nOk(Self {{ {} }})\n}}\n}}", message.source.fields.iter().map(|f| format!("{}: experience_sdk::message::WireValue::from_value(fields.next().ok_or(\"missing message field\")?)?", f.ident.as_ref().expect("named fields"))).collect::<Vec<_>>().join(","))?;
    }
    let fingerprint = project.fingerprint();
    writeln!(
        out,
        "/// The complete shared definition digest confirmed by the server.\npub const REGISTRY_FINGERPRINT: &str = {fingerprint:?};\n/// The confirmation required before authored client callbacks start.\npub static REGISTRY: experience_sdk::message::Registry = experience_sdk::message::Registry {{ fingerprint: REGISTRY_FINGERPRINT, channel: experience_sdk::Channel {{ id: {:?}, schema: 1, direction: experience_sdk::Direction::ToClient, fields: &[experience_sdk::Field::Text {{ max_bytes: {} }}] }} }};",
        format!("{}.registry", project.id),
        fingerprint.len()
    )?;
    Ok(out)
}

/// Emits the SDK field declaration directly from the same model used by Go and the manifest.
fn field(ty: &FieldType) -> String {
    let value = match ty {
        FieldType::Bool => "Bool".into(),
        FieldType::Integer { min, max, .. } => format!("Integer {{ min: {min}, max: {max} }}"),
        FieldType::Text { max_bytes, .. } => format!("Text {{ max_bytes: {max_bytes} }}"),
        FieldType::List { item, max_items } => {
            format!("List {{ item: &{}, max_items: {max_items} }}", field(item))
        }
    };
    format!("experience_sdk::Field::{value}")
}
