use std::{fs, io::Cursor, path::Path};

use ::image::{ImageFormat, ImageReader};
use assets::{
    parse_skin_geometry,
    starter_skins::{
        CLASSIC_SKIN_GEOMETRY, MAX_STARTER_SKIN_GEOMETRY_BYTES, SLIM_SKIN_GEOMETRY,
        STARTER_SKIN_SIDE, STARTER_SKIN_SOURCES, StarterSkin, StarterSkins, encode_starter_skins,
    },
};
use serde_json::{Map, Value};

const MAX_TEXTURE_BYTES: u64 = 64 * 1024;
const MAX_MODELS_BYTES: u64 = 4 * 1024 * 1024;
const MODELS: &str = "models/mobs.json";
const MAX_INHERITANCE_DEPTH: usize = 8;

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let size = fs::metadata(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .len();
    if size > limit {
        return Err(format!("{} exceeds {limit} bytes", path.display()));
    }
    fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn read_skin(path: &Path) -> Result<Box<[u8]>, String> {
    let decoded = ImageReader::with_format(
        Cursor::new(read_bounded(path, MAX_TEXTURE_BYTES)?),
        ImageFormat::Png,
    )
    .decode()
    .map_err(|error| format!("{}: {error}", path.display()))?
    .into_rgba8();
    if decoded.dimensions() != (STARTER_SKIN_SIDE, STARTER_SKIN_SIDE) {
        return Err(format!(
            "{} is {:?}, expected {STARTER_SKIN_SIDE}x{STARTER_SKIN_SIDE}",
            path.display(),
            decoded.dimensions()
        ));
    }
    Ok(decoded.into_raw().into_boxed_slice())
}

/// The legacy model document reduced to the classic and slim skin models and their parents.
fn skin_geometry(models: &Map<String, Value>) -> Result<String, String> {
    let identifier = |key: &str| key.split_once(':').map_or(key, |(own, _)| own).to_owned();
    let mut kept = Map::new();
    for model in [CLASSIC_SKIN_GEOMETRY, SLIM_SKIN_GEOMETRY] {
        let mut next = Some(model.to_owned());
        let mut depth = 0;
        while let Some(wanted) = next.take() {
            depth += 1;
            if depth > MAX_INHERITANCE_DEPTH {
                return Err(format!("{model} inherits too deeply"));
            }
            let (key, value) = models
                .iter()
                .find(|(key, _)| identifier(key) == wanted)
                .ok_or_else(|| format!("{MODELS} has no {wanted}"))?;
            kept.insert(key.clone(), value.clone());
            next = key.split_once(':').map(|(_, parent)| parent.to_owned());
        }
    }
    if let Some(version) = models.get("format_version") {
        kept.insert("format_version".to_owned(), version.clone());
    }
    let geometry = Value::Object(kept).to_string();
    if geometry.len() > MAX_STARTER_SKIN_GEOMETRY_BYTES {
        return Err(format!(
            "skin geometry exceeds {MAX_STARTER_SKIN_GEOMETRY_BYTES} bytes"
        ));
    }
    for model in [CLASSIC_SKIN_GEOMETRY, SLIM_SKIN_GEOMETRY] {
        let patch = serde_json::json!({ "geometry": { "default": model } }).to_string();
        match parse_skin_geometry(&patch, &geometry) {
            Ok(Some(_)) => {}
            _ => return Err(format!("{model} does not resolve as skin geometry")),
        }
    }
    Ok(geometry)
}

/// Reads the starter skins and humanoid skin models from `pack` and encodes the MCBESKN1 carrier.
pub fn compile_starter_skins(pack: &Path) -> Result<Vec<u8>, String> {
    let models: Map<String, Value> =
        serde_json::from_slice(&read_bounded(&pack.join(MODELS), MAX_MODELS_BYTES)?)
            .map_err(|error| format!("{MODELS}: {error}"))?;
    let skins = STARTER_SKIN_SOURCES
        .iter()
        .map(|source| {
            Ok(StarterSkin {
                name: source.name.into(),
                slim: source.slim,
                rgba8: read_skin(&pack.join(source.texture))?,
            })
        })
        .collect::<Result<_, String>>()?;
    let carrier = StarterSkins {
        geometry: skin_geometry(&models)?.into(),
        skins,
    };
    encode_starter_skins(&carrier).map_err(|error| error.to_string())
}

/// Compiles the carrier and writes it to `out` through a temporary sibling file.
pub fn compile_starter_skins_to_file(pack: &Path, out: &Path) -> Result<(), String> {
    let blob = compile_starter_skins(pack)?;
    let temporary = out.with_extension("tmp");
    fs::write(&temporary, &blob).map_err(|error| format!("{}: {error}", temporary.display()))?;
    fs::rename(&temporary, out).map_err(|error| format!("{}: {error}", out.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube_model() -> Value {
        serde_json::json!({
            "texturewidth": 64, "textureheight": 64,
            "bones": [{"name": "body", "pivot": [0, 24, 0],
                "cubes": [{"origin": [-4, 12, -2], "size": [8, 12, 4], "uv": [16, 16]}]}]
        })
    }

    #[test]
    fn keeps_only_skin_models_and_their_parents() {
        let models = serde_json::json!({
            "format_version": "1.8.0",
            "geometry.humanoid": cube_model(),
            "geometry.humanoid.custom:geometry.humanoid": {"bones": []},
            "geometry.humanoid.customSlim": cube_model(),
            "geometry.zombie": cube_model(),
        });
        let geometry = skin_geometry(models.as_object().unwrap()).unwrap();
        let kept: Map<String, Value> = serde_json::from_str(&geometry).unwrap();
        let mut keys: Vec<_> = kept.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "format_version",
                "geometry.humanoid",
                "geometry.humanoid.custom:geometry.humanoid",
                "geometry.humanoid.customSlim",
            ]
        );
    }

    #[test]
    fn a_missing_slim_model_fails_the_compile() {
        let models = serde_json::json!({
            "format_version": "1.8.0",
            "geometry.humanoid.custom": cube_model(),
        });
        assert!(skin_geometry(models.as_object().unwrap()).is_err());
    }

    #[test]
    fn pinned_pack_compiles_when_available() {
        let pack = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(assets::vanilla_source().resource_pack_dir());
        if !pack.join(MODELS).is_file() {
            eprintln!(
                "skipping pinned_pack_compiles_when_available: fixture unavailable; requires the pinned vanilla resource pack under .local"
            );
            return;
        }
        let decoded =
            assets::starter_skins::decode_starter_skins(&compile_starter_skins(&pack).unwrap())
                .unwrap();
        assert_eq!(decoded.skins.len(), STARTER_SKIN_SOURCES.len());
    }
}
