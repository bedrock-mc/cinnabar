use std::{collections::BTreeMap, path::Path};

use assets::{AssetError, RegistryRecord};
use serde::Deserialize;
use serde_json::Value;

use super::{
    block::model_variant_index,
    parse::{
        BoundedMapIssue, BoundedUniqueMap, MAX_TEXTURE_KEYS, MAX_TEXTURE_VARIANTS, read_json,
        validate_texture_path,
    },
};

/// Bounded terrain path variants indexed by texture key.
#[derive(Debug)]
pub struct TerrainTextureMap {
    pub(super) entries: BTreeMap<Box<str>, TerrainPaths>,
    pub(crate) position_variations: BTreeMap<(Box<str>, u32), Vec<super::variations::WeightedPath>>,
}

impl TerrainTextureMap {
    /// Returns the deterministic variant-zero path used for terrain arrays
    /// without a documented block-state selector.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(TerrainPaths::first)
    }

    /// Returns exact array indices 0 and 1 only when the terrain key has
    /// exactly two variants.
    #[must_use]
    pub fn get_exact_pair(&self, key: &str) -> Option<[&str; 2]> {
        match self.entries.get(key)? {
            TerrainPaths::Variants { paths, .. } if paths.len() == 2 => {
                Some([paths[0].as_ref(), paths[1].as_ref()])
            }
            TerrainPaths::Static { .. } | TerrainPaths::Variants { .. } => None,
        }
    }

    /// Returns an exact two-variant route only when neither source carries
    /// overlay tint metadata.
    #[must_use]
    pub fn get_exact_pair_no_tint(&self, key: &str) -> Option<[&str; 2]> {
        match self.entries.get(key)? {
            TerrainPaths::Variants {
                paths,
                requires_tint: false,
                ..
            } if paths.len() == 2 => Some([paths[0].as_ref(), paths[1].as_ref()]),
            TerrainPaths::Static { .. } | TerrainPaths::Variants { .. } => None,
        }
    }

    /// Returns an exact two-variant route only when neither variant is tinted
    /// and the terrain entry carries no alias or extension metadata.
    pub(crate) fn get_exact_pair_plain(&self, key: &str) -> Option<[&str; 2]> {
        match self.entries.get(key)? {
            TerrainPaths::Variants {
                paths,
                requires_tint: false,
                has_extra_metadata: false,
                ..
            } if paths.len() == 2 => Some([paths[0].as_ref(), paths[1].as_ref()]),
            TerrainPaths::Static { .. } | TerrainPaths::Variants { .. } => None,
        }
    }

    /// Resolves the native-verified farmland top selector. Vanilla stores the
    /// wet path at index zero and the dry path at index one; only moisture
    /// zero is dry.
    #[must_use]
    pub fn get_exact_farmland_top(&self, moisturized_amount: u32) -> Option<(&str, u32)> {
        let TerrainPaths::Variants {
            paths,
            requires_tint: false,
            has_extra_metadata: false,
            ..
        } = self.entries.get("farmland")?
        else {
            return None;
        };
        if paths.len() != 2
            || paths[0].as_ref() != "textures/blocks/farmland_wet"
            || paths[1].as_ref() != "textures/blocks/farmland_dry"
            || moisturized_amount > 7
        {
            return None;
        }
        let index = usize::from(moisturized_amount == 0);
        Some((paths[index].as_ref(), index as u32))
    }

    /// Returns the literal vanilla farmland side source only when the terrain
    /// entry is static, untinted, and carries no alias or carried metadata.
    #[must_use]
    pub fn get_exact_farmland_side(&self) -> Option<&str> {
        match self.entries.get("farmland_side")? {
            TerrainPaths::Static {
                path,
                requires_tint: false,
                has_extra_metadata: false,
                ..
            } if path.as_ref() == "textures/blocks/dirt" => Some(path),
            TerrainPaths::Static { .. } | TerrainPaths::Variants { .. } => None,
        }
    }

    /// Returns an exact static route only when it carries no overlay tint
    /// metadata.
    #[must_use]
    pub fn get_exact_static_no_tint(&self, key: &str) -> Option<&str> {
        match self.entries.get(key)? {
            TerrainPaths::Static {
                path,
                requires_tint: false,
                ..
            } => Some(path),
            TerrainPaths::Static { .. } | TerrainPaths::Variants { .. } => None,
        }
    }

    /// Returns an exact static route only when it is untinted and carries no
    /// alias or extension metadata.
    pub(crate) fn get_exact_static_plain(&self, key: &str) -> Option<&str> {
        match self.entries.get(key)? {
            TerrainPaths::Static {
                path,
                requires_tint: false,
                has_extra_metadata: false,
                ..
            } => Some(path),
            TerrainPaths::Static { .. } | TerrainPaths::Variants { .. } => None,
        }
    }

    /// Returns the pinned vanilla singleton-array form only when it is
    /// untinted and carries no alias or extension metadata.
    pub(crate) fn get_exact_singleton_plain(&self, key: &str) -> Option<&str> {
        match self.entries.get(key)? {
            TerrainPaths::Variants {
                paths,
                requires_tint: false,
                has_extra_metadata: false,
                ..
            } if paths.len() == 1 => Some(paths[0].as_ref()),
            TerrainPaths::Static { .. } | TerrainPaths::Variants { .. } => None,
        }
    }

    pub(crate) fn requires_tint(&self, key: &str) -> bool {
        self.entries
            .get(key)
            .is_some_and(TerrainPaths::requires_tint)
    }

    pub(crate) fn get_for_record(&self, key: &str, record: &RegistryRecord) -> Option<&str> {
        self.get_for_record_variant(key, record)
            .map(|(path, _)| path)
    }

    /// Returns the same state index and path, before positional variation selection.
    pub(crate) fn get_for_record_variant(
        &self,
        key: &str,
        record: &RegistryRecord,
    ) -> Option<(&str, u32)> {
        let paths = self.entries.get(key)?;
        if !is_mushroom_face_key(key, &record.name) {
            return Some((paths.first(), 0));
        }
        let selected = mushroom_variant_index(record)?;
        match paths {
            TerrainPaths::Static { path, .. } => Some((path, 0)),
            TerrainPaths::Variants { paths, .. } if paths.len() == 16 => paths
                .get(selected)
                .map(|path| (path.as_ref(), selected as u32)),
            TerrainPaths::Variants { .. } => None,
        }
    }

    /// Resolves an immutable model material and records its selected terrain
    /// array index for deterministic state-to-variant inspection.
    pub(crate) fn get_for_model_record(
        &self,
        key: &str,
        record: &RegistryRecord,
    ) -> Option<(&str, u32)> {
        let paths = self.entries.get(key)?;
        match paths {
            TerrainPaths::Static { path, .. } => Some((path, 0)),
            TerrainPaths::Variants { paths, .. } => {
                let selected = if is_mushroom_face_key(key, &record.name) {
                    mushroom_variant_index(record).filter(|_| paths.len() == 16)?
                } else {
                    model_variant_index(key, record, paths.len())?
                };
                paths
                    .get(selected)
                    .map(|path| (path.as_ref(), selected as u32))
            }
        }
    }

    /// Path `variant` of an untinted key, clamped to its last variant as vanilla's icon lookup is.
    pub(crate) fn get_clamped_untinted(&self, key: &str, variant: usize) -> Option<&str> {
        match self.entries.get(key)? {
            TerrainPaths::Static {
                path,
                requires_tint: false,
                has_extra_metadata: false,
                ..
            } => Some(path),
            TerrainPaths::Variants {
                paths,
                requires_tint: false,
                has_extra_metadata: false,
                ..
            } => paths.get(variant.min(paths.len() - 1)).map(AsRef::as_ref),
            TerrainPaths::Static { .. } | TerrainPaths::Variants { .. } => None,
        }
    }

    /// The carried-item route understands a literal overlay color, but no
    /// unreviewed terrain extension metadata. Keep world/untinted resolution
    /// conservative: this does not relax `get_clamped_untinted`.
    pub(crate) fn get_clamped_carried(
        &self,
        key: &str,
        variant: usize,
    ) -> Option<(&str, Option<&str>)> {
        match self.entries.get(key)? {
            TerrainPaths::Static {
                path,
                overlay_color,
                has_extra_metadata: false,
                ..
            } => Some((path, overlay_color.as_deref())),
            TerrainPaths::Variants {
                paths,
                overlay_colors,
                has_extra_metadata: false,
                ..
            } => {
                let selected = variant.min(paths.len() - 1);
                Some((
                    paths.get(selected)?,
                    overlay_colors.get(selected)?.as_deref(),
                ))
            }
            TerrainPaths::Static { .. } | TerrainPaths::Variants { .. } => None,
        }
    }

    /// Every key with its variant-zero path, as the runtime's base catalog resolves it.
    pub(crate) fn first_paths(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .map(|(key, paths)| (key.as_ref(), paths.first()))
    }

    pub(crate) fn source_paths(&self) -> impl Iterator<Item = &str> {
        self.entries.values().flat_map(TerrainPaths::paths).chain(
            self.position_variations
                .values()
                .flatten()
                .map(|entry| entry.path.as_ref()),
        )
    }
}
#[derive(Debug)]
pub(super) enum TerrainPaths {
    Static {
        path: Box<str>,
        overlay_color: Option<Box<str>>,
        requires_tint: bool,
        has_extra_metadata: bool,
    },
    Variants {
        paths: Box<[Box<str>]>,
        overlay_colors: Box<[Option<Box<str>>]>,
        requires_tint: bool,
        has_extra_metadata: bool,
    },
}

