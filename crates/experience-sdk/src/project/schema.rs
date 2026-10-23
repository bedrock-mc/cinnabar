//! The checked source model shared by the Rust and Go generators.

use std::collections::BTreeSet;

use anyhow::{Context, Result, bail, ensure};
use quote::ToTokens;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use syn::{Attribute, Expr, Fields, Item, LitStr, Type};

/// One supported bounded field, with its Rust type retained for generated bindings.
#[derive(Clone, Debug)]
pub enum FieldType {
    Bool,
    Integer {
        rust: String,
        min: i64,
        max: i64,
    },
    Text {
        max_bytes: u16,
        id: Option<String>,
    },
    List {
        item: Box<FieldType>,
        max_items: u16,
    },
}

/// A stable typed ID declared in the shared source.
pub struct Definition {
    pub name: String,
    pub kind: String,
    pub value: String,
}

/// A checked message and its source definition, with generation attributes removed.
pub struct Message {
    pub source: syn::ItemStruct,
    pub id: String,
    pub schema: u16,
    pub to_server: bool,
    pub fields: Vec<FieldType>,
}

/// A single source-driven project, including its original shared Rust items.
pub struct Project {
    pub id: String,
    pub permissions: BTreeSet<String>,
    pub messages: Vec<Message>,
    pub definitions: Vec<Definition>,
    pub items: Vec<Item>,
}

impl Project {
    /// Reads the annotated shared module and rejects ambiguous or unsupported declarations.
    pub fn parse(source: &str) -> Result<Self> {
        let file = syn::parse_file(source)?;
        ensure!(
            file.attrs.len() == 1 && file.attrs[0].path().is_ident("project"),
            "shared.rs needs one #![project(id = ..., permissions(...))] declaration"
        );
        let mut project = Self {
            id: String::new(),
            permissions: BTreeSet::new(),
            messages: Vec::new(),
            definitions: Vec::new(),
            items: Vec::new(),
        };
        file.attrs[0].parse_nested_meta(|meta| {
            if meta.path.is_ident("id") && project.id.is_empty() {
                project.id = meta.value()?.parse::<LitStr>()?.value();
            } else if meta.path.is_ident("permissions") {
                meta.parse_nested_meta(|permission| {
                    let name = permission
                        .path
                        .get_ident()
                        .ok_or_else(|| permission.error("expected permission name"))?
                        .to_string();
                    if !project.permissions.insert(name) {
                        return Err(permission.error("duplicate permission"));
                    }
                    Ok(())
                })?;
            } else {
                return Err(meta.error("unknown or repeated project option"));
            }
            Ok(())
        })?;
        ensure!(
            valid_name(&project.id),
            "project ID must start with a lowercase letter and contain only lowercase letters, digits and underscores"
        );
        project.permissions.insert("messaging".into());
        let mut names = BTreeSet::new();
        let mut ids = BTreeSet::new();
        ids.insert(format!("{}.registry", project.id));
        for item in file.items {
            match item {
                Item::Struct(mut source) => {
                    ensure!(
                        source.generics.params.is_empty() && source.generics.where_clause.is_none(),
                        "messages cannot be generic"
                    );
                    let attributes = take(&mut source.attrs, "message");
                    ensure!(
                        attributes.len() == 1,
                        "{} needs one #[message(...)]",
                        source.ident
                    );
                    ensure!(
                        source
                            .attrs
                            .iter()
                            .all(|a| a.path().is_ident("derive") || a.path().is_ident("doc")),
                        "conditional message definitions are unsupported"
                    );
                    let mut id = None;
                    let mut schema = None;
                    let mut to_server = None;
                    attributes[0].parse_nested_meta(|meta| {
                        if meta.path.is_ident("id") && id.is_none() {
                            id = Some(meta.value()?.parse::<LitStr>()?.value());
                        } else if meta.path.is_ident("schema") && schema.is_none() {
                            schema = Some(
                                meta.value()?
                                    .parse::<syn::LitInt>()?
                                    .base10_parse::<u16>()?,
                            );
                        } else if (meta.path.is_ident("to_server")
                            || meta.path.is_ident("to_client"))
                            && to_server.is_none()
                        {
                            to_server = Some(meta.path.is_ident("to_server"));
                        } else {
                            return Err(meta.error("unknown or repeated message option"));
                        }
                        Ok(())
                    })?;
                    let id = id.context("message needs id")?;
                    ensure!(
                        id.starts_with(&format!("{}.", project.id)) && portable(&id),
                        "message ID must belong to the project namespace"
                    );
                    ensure!(
                        names.insert(source.ident.to_string()) && ids.insert(id.clone()),
                        "duplicate message name or ID"
                    );
                    let Fields::Named(fields) = &mut source.fields else {
                        bail!("messages need named fields");
                    };
                    let mut types = Vec::new();
                    for field in &mut fields.named {
                        let attrs = take(&mut field.attrs, "field");
                        ensure!(
                            field.attrs.iter().all(|a| a.path().is_ident("doc")),
                            "unsupported message field attribute"
                        );
                        let bounds = Bounds::parse(&attrs)?;
                        types.push(field_type(&field.ty, &bounds, false).with_context(|| {
                            format!("{} field {:?}", source.ident, field.ident)
                        })?);
                    }
                    project.messages.push(Message {
                        source,
                        id,
                        schema: schema.context("message needs schema")?,
                        to_server: to_server.context("message needs to_server or to_client")?,
                        fields: types,
                    });
                }
                Item::Const(constant) => {
                    ensure!(
                        constant.attrs.iter().all(|a| a.path().is_ident("doc")),
                        "conditional ID constants are unsupported"
                    );
                    let kind = constant.ty.to_token_stream().to_string();
                    ensure!(
                        matches!(kind.as_str(), "EntityId" | "WorldId" | "ItemId"),
                        "shared constants must use EntityId, WorldId or ItemId"
                    );
                    let Expr::Macro(value) = constant.expr.as_ref() else {
                        bail!("typed ID constants must use id!(...)");
                    };
                    ensure!(
                        value.mac.path.is_ident("id"),
                        "typed ID constants must use id!(...)"
                    );
                    let value = syn::parse2::<LitStr>(value.mac.tokens.clone())?.value();
                    ensure!(
                        crate::message::valid_id(&value)
                            && value.starts_with(&format!("{}:", project.id)),
                        "typed ID must belong to the project namespace"
                    );
                    ensure!(
                        names.insert(constant.ident.to_string()) && ids.insert(value.clone()),
                        "duplicate typed ID"
                    );
                    project.definitions.push(Definition {
                        name: constant.ident.to_string(),
                        kind,
                        value,
                    });
                    project.items.push(Item::Const(constant));
                }
                Item::Use(import) => {
                    ensure!(
                        import.attrs.is_empty(),
                        "conditional shared imports are unsupported"
                    );
                    project.items.push(Item::Use(import));
                }
                _ => bail!("shared.rs supports imports, typed ID constants and #[message] structs"),
            }
        }
        project.messages.sort_by(|a, b| a.id.cmp(&b.id));
        project.definitions.sort_by(|a, b| a.value.cmp(&b.value));
        Ok(project)
    }

