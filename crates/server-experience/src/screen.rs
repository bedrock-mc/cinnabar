//! Modal screens: the data a guest binds into its signed JSON-UI templates, and the checks a
//! template passes before the client draws it.

use crate::policy::*;
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// A value bound into a screen property. Bool, integer and text use a channel record's leaf
/// encoding; `number` and `numbers` (a colour, an offset) carry what only the screen needs.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Value {
    Bool(bool),
    Integer(i64),
    Number(f64),
    Text(String),
    Numbers(Vec<f64>),
}

/// One collection row: each `#binding` name a template's collection controls read.
pub type Row = BTreeMap<String, Value>;

/// A bundle's verified screen files: the JSON-UI templates its manifest indexes and the images
/// under `textures/`, both by bundle path, and the namespace every template declares.
#[derive(Debug, Default)]
pub struct Files {
    pub namespace: String,
    pub templates: BTreeMap<String, Vec<u8>>,
    pub textures: Vec<(String, Vec<u8>)>,
}

impl Value {
    /// Text may carry formatting codes and line breaks but no other control characters.
    pub fn validate(&self) -> Result<()> {
        let valid = match self {
            Value::Bool(_) | Value::Integer(_) => true,
            Value::Number(number) => number.is_finite(),
            Value::Text(text) => {
                text.len() <= MAX_WIDGET_TEXT_BYTES
                    && !text.chars().any(|c| c.is_control() && c != '\n')
            }
            Value::Numbers(numbers) => {
                !numbers.is_empty()
                    && numbers.len() <= MAX_UI_NUMBERS
                    && numbers.iter().all(|number| number.is_finite())
            }
        };
        ensure!(valid, "invalid screen value");
        Ok(())
    }
}

/// A `#name` the JSON-UI engine binds, such as `#item_name` or `#propagateAlpha`.
/// Text an edit box may hold across the helper boundary: at most `MAX_EDIT_TEXT_BYTES`, with no
/// control characters but line breaks.
pub fn edit_text(text: &str) -> bool {
    text.len() <= MAX_EDIT_TEXT_BYTES && !text.chars().any(|c| c.is_control() && c != '\n')
}

pub fn binding_name(name: &str) -> bool {
    name.strip_prefix('#').is_some_and(|rest| {
        !rest.is_empty()
            && name.len() <= MAX_IDENTIFIER_BYTES
            && rest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'))
    })
}

/// A collection name a template's `collection_name` or `binding_collection_name` reads.
pub fn collection_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_IDENTIFIER_BYTES
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Checks a whole collection before it replaces the bound one.
pub fn validate_rows(rows: &[Row]) -> Result<()> {
    ensure!(
        rows.len() <= MAX_COLLECTION_ROWS,
        "too many collection rows"
    );
    for row in rows {
        ensure!(row.len() <= MAX_ROW_FIELDS, "too many row fields");
        for (name, value) in row {
            ensure!(binding_name(name), "invalid binding name");
            value.validate()?;
        }
    }
    Ok(())
}

/// A modal's layout size in GUI units and the GUI scale, the window's logical pixels per GUI unit.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GuiSize {
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

/// What a modal's scroll view shows along its scrolling axis, in GUI units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScrollRange {
    /// How far the content is scrolled, from 0 to `content` - `viewport`.
    pub offset: f64,
    pub viewport: f64,
    pub content: f64,
}

impl ScrollRange {
    /// Whether every length is finite and not negative.
    pub fn valid(&self) -> bool {
        [self.offset, self.viewport, self.content]
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0)
    }
}

impl GuiSize {
    /// Whether every dimension is finite and positive.
    pub fn valid(&self) -> bool {
        [self.width, self.height, self.scale]
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
    }
}

/// A rectangle in GUI units, from the top left of the screen root.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn contains(&self, [x, y]: [f64; 2]) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
}

/// An open vanilla container screen as a player mod's overlay sees it: the JSON-UI screen
/// drawn, the root's size, the union of its laid-out panels and the other vanilla areas the
/// overlay may not cover.
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenLayout {
    pub screen: String,
    pub size: GuiSize,
    pub gui: Rect,
    pub exclusions: Vec<Rect>,
    /// The open view's laid-out bounds, which stand in for `gui` while the view is up.
    pub view: Option<Rect>,
}

