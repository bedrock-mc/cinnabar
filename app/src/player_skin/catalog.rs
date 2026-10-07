//! Runtime classic skin catalog and private imported-image persistence.

use super::*;
use launcher::dressing_room::{DressingRoomSkin, DressingRoomView, SkinModel};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, PathBuf};

const MAX_PNG_BYTES: u64 = 4 * 1024 * 1024;
const MAX_IMPORTED_ITEMS: usize = 256;
const MAX_PREFERENCES_BYTES: u64 = 1024 * 1024;

mod capes;
mod default_capes;
mod default_capes_loader;
mod edits;
pub(crate) use capes::{delete_cape, import_cape, rename_cape, select_cape};
pub(crate) use default_capes_loader::merge as merge_default_capes;
#[cfg(not(test))]
pub(crate) use default_capes_loader::refresh as refresh_default_capes;
pub(crate) use edits::{delete, rename};

#[derive(Default, Serialize, Deserialize)]
struct Preferences {
    selected: Option<String>,
    #[serde(default)]
    imported: Vec<Imported>,
    #[serde(default)]
    selected_cape: Option<String>,
    #[serde(default)]
    capes: Vec<ImportedCape>,
}

#[derive(Serialize, Deserialize)]
struct ImportedCape {
    id: String,
    name: String,
    file: String,
}

#[derive(Serialize, Deserialize)]
struct Imported {
    id: String,
    name: String,
    file: String,
    model: SkinModel,
}

#[derive(Deserialize)]
struct NativeCatalog {
    skins: Vec<NativeSkin>,
}

#[derive(Deserialize)]
struct NativeSkin {
    localization_name: String,
    geometry: String,
    texture: String,
    #[serde(rename = "type")]
    kind: String,
}

pub(crate) fn load(layout: &InstallLayout, fallback: &LocalPlayerSkin) -> DressingRoomView {
    let preferences = read_bounded(&layout.skin_selection_file(), MAX_PREFERENCES_BYTES)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Preferences>(&bytes).ok())
        .unwrap_or_default();
    let mut skins = Vec::new();
    let root = native_root(layout);
    let geometries = root.as_deref().and_then(|root| geometry_catalog(root).ok());
    if let Some(root) = root
        && let Ok(bytes) = read_bounded(
            &root.join("skins.json"),
            protocol::MAX_SKIN_GEOMETRY_SOURCE_BYTES as u64,
        )
        && bytes.len() <= protocol::MAX_SKIN_GEOMETRY_SOURCE_BYTES
        && let Ok(catalog) = serde_json::from_slice::<NativeCatalog>(&bytes)
    {
        for entry in catalog.skins.into_iter().take(256) {
            let model = match entry.geometry.as_str() {
                value if value == SkinModel::Classic.geometry() => SkinModel::Classic,
                value if value == SkinModel::Slim.geometry() => SkinModel::Slim,
                _ => continue,
            };
            if entry.kind != "free"
                || !single_filename(&entry.texture)
                || !launcher::dressing_room::STARTER_SKIN_NAMES
                    .contains(&entry.localization_name.as_str())
            {
                continue;
            }
            let path = root.join(&entry.texture);
            let Ok(mut skin) = read_png(&path) else {
                continue;
            };
            let Some(geometry) = geometries
                .as_ref()
                .map(|catalog| catalog[usize::from(model == SkinModel::Slim)].clone())
            else {
                continue;
            };
            skin.geometry = Some(geometry);
            skins.push(DressingRoomSkin {
                id: format!("vanilla:{}", entry.localization_name),
                name: entry.localization_name,
                path: path.to_string_lossy().into_owned(),
                imported: false,
                model,
                skin,
            });
        }
    }
    for entry in preferences.imported.into_iter().take(MAX_IMPORTED_ITEMS) {
        if !single_filename(&entry.file) {
            continue;
        }
        let path = layout.dressing_room_dir().join(&entry.file);
        let Ok(mut skin) = read_png(&path) else {
            continue;
        };
        let geometry = match geometries.as_ref() {
            Some(catalog) => Some(catalog[usize::from(entry.model == SkinModel::Slim)].clone()),
            None if entry.model == SkinModel::Classic => None,
            None => continue,
        };
        skin.geometry = geometry;
        skins.push(DressingRoomSkin {
            id: entry.id,
            name: entry.name,
            path: path.to_string_lossy().into_owned(),
            imported: true,
            model: entry.model,
            skin,
        });
    }
    let selected = preferences
        .selected
        .as_ref()
        .and_then(|id| skins.iter().position(|entry| entry.id == id.as_str()))
        .or_else(|| {
            skins.iter().position(|entry| {
                entry.skin.width == fallback.width
                    && entry.skin.height == fallback.height
                    && entry.skin.rgba8 == fallback.rgba8
                    && entry.model == fallback.model()
            })
        })
        .or_else(|| {
            preferences
                .selected
                .as_deref()
                .filter(|id| id.starts_with("vanilla:"))
                .and_then(|_| {
                    skins.iter().position(|entry| {
                        !entry.imported
                            && entry.name == launcher::dressing_room::STARTER_SKIN_NAMES[0]
                    })
                })
        });
    let selected = selected.or_else(|| {
        skins.push(DressingRoomSkin {
            id: "current".to_owned(),
            name: "Current skin".to_owned(),
            path: layout.player_skin_asset().to_string_lossy().into_owned(),
            imported: false,
            model: fallback.model(),
            skin: fallback.standard_skin(),
        });
        Some(skins.len() - 1)
    });
    let mut capes = default_capes_loader::cached(layout);
    capes.extend(capes::load(layout, preferences.capes));
    let selected_cape = preferences
        .selected_cape
        .as_ref()
        .and_then(|id| capes.iter().position(|entry| &entry.id == id));
    DressingRoomView {
        skins: skins.into(),
        selected,
        capes: capes.into(),
        selected_cape,
        ..Default::default()
    }
}

