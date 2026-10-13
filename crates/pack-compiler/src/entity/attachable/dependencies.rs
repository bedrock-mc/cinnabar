//! Input dependencies for deciding whether an installed equipment binding remains reusable.

use std::{collections::BTreeSet, path::Path};

use assets::AssetError;
use serde_json::Value;

use super::super::{collect::collect_optional_family, materials::MaterialStates};
use super::{parse_description, parse_unique_json, read_bounded_source};

/// Tracks material declarations alongside the attachable inputs read by binding compilation.
#[derive(Default)]
pub struct EquipmentBindingDependencies {
    materials: MaterialStates,
    authored_materials: BTreeSet<String>,
}

impl EquipmentBindingDependencies {
    /// Reads installed material parents and applies the stack's winning material declarations.
    pub fn load(root: Option<&Path>, authored: Option<&Value>) -> Result<Self, AssetError> {
        let mut dependencies = Self::default();
        // Installed material parents cannot affect classification without authored materials.
        if let Some(root) = root.filter(|_| authored.is_some()) {
            let mut files = Vec::new();
            collect_optional_family(root, "materials", &["material"], &mut files)?;
            for (_, path) in files {
                let bytes = read_bounded_source(root, &path)?;
                dependencies
                    .materials
                    .read(&parse_unique_json(&path, &bytes)?);
            }
        }
        if let Some(authored) = authored {
            let mut upper = MaterialStates::default();
            upper.read(authored);
            dependencies.authored_materials = dependencies.materials.overlay(upper);
        }
        Ok(dependencies)
    }

    /// Returns normalized material names changed by the accepted stack.
    pub fn authored_materials(&self) -> &BTreeSet<String> {
        &self.authored_materials
    }

    /// Conservatively includes every name in the description consumed by `compile_bindings`.
    /// Traversing that input instead of listing fields also covers controller aliases, literal
    /// poses and display transforms. Material parents use the compiler's declaration parser.
    pub fn names(&self, document: &Value) -> Result<BTreeSet<String>, AssetError> {
        let description = parse_description(document)?;
        let mut names = BTreeSet::new();
        collect_names(description, &mut names);
        self.materials.extend_dependencies(&mut names);
        Ok(names)
    }
}

/// Retains string values and conditional-reference keys throughout a binding input.
fn collect_names(value: &Value, names: &mut BTreeSet<String>) {
    match value {
        Value::String(name) => {
            names.insert(name.clone());
        }
        Value::Array(values) => {
            for value in values {
                collect_names(value, names);
            }
        }
        Value::Object(values) => {
            for (name, value) in values {
                names.insert(name.clone());
                collect_names(value, names);
            }
        }
        _ => {}
    }
}
