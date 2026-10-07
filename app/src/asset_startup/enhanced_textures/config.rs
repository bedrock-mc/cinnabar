use std::{
    env, fs,
    path::{Path, PathBuf},
};

use pack_compiler::pbr::{PbrFormat, PbrNormalFormat, PbrPack};
use serde::Deserialize;

const LOCAL_CONFIG: &str = ".local/enhanced-pbr.json";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    packs: Vec<Pack>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pack {
    path: PathBuf,
    format: Option<String>,
    group: Option<String>,
    normal_format: Option<String>,
}

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
            let parsed = PbrPack::new(path, format).with_normal_format(normal_format);
            Ok(match pack.group {
                Some(group) => parsed.with_group(group),
                None => parsed,
            })
        })
        .collect()
}

pub(super) fn selected_packs() -> Option<Vec<PbrPack>> {
    let packs: Vec<PbrPack> =
        if let Some(value) = env::var_os(crate::asset_startup::ENHANCED_PBR_DIR_ENVIRONMENT) {
            value
                .to_string_lossy()
                .split([';', '\n'])
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| PbrPack::new(value, None).with_group("environment selection"))
                .collect()
        } else {
            let path = Path::new(LOCAL_CONFIG);
            let bytes = fs::read(path).ok()?;
            if bytes.len() > 64 * 1024 {
                eprintln!(
                    "{} exceeds the optional PBR configuration budget",
                    path.display()
                );
                return None;
            }
            match configuration(&bytes, path.parent().unwrap_or(Path::new("."))) {
                Ok(packs) => packs,
                Err(error) => {
                    eprintln!(
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
        eprintln!("optional Enhanced PBR selection contains no existing pack directory");
        return None;
    }
    Some(packs)
}

#[cfg(test)]
mod tests {
    use super::*;

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