impl ScreenLayout {
    /// What the overlay keeps clear of: the open view, else the vanilla panels.
    pub fn panels(&self) -> &Rect {
        self.view.as_ref().unwrap_or(&self.gui)
    }

    /// Whether a mod's overlay may draw or take input at `point`: outside the panels (the open
    /// view's while it is up) and every exclusion.
    pub fn overlay_allows(&self, point: [f64; 2]) -> bool {
        !self.panels().contains(point) && !self.exclusions.iter().any(|area| area.contains(point))
    }
}

/// The modal one bundle draws: its open template and everything bound into it. Bound data
/// outlives switching or closing the screen, as a widget does.
#[derive(Clone, Debug, Default)]
pub struct Modal {
    pub template: Option<String>,
    pub collections: BTreeMap<String, Arc<[Row]>>,
    pub values: BTreeMap<String, Value>,
    /// The text each edit box was last set to by its `text_box_name`, with the `revision` that
    /// set it, so a presenter applies each one once and the user's later typing stands.
    pub texts: BTreeMap<String, (u64, String)>,
    /// Counts every change, so a presenter rebuilds its data source only when it moved.
    pub revision: u64,
}

impl Modal {
    /// Opens or switches to `template`; `None` closes the modal.
    pub fn open(&mut self, template: Option<String>) {
        self.template = template;
        self.revision += 1;
    }

    pub fn set_collection(&mut self, name: String, rows: Vec<Row>) {
        self.collections.insert(name, rows.into());
        self.revision += 1;
    }

    pub fn set_value(&mut self, name: String, value: Value) {
        self.values.insert(name, value);
        self.revision += 1;
    }

    pub fn set_text(&mut self, control: String, text: String) {
        self.revision += 1;
        self.texts.insert(control, (self.revision, text));
    }

    /// Bounds the retained data after a transaction applied to a private copy.
    pub fn check(&self) -> Result<()> {
        ensure!(
            self.collections.len() <= MAX_COLLECTIONS,
            "collection budget exceeded"
        );
        ensure!(self.values.len() <= MAX_UI_VALUES, "value budget exceeded");
        ensure!(
            self.texts.len() <= MAX_UI_VALUES,
            "edit box budget exceeded"
        );
        Ok(())
    }
}

/// Parses a template as strict JSON and checks it declares `namespace`. Returns every control
/// of another namespace it names in an unambiguous reference (`name@ns.base`, `@ns.anim`), as
/// `(namespace, name)`, which the client must find in the vanilla pack; a bare `ns.name` string
/// may be a button id, and can only resolve against the vanilla pack and the bundle anyway.
pub fn validate_template(bytes: &[u8], namespace: &str) -> Result<BTreeSet<(String, String)>> {
    ensure!(bytes.len() <= MAX_TEMPLATE_BYTES, "template too large");
    let document: Json = serde_json::from_slice(bytes)?;
    let Json::Object(object) = &document else {
        bail!("template is not a JSON object");
    };
    ensure!(
        object.get("namespace").and_then(Json::as_str) == Some(namespace),
        "template must declare its bundle's namespace"
    );
    let mut foreign = BTreeSet::new();
    references(&document, &mut |reference| {
        if let Some((referenced, name)) = reference.split_once('.')
            && !referenced.starts_with('$')
            && referenced != namespace
        {
            foreign.insert((referenced.to_owned(), name.to_owned()));
        }
    });
    Ok(foreign)
}

/// Visits the base of every `name@base` key or value and every `@reference` string.
fn references(value: &Json, visit: &mut impl FnMut(&str)) {
    let mut text = |text: &str| {
        if let Some(reference) = text.strip_prefix('@') {
            visit(reference);
        } else if let Some((name, base)) = text.split_once('@')
            && !name.contains(char::is_whitespace)
            && !base.contains(char::is_whitespace)
        {
            visit(base);
        }
    };
    match value {
        Json::String(string) => text(string),
        Json::Array(items) => items.iter().for_each(|item| references(item, visit)),
        Json::Object(object) => {
            for (key, item) in object {
                if let Some((_, base)) = key.split_once('@') {
                    visit(base);
                }
                references(item, visit);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
