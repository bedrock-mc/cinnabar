use std::{
    env, fs,
    path::{Path, PathBuf},
};

use pack_compiler::pbr::{PbrColorStyle, PbrFormat, PbrNormalFormat, PbrPack};
use serde::Deserialize;

const LOCAL_CONFIG: &str = ".local/enhanced-pbr.json";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    packs: Vec<Pack>,
    #[serde(default)]
    complete_material: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pack {
    path: PathBuf,
    format: Option<String>,
    group: Option<String>,
    normal_format: Option<String>,
    color_style: Option<String>,
}

/// Parses explicit pack formats and resolves paths relative to the config file.
fn configuration(bytes: &[u8], parent: &Path) -> Result<Vec<PbrPack>, String> {
    let config: Config = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    config
        .packs
        .into_iter()
        .map(|pack| {
            let format = pack
                .format
                .map(|value| {
                    PbrFormat::parse(&value).ok_or_else(|| format!("unknown PBR format {value}"))
                })
                .transpose()?;
            let path = if pack.path.is_absolute() {
                pack.path
            } else {
                parent.join(pack.path)
            };
            let normal_format = pack
                .normal_format
                .map(|value| {
                    PbrNormalFormat::parse(&value)
                        .ok_or_else(|| format!("unknown normal format {value}"))
                })
                .transpose()?
                .unwrap_or_default();
            let color_style = pack
                .color_style
                .map(|value| {
                    PbrColorStyle::parse(&value)
                        .ok_or_else(|| format!("unknown color style {value}"))
                })
                .transpose()?
                .unwrap_or_default();
            let parsed = PbrPack::new(path, format)
                .with_normal_format(normal_format)
                .with_color_style(color_style)
                .with_complete_material_preference(config.complete_material);
            Ok(match pack.group {
                Some(group) => parsed.with_group(group),
                None => parsed,
            })
        })
        .collect()
}

/// Treats roots listed in one environment override as a shared companion pack group.
fn environment_packs(value: &str) -> Vec<PbrPack> {
    value
        .split([';', '\n'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| PbrPack::new(value, None).with_group(super::ENHANCED_PBR_DIR_ENVIRONMENT))
        .collect()
}

/// Selects existing pack directories from the override or local configuration.
pub(super) fn selected_packs() -> Option<Vec<PbrPack>> {
    let packs: Vec<PbrPack> = if let Some(value) = env::var_os(super::ENHANCED_PBR_DIR_ENVIRONMENT)
    {
        environment_packs(&value.to_string_lossy())
    } else {
        let path = Path::new(LOCAL_CONFIG);
        let bytes = fs::read(path).ok()?;
        if bytes.len() > 64 * 1024 {
            bevy::log::warn!(
                "{} exceeds the optional PBR configuration budget",
                path.display()
            );
            return None;
        }
        match configuration(&bytes, path.parent().unwrap_or(Path::new("."))) {
            Ok(packs) => packs,
            Err(error) => {
                bevy::log::warn!(
                    "{}: {error}; authored Enhanced textures disabled",
                    path.display()
                );
                return None;
            }
        }
    };
    let packs: Vec<_> = packs
        .into_iter()
        .filter(|pack: &PbrPack| pack.root().is_dir())
        .collect();
    if packs.is_empty() {
        bevy::log::warn!("optional Enhanced PBR selection contains no existing pack directory");
        return None;
    }
    Some(packs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_roots_pair_colour_and_material_maps() {
        let colours = tempfile::tempdir().unwrap();
        let materials = tempfile::tempdir().unwrap();
        for (root, name, pixel) in [
            (colours.path(), "stone.png", [180u8, 100, 40, 255]),
            (materials.path(), "stone_mer.png", [64, 128, 192, 255]),
        ] {
            let directory = root.join("textures/blocks");
            fs::create_dir_all(&directory).unwrap();
            image::ImageBuffer::from_pixel(2, 2, image::Rgba(pixel))
                .save(directory.join(name))
                .unwrap();
        }
        for separator in [";", "\n"] {
            let value = format!(
                "  {}{separator}  {}{separator}  ",
                colours.path().display(),
                materials.path().display()
            );
            let packs = environment_packs(&value);
            assert_eq!(packs.len(), 2);
            let frame = pack_compiler::pbr::load_pbr_texture(&packs, "textures/blocks/stone")
                .unwrap()
                .unwrap()
                .frame(0, 1)
                .unwrap();
            assert_eq!(frame.color.get_pixel(0, 0).0, [180, 100, 40, 255]);
            assert_eq!(frame.material.get_pixel(0, 0).0, [64, 128, 192, 0]);
            assert_ne!(frame.flags & assets::texture::PBR_REF_MATERIAL, 0);
        }
        let json = serde_json::to_vec(&serde_json::json!({"packs": [
            {"path": colours.path()}, {"path": materials.path()}
        ]}))
        .unwrap();
        let packs = configuration(&json, Path::new(".")).unwrap();
        let frame = pack_compiler::pbr::load_pbr_texture(&packs, "textures/blocks/stone")
            .unwrap()
            .unwrap()
            .frame(0, 1)
            .unwrap();
        assert_eq!(frame.flags & assets::texture::PBR_REF_MATERIAL, 0);
    }

    #[test]
    fn local_selection_retains_explicit_format_and_companion_ownership() {
        let packs=configuration(br#"{"packs":[{"path":"base","group":"paired"},{"path":"materials","format":"lab-pbr/1.3","group":"paired"},{"path":"fallback","format":"legacy"}]}"#,Path::new("config")).unwrap();
        assert_eq!(packs.len(), 3);
        assert_eq!(packs[1].root(), Path::new("config/materials"));
        assert_eq!(packs[1].format(), PbrFormat::LabPbr13);
        assert_eq!(packs[2].format(), PbrFormat::Legacy);
        assert!(
            configuration(
                br#"{"packs":[{"path":"base","format":"guess-from-suffix"}]}"#,
                Path::new(".")
            )
            .is_err()
        );
    }
}
