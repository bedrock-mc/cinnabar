//! Content declarations lowered to the existing server registration API.

use anyhow::{Context, Result, ensure};
use quote::ToTokens;
use syn::{Attribute, Expr, ExprPath, LitStr};

/// A block or item declaration attached to its stable typed ID.
pub struct Content {
    pub display_name: String,
    pub asset: ExprPath,
    pub hardness: Option<f32>,
    pub max_stack: u8,
}

impl Content {
    /// Reads typed asset references and checks options for the declared content kind.
    pub fn parse(attribute: &Attribute, kind: &str) -> Result<Self> {
        let block = attribute.path().is_ident("block");
        ensure!(
            kind == if block { "BlockId" } else { "ItemId" },
            "content attribute and typed ID kind disagree"
        );
        let (mut name, mut asset, mut hardness, mut stack) = (None, None, None, None);
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") && name.is_none() {
                name = Some(meta.value()?.parse::<LitStr>()?.value());
            } else if meta.path.is_ident(if block { "texture" } else { "icon" }) && asset.is_none()
            {
                asset = Some(meta.value()?.parse::<ExprPath>()?);
            } else if block && meta.path.is_ident("hardness") && hardness.is_none() {
                let value = meta
                    .value()?
                    .parse::<Expr>()?
                    .to_token_stream()
                    .to_string()
                    .replace(' ', "");
                hardness = Some(
                    value
                        .parse::<f32>()
                        .map_err(|_| meta.error("expected hardness number"))?,
                );
            } else if !block && meta.path.is_ident("max_stack") && stack.is_none() {
                stack = Some(meta.value()?.parse::<syn::LitInt>()?.base10_parse::<u8>()?);
            } else {
                return Err(meta.error("unknown or repeated content option"));
            }
            Ok(())
        })?;
        let display_name = name.context("content needs name")?;
        ensure!(
            !display_name.is_empty() && !display_name.chars().any(char::is_control),
            "invalid content name"
        );
        let asset = asset.context("content needs a generated texture or icon asset constant")?;
        ensure!(
            asset.path.segments.len() == 2 && asset.path.segments[0].ident == "assets",
            "use assets::ASSET_<PATH> for content assets"
        );
        ensure!(
            hardness.is_none_or(|h| h.is_finite() && h >= 0.0),
            "hardness must be finite and nonnegative"
        );
        let max_stack = stack.unwrap_or(crate::artifact::MAX_STACK_SIZE);
        ensure!(
            (1..=crate::artifact::MAX_STACK_SIZE).contains(&max_stack),
            "max_stack must be 1..{}",
            crate::artifact::MAX_STACK_SIZE
        );
        Ok(Self {
            display_name,
            asset,
            hardness,
            max_stack,
        })
    }
}

/// Emits registration from the same declarations whose IDs enter the shared fingerprint.
pub fn generate(project: &super::Project) -> String {
    let mut blocks = Vec::new();
    let mut items = Vec::new();
    for definition in &project.definitions {
        let Some(content) = &definition.content else {
            continue;
        };
        let id = &definition.name;
        let name = format!("{:?}.into()", content.display_name);
        let asset = content.asset.to_token_stream();
        if definition.kind == "BlockId" {
            let mining = content.hardness.map_or_else(
                || "Unbreakable".into(),
                |hardness| format!("Breakable({hardness:?})"),
            );
            blocks.push(format!("::experience_sdk::server::cube(::experience_sdk::server::BlockDef {{ id: {id}.as_str().into(), display_name: {name}, textures: ::std::vec![::experience_sdk::server::TextureBinding {{ slot: \"*\".into(), path: {asset}.path.into() }}], mining: ::experience_sdk::server::Mining::{mining} }})"));
        } else {
            items.push(format!("::experience_sdk::server::ItemDef {{ id: {id}.as_str().into(), display_name: {name}, icon: {asset}.path.into(), max_stack: {} }}", content.max_stack));
        }
    }
    format!(
        "#[cfg(feature = \"server\")]\n/// Declares the blocks and items authored beside their stable IDs.\npub fn registration() -> ::experience_sdk::server::Registration {{ ::experience_sdk::server::Registration {{ blocks: ::std::vec![{}], items: ::std::vec![{}] }} }}\n",
        blocks.join(","),
        items.join(",")
    )
}