pub(crate) fn select(
    layout: &InstallLayout,
    view: &mut DressingRoomView,
    index: usize,
) -> Result<(), String> {
    if view.skins.get(index).is_none() {
        return Err("This skin is no longer available.".to_owned());
    }
    let previous = view.selected;
    view.selected = Some(index);
    if let Err(error) = save(layout, view) {
        view.selected = previous;
        return Err(error);
    }
    Ok(())
}

pub(crate) fn import(
    layout: &InstallLayout,
    view: &mut DressingRoomView,
    source: &Path,
) -> Result<(), String> {
    let bytes = png_bytes(source)?;
    let mut skin = decode_png(&bytes)?;
    skin.geometry = geometry_for_view(layout, view, SkinModel::Classic)?;
    let digest = Sha256::digest(&bytes);
    let id = format!("imported:{digest:x}");
    if let Some(index) = view.skins.iter().position(|entry| entry.id == id) {
        return select(layout, view, index);
    }
    if view.skins.iter().filter(|entry| entry.imported).count() >= MAX_IMPORTED_ITEMS {
        return Err(format!(
            "The skin library already contains {MAX_IMPORTED_ITEMS} imported skins."
        ));
    }
    let file = format!("{digest:x}.png");
    fs::create_dir_all(layout.dressing_room_dir()).map_err(|error| error.to_string())?;
    let path = layout.dressing_room_dir().join(file);
    fs::write(&path, bytes).map_err(|error| error.to_string())?;
    let entry = DressingRoomSkin {
        id,
        name: source
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("Imported skin")
            .to_owned(),
        path: path.to_string_lossy().into_owned(),
        imported: true,
        model: SkinModel::Classic,
        skin,
    };
    let old = view.clone();
    let mut skins = view.skins.to_vec();
    skins.push(entry);
    view.selected = Some(skins.len() - 1);
    view.skins = skins.into();
    if let Err(error) = save(layout, view) {
        *view = old;
        return Err(error);
    }
    Ok(())
}

pub(crate) fn set_model(
    layout: &InstallLayout,
    view: &mut DressingRoomView,
    model: SkinModel,
) -> Result<(), String> {
    let index = view.selected.ok_or("Select an imported skin first.")?;
    let mut skins = view.skins.to_vec();
    let entry = skins
        .get_mut(index)
        .ok_or("This skin is no longer available.")?;
    if !entry.imported {
        return Err("Default skins keep their original model.".to_owned());
    }
    let source_geometry = geometry_for_view(layout, view, model)?;
    entry.skin.geometry = source_geometry;
    entry.model = model;
    let old = view.skins.clone();
    view.skins = skins.into();
    if let Err(error) = save(layout, view) {
        view.skins = old;
        return Err(error);
    }
    Ok(())
}

fn save(layout: &InstallLayout, view: &DressingRoomView) -> Result<(), String> {
    let preferences = Preferences {
        selected: view.selected_skin().map(|entry| entry.id.clone()),
        imported: view
            .skins
            .iter()
            .filter(|entry| entry.imported)
            .map(|entry| Imported {
                id: entry.id.clone(),
                name: entry.name.clone(),
                model: entry.model,
                file: Path::new(&entry.path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            })
            .collect(),
        selected_cape: view.selected_cape().map(|entry| entry.id.clone()),
        capes: view
            .capes
            .iter()
            .filter(|entry| entry.imported)
            .map(|entry| ImportedCape {
                id: entry.id.clone(),
                name: entry.name.clone(),
                file: Path::new(&entry.path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            })
            .collect(),
    };
    fs::create_dir_all(&layout.user_config_root).map_err(|error| error.to_string())?;
    let path = layout.skin_selection_file();
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(&preferences).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_PREFERENCES_BYTES {
        return Err("The skin library could not be saved because it is too large.".to_owned());
    }
    fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(temp);
        format!("The skin selection could not be saved: {error}")
    })
}