    /// The complete definition schema; names, ordered fields and typed ID values affect its digest.
    pub fn document(&self) -> Value {
        let messages = self
            .messages
            .iter()
            .map(|message| {
                let fields = message
                    .source
                    .fields
                    .iter()
                    .map(|field| {
                        json!({"name": field.ident.as_ref().map(ToString::to_string),
                    "rust_type": field.ty.to_token_stream().to_string()})
                    })
                    .collect::<Vec<_>>();
                json!({"name": message.source.ident.to_string(),
                "channel": message.channel(), "fields": fields})
            })
            .collect::<Vec<_>>();
        let ids = self
            .definitions
            .iter()
            .map(|id| json!({"name": id.name, "kind": id.kind, "value": id.value}))
            .collect::<Vec<_>>();
        json!({"format": 1, "id": self.id, "messages": messages, "ids": ids})
    }

    /// SHA-256 of the canonical generated registry, independent of comments and item order.
    pub fn fingerprint(&self) -> String {
        format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&canonical(self.document())).expect("registry serializes")
            )
        )
    }

    /// All signed channels, including the confirmation required before authored code starts.
    pub fn channels(&self) -> Vec<Value> {
        let mut channels: Vec<_> = self.messages.iter().map(Message::channel).collect();
        channels.push(json!({"id": format!("{}.registry", self.id), "schema": 1, "direction": "to_client", "fields": [{"type": "text", "max_bytes": self.fingerprint().len()}]}));
        channels
    }
}

impl Message {
    /// The existing wire channel declaration derived from this message.
    fn channel(&self) -> Value {
        json!({"id": self.id, "schema": self.schema, "direction": if self.to_server { "to_server" } else { "to_client" }, "fields": self.fields.iter().map(FieldType::declaration).collect::<Vec<_>>()})
    }
}

impl FieldType {
    /// The existing wire field declaration, with no SDK-specific envelope fields.
    pub fn declaration(&self) -> Value {
        match self {
            Self::Bool => json!({"type": "bool"}),
            Self::Integer { min, max, .. } => json!({"type": "integer", "min": min, "max": max}),
            Self::Text { max_bytes, .. } => json!({"type": "text", "max_bytes": max_bytes}),
            Self::List { item, max_items } => {
                json!({"type": "list", "item": item.declaration(), "max_items": max_items})
            }
        }
    }
}