impl TerrainPaths {
    fn first(&self) -> &str {
        match self {
            Self::Static { path, .. } => path,
            Self::Variants { paths, .. } => &paths[0],
        }
    }

    const fn requires_tint(&self) -> bool {
        match self {
            Self::Static { requires_tint, .. } | Self::Variants { requires_tint, .. } => {
                *requires_tint
            }
        }
    }

    fn paths(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        match self {
            Self::Static { path, .. } => Box::new(std::iter::once(path.as_ref())),
            Self::Variants { paths, .. } => Box::new(paths.iter().map(AsRef::as_ref)),
        }
    }
}

#[derive(Deserialize)]
struct TerrainDocument {
    texture_data: BoundedUniqueMap<TerrainEntry, MAX_TEXTURE_KEYS>,
}

#[derive(Deserialize)]
struct TerrainEntry {
    textures: Value,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TerrainValue {
    Path(String),
    Entry {
        path: String,
        overlay_color: Option<String>,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
    Variants(Vec<TerrainVariant>),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TerrainVariant {
    Path(String),
    Entry {
        path: String,
        overlay_color: Option<String>,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
}

impl TerrainVariant {
    fn into_path_tint_and_extra(self) -> (String, Option<String>, bool) {
        match self {
            Self::Path(path) => (path, None, false),
            Self::Entry {
                path,
                overlay_color,
                extra,
            } => (path, overlay_color, !extra.is_empty()),
        }
    }
}

pub(super) fn read_terrain(path: &Path) -> Result<TerrainTextureMap, AssetError> {
    let document: TerrainDocument = read_json(path, true)?;
    let texture_data = match document.texture_data.issue {
        Some(BoundedMapIssue::Duplicate(key)) => {
            return Err(AssetError::DuplicateTerrainTextureKey {
                path: path.to_path_buf(),
                key,
            });
        }
        Some(BoundedMapIssue::TooMany { count }) => {
            return Err(AssetError::TooManyTextureKeys {
                count,
                max: MAX_TEXTURE_KEYS,
            });
        }
        None => document.texture_data.entries,
    };

    let mut entries = BTreeMap::new();
    let mut position_variations = BTreeMap::new();
    for (key, mut entry) in texture_data {
        let values = match &mut entry.textures {
            Value::Array(values) => values.as_mut_slice(),
            value => std::slice::from_mut(value),
        };
        if values.len() > MAX_TEXTURE_VARIANTS {
            return Err(AssetError::TooManyTextureVariants {
                key: key.into(),
                count: values.len(),
                max: MAX_TEXTURE_VARIANTS,
            });
        }
        for (index, value) in values.iter_mut().enumerate() {
            let alternatives = super::variations::extract(value)?;
            if !alternatives.is_empty() {
                position_variations
                    .insert((key.clone().into_boxed_str(), index as u32), alternatives);
            }
        }
        let value = serde_json::from_value(entry.textures).map_err(|source| AssetError::Json {
            path: path.to_path_buf(),
            source,
        })?;
        let variants = collect_terrain_paths(&key, value, !entry.extra.is_empty())?;
        entries.insert(key.into_boxed_str(), variants);
    }
    Ok(TerrainTextureMap {
        entries,
        position_variations,
    })
}

fn collect_terrain_paths(
    key: &str,
    value: TerrainValue,
    entry_has_extra_metadata: bool,
) -> Result<TerrainPaths, AssetError> {
    match value {
        TerrainValue::Path(path) => {
            validate_texture_path(&path)?;
            Ok(TerrainPaths::Static {
                path: path.into_boxed_str(),
                overlay_color: None,
                requires_tint: false,
                has_extra_metadata: entry_has_extra_metadata,
            })
        }
        TerrainValue::Entry {
            path,
            overlay_color,
            extra,
        } => {
            validate_texture_path(&path)?;
            Ok(TerrainPaths::Static {
                path: path.into_boxed_str(),
                requires_tint: overlay_color.is_some(),
                overlay_color: overlay_color.map(String::into_boxed_str),
                has_extra_metadata: entry_has_extra_metadata || !extra.is_empty(),
            })
        }
        TerrainValue::Variants(variants) => {
            if variants.len() > MAX_TEXTURE_VARIANTS {
                return Err(AssetError::TooManyTextureVariants {
                    key: key.into(),
                    count: variants.len(),
                    max: MAX_TEXTURE_VARIANTS,
                });
            }
            let mut paths = Vec::with_capacity(variants.len());
            let mut overlay_colors = Vec::with_capacity(variants.len());
            let mut requires_tint = false;
            let mut has_extra_metadata = entry_has_extra_metadata;
            for variant in variants {
                let (path, overlay_color, variant_has_extra_metadata) =
                    variant.into_path_tint_and_extra();
                validate_texture_path(&path)?;
                paths.push(path.into_boxed_str());
                requires_tint |= overlay_color.is_some();
                overlay_colors.push(overlay_color.map(String::into_boxed_str));
                has_extra_metadata |= variant_has_extra_metadata;
            }
            if paths.is_empty() {
                return Err(AssetError::EmptyTextureVariants(key.into()));
            }
            Ok(TerrainPaths::Variants {
                paths: paths.into_boxed_slice(),
                overlay_colors: overlay_colors.into_boxed_slice(),
                requires_tint,
                has_extra_metadata,
            })
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HugeMushroomState {
    huge_mushroom_bits: HugeMushroomSelector,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HugeMushroomSelector {
    #[serde(rename = "type")]
    kind: Box<str>,
    value: u8,
}

fn mushroom_variant_index(record: &RegistryRecord) -> Option<usize> {
    let state = serde_json::from_str::<HugeMushroomState>(&record.canonical_state).ok()?;
    (state.huge_mushroom_bits.kind.as_ref() == "int")
        .then_some(usize::from(state.huge_mushroom_bits.value))
        .filter(|&bits| bits <= 15)
}

fn is_mushroom_face_key(key: &str, block_name: &str) -> bool {
    let prefix = match block_name {
        "minecraft:brown_mushroom_block" => "mushroom_brown_",
        "minecraft:red_mushroom_block" => "mushroom_red_",
        "minecraft:mushroom_stem" => "mushroom_stem_",
        _ => return false,
    };
    key.strip_prefix(prefix)
        .is_some_and(|face| matches!(face, "west" | "east" | "bottom" | "top" | "north" | "south"))
}

#[cfg(test)]
mod review_tests {
    use super::*;
    #[test]
    fn review_outer_variant_limits_precede_nested_extraction() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("terrain.json");
        let entries = vec![serde_json::json!({"variations": []}); MAX_TEXTURE_VARIANTS + 1];
        let document = serde_json::json!({"texture_data":{"fixture":{"textures":entries}}});
        std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        assert!(matches!(
            read_terrain(&path),
            Err(AssetError::TooManyTextureVariants { .. })
        ));
    }
}
