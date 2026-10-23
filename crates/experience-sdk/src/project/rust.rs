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
        "/// The owning bundle's stable identity.\npub const PACKAGE_ID: &::core::primitive::str = {:?};",
        project.id
    )?;
    for item in &project.items {
        writeln!(out, "{}", item.to_token_stream())?;
    }
    for message in &project.messages {
        let mut source = message.source.clone();
        for (field, ty) in source.fields.iter_mut().zip(&message.fields) {
            field.ty = rust_type(ty);
        }
        writeln!(out, "{}", source.to_token_stream())?;
        let name = &message.source.ident;
        let fields = message
            .fields
            .iter()
            .map(field)
            .collect::<Vec<_>>()
            .join(",");
        writeln!(
            out,
            "impl ::experience_sdk::message::Message for {name} {{\nconst CHANNEL: ::experience_sdk::Channel = ::experience_sdk::Channel {{ id: {:?}, schema: {}, direction: ::experience_sdk::Direction::{}, fields: &[{fields}] }};",
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
            "/// Encodes and validates this message's positional fields.\nfn encode(&self) -> ::core::result::Result<::std::vec::Vec<::experience_sdk::Value>, ::std::string::String> {{\nlet record = ::std::vec![{}];\n::experience_sdk::message::validate(&Self::CHANNEL, &record)?;\n::core::result::Result::Ok(record)\n}}",
            message
                .source
                .fields
                .iter()
                .map(|f| format!(
                    "::experience_sdk::message::WireValue::to_value(&self.{})",
                    f.ident.as_ref().expect("named fields")
                ))
                .collect::<Vec<_>>()
                .join(",")
        )?;
        let iterator = if message.fields.is_empty() {
            ""
        } else {
            "let mut fields = ::core::iter::IntoIterator::into_iter(record);"
        };
        writeln!(out, "/// Validates and decodes exactly this message's positional fields.\nfn decode(record: ::std::vec::Vec<::experience_sdk::Value>) -> ::core::result::Result<Self, ::std::string::String> {{\n::experience_sdk::message::validate(&Self::CHANNEL, &record)?;\n{iterator}\n::core::result::Result::Ok(Self {{ {} }})\n}}\n}}", message.source.fields.iter().map(|f| format!("{}: ::experience_sdk::message::WireValue::from_value(::core::iter::Iterator::next(&mut fields).ok_or(\"missing message field\")?)?", f.ident.as_ref().expect("named fields"))).collect::<Vec<_>>().join(","))?;
    }
    let fingerprint = project.fingerprint();
    writeln!(
        out,
        "/// The complete shared definition digest confirmed by the server.\npub const REGISTRY_FINGERPRINT: &::core::primitive::str = {fingerprint:?};\n/// The confirmation required before authored client callbacks start.\npub static REGISTRY: ::experience_sdk::message::Registry = ::experience_sdk::message::Registry {{ fingerprint: REGISTRY_FINGERPRINT, channel: ::experience_sdk::Channel {{ id: {:?}, schema: 1, direction: ::experience_sdk::Direction::ToClient, fields: &[::experience_sdk::Field::Text {{ max_bytes: {} }}] }}, request: ::experience_sdk::Channel {{ id: {:?}, schema: 1, direction: ::experience_sdk::Direction::ToServer, fields: &[::experience_sdk::Field::Text {{ max_bytes: {} }}] }} }};",
        format!("{}.registry", project.id),
        fingerprint.len(),
        format!("{}.registry_request", project.id),
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
    format!("::experience_sdk::Field::{value}")
}

/// Resolves supported field types independently of authored names and imports.
fn rust_type(ty: &FieldType) -> syn::Type {
    match ty {
        FieldType::Bool => syn::parse_quote!(::core::primitive::bool),
        FieldType::Integer { rust, .. } => {
            let name = quote::format_ident!("{rust}");
            syn::parse_quote!(::core::primitive::#name)
        }
        FieldType::Text { id: None, .. } => syn::parse_quote!(::std::string::String),
        FieldType::Text { id: Some(id), .. } => {
            let name = quote::format_ident!("{id}");
            syn::parse_quote!(::experience_sdk::message::#name)
        }
        FieldType::List { item, .. } => {
            let item = rust_type(item);
            syn::parse_quote!(::std::vec::Vec<#item>)
        }
    }
}