#[derive(Default)]
struct Bounds {
    min: Option<i64>,
    max: Option<i64>,
    max_bytes: Option<u16>,
    max_items: Option<u16>,
}

impl Bounds {
    /// Reads each field bound once and rejects unknown options.
    fn parse(attributes: &[Attribute]) -> Result<Self> {
        let mut bounds = Self::default();
        for attr in attributes {
            attr.parse_nested_meta(|meta| {
                macro_rules! bound {
                    ($name:ident) => {
                        if meta.path.is_ident(stringify!($name)) && bounds.$name.is_none() {
                            bounds.$name = Some(
                                meta.value()?
                                    .parse::<syn::Expr>()?
                                    .to_token_stream()
                                    .to_string()
                                    .replace(' ', "")
                                    .parse()
                                    .map_err(|_| meta.error("expected an integer bound"))?,
                            );
                            return Ok(());
                        }
                    };
                }
                bound!(min);
                bound!(max);
                bound!(max_bytes);
                bound!(max_items);
                Err(meta.error("unknown or repeated field bound"))
            })?;
        }
        Ok(bounds)
    }
}

/// Maps one supported Rust field to its exact bounded wire type.
fn field_type(ty: &Type, bounds: &Bounds, in_list: bool) -> Result<FieldType> {
    let Type::Path(path) = ty else {
        bail!("unsupported field type");
    };
    ensure!(
        path.qself.is_none() && path.path.segments.len() == 1,
        "use an unqualified supported field type"
    );
    let segment = &path.path.segments[0];
    let name = segment.ident.to_string();
    if name == "Vec" {
        ensure!(!in_list, "nested Vec fields need a separate message");
        let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
            bail!("Vec needs an item type");
        };
        let Some(syn::GenericArgument::Type(item)) = args.args.first() else {
            bail!("Vec needs an item type");
        };
        ensure!(args.args.len() == 1, "Vec needs one item type");
        return Ok(FieldType::List {
            item: Box::new(field_type(item, bounds, true)?),
            max_items: bounds
                .max_items
                .context("Vec needs #[field(max_items = ...)]")?,
        });
    }
    ensure!(
        segment.arguments.is_empty(),
        "unsupported generic field type"
    );
    ensure!(
        in_list || bounds.max_items.is_none(),
        "max_items only applies to Vec"
    );
    let range = match name.as_str() {
        "i8" => Some((i64::from(i8::MIN), i64::from(i8::MAX))),
        "i16" => Some((i64::from(i16::MIN), i64::from(i16::MAX))),
        "i32" => Some((i64::from(i32::MIN), i64::from(i32::MAX))),
        "i64" => Some((i64::MIN, i64::MAX)),
        "u8" => Some((0, i64::from(u8::MAX))),
        "u16" => Some((0, i64::from(u16::MAX))),
        "u32" => Some((0, i64::from(u32::MAX))),
        _ => None,
    };
    if let Some((low, high)) = range {
        ensure!(bounds.max_bytes.is_none(), "max_bytes only applies to text");
        let min = bounds.min.unwrap_or(low);
        let max = bounds.max.unwrap_or(high);
        ensure!(
            low <= min && min <= max && max <= high,
            "integer bounds exceed the Rust type"
        );
        return Ok(FieldType::Integer {
            rust: name,
            min,
            max,
        });
    }
    ensure!(
        bounds.min.is_none() && bounds.max.is_none(),
        "min and max only apply to integers"
    );
    match name.as_str() {
        "bool" => {
            ensure!(bounds.max_bytes.is_none(), "max_bytes only applies to text");
            Ok(FieldType::Bool)
        }
        "String" | "EntityId" | "WorldId" | "ItemId" => Ok(FieldType::Text {
            max_bytes: bounds
                .max_bytes
                .context("text and IDs need #[field(max_bytes = ...)]")?,
            id: (name != "String").then_some(name),
        }),
        _ => bail!("unsupported message field {name}"),
    }
}

/// Removes only the generator-owned attributes, preserving ordinary Rust documentation.
fn take(attributes: &mut Vec<Attribute>, name: &str) -> Vec<Attribute> {
    let mut found = Vec::new();
    attributes.retain(|attr| {
        if attr.path().is_ident(name) {
            found.push(attr.clone());
            false
        } else {
            true
        }
    });
    found
}

/// The wire's portable lowercase identifier alphabet; host limits are checked during packaging.
fn portable(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b":_-.".contains(&b))
}

/// Whether a project name is both a portable package directory and a Rust/Go namespace.
pub fn valid_name(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Sorts every object so Cargo feature unification cannot alter the registry's digest.
fn canonical(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries = object.into_iter().collect::<Vec<_>>();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonical(value)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonical).collect()),
        value => value,
    }
}