fn single_filename(value: &str) -> bool {
    let mut components = Path::new(value).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn native_root(layout: &InstallLayout) -> Option<PathBuf> {
    let local = layout.resource_root.join("skin_packs/vanilla");
    if local.join("skins.json").is_file() {
        return Some(local);
    }
    let bundle = client_ui::ui_runtime::oreui_assets::bundle_dir()?;
    bundle
        .ancestors()
        .take(6)
        .map(|path| path.join("skin_packs/vanilla"))
        .find(|path| path.join("skins.json").is_file())
}

fn geometry(
    layout: &InstallLayout,
    model: SkinModel,
) -> Result<Option<Arc<protocol::SkinGeometrySource>>, String> {
    match native_root(layout) {
        Some(root) => geometry_from_root(&root, model).map(Some),
        None if model == SkinModel::Classic => Ok(None),
        None => Err(
            "The slim model is unavailable until the vanilla skin assets are installed.".to_owned(),
        ),
    }
}

fn geometry_for_view(
    layout: &InstallLayout,
    view: &DressingRoomView,
    model: SkinModel,
) -> Result<Option<Arc<protocol::SkinGeometrySource>>, String> {
    if let Some(source) = view
        .skins
        .iter()
        .find(|entry| entry.model == model)
        .and_then(|entry| entry.skin.geometry.as_ref())
    {
        return Ok(Some(source.clone()));
    }
    geometry(layout, model)
}

fn geometry_catalog(root: &Path) -> Result<[Arc<protocol::SkinGeometrySource>; 2], String> {
    let data: Arc<str> = String::from_utf8(read_bounded(
        &root.join("geometry.json"),
        protocol::MAX_SKIN_GEOMETRY_SOURCE_BYTES as u64,
    )?)
    .map_err(|error| error.to_string())?
    .into();
    if data.len() > protocol::MAX_SKIN_GEOMETRY_SOURCE_BYTES {
        return Err("Skin geometry is too large.".to_owned());
    }
    let make = |model: SkinModel| -> Result<Arc<protocol::SkinGeometrySource>, String> {
        let patch = serde_json::json!({"geometry":{"default":model.geometry()}}).to_string();
        assets::parse_skin_geometry(&patch, &data)
            .map_err(|error| format!("The skin model could not be read: {error:?}"))?
            .ok_or("The selected skin model is unavailable.")?;
        Ok(Arc::new(protocol::SkinGeometrySource {
            resource_patch: patch.into(),
            geometry_data: data.clone(),
            animations: Arc::from([]),
        }))
    };
    Ok([make(SkinModel::Classic)?, make(SkinModel::Slim)?])
}

fn geometry_from_root(
    root: &Path,
    model: SkinModel,
) -> Result<Arc<protocol::SkinGeometrySource>, String> {
    let path = root.join("geometry.json");
    let bytes = read_bounded(&path, protocol::MAX_SKIN_GEOMETRY_SOURCE_BYTES as u64)?;
    if bytes.len() > protocol::MAX_SKIN_GEOMETRY_SOURCE_BYTES {
        return Err("Skin geometry is too large.".to_owned());
    }
    let data = String::from_utf8(bytes).map_err(|error| error.to_string())?;
    let patch = serde_json::json!({"geometry":{"default":model.geometry()}}).to_string();
    assets::parse_skin_geometry(&patch, &data)
        .map_err(|error| format!("The skin model could not be read: {error:?}"))?
        .ok_or("The selected skin model is unavailable.")?;
    Ok(Arc::new(protocol::SkinGeometrySource {
        resource_patch: patch.into(),
        geometry_data: data.into(),
        animations: Arc::from([]),
    }))
}

fn png_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let size = fs::metadata(path).map_err(|error| error.to_string())?.len();
    if size > MAX_PNG_BYTES {
        return Err("Choose a PNG smaller than 4 MB.".to_owned());
    }
    read_bounded(path, MAX_PNG_BYTES)
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|error| error.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("The skin asset is too large.".to_owned());
    }
    Ok(bytes)
}

fn read_png(path: &Path) -> Result<protocol::StandardSkin, String> {
    decode_png(&png_bytes(path)?)
}

fn decode_png(bytes: &[u8]) -> Result<protocol::StandardSkin, String> {
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(protocol::MAX_CLASSIC_SKIN_SIDE as u32);
    limits.max_image_height = Some(protocol::MAX_CLASSIC_SKIN_SIDE as u32);
    reader.limits(limits);
    let rgba = reader
        .decode()
        .map_err(|error| format!("The skin PNG could not be read: {error}"))?
        .to_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    if !(width as usize == protocol::CLASSIC_SKIN_SIDE
        || width as usize == protocol::MAX_CLASSIC_SKIN_SIDE)
    {
        return Err(format!(
            "Unsupported skin dimensions {width}×{height}. Choose a classic skin PNG."
        ));
    }
    let mut pixels = rgba.into_raw();
    if height.checked_mul(2) == Some(width) {
        pixels = protocol::expand_legacy_skin_rgba8(&pixels, width as usize);
    }
    if (height != width && height.checked_mul(2) != Some(width))
        || !protocol::normalize_classic_skin_rgba8(width, width, &mut pixels)
    {
        return Err(format!(
            "Unsupported skin dimensions {width}×{height}. Choose a classic skin PNG."
        ));
    }
    Ok(protocol::StandardSkin {
        width,
        height: width,
        rgba8: pixels.into(),
        cape: None,
        geometry: None,
    })
}

#[cfg(test)]
mod tests;
