//! Whether the prepared carriers are current. Each `assetc` step's identity hashes its arguments,
//! the pinned kit inputs it reads, the pinned pack and its output's carrier format; `prepared.json`
//! beside the carriers records the identities they were built from.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::plan::{self, Action, COMPILED, Step, VANILLA_MANIFEST};

pub(super) const STAMP_FILE: &str = "prepared.json";
const SCHEMA: u32 = 1;
const FONT_CACHE: &str = ".local/assets/ui-font/";

#[derive(Debug, Default, Deserialize, PartialEq, Serialize)]
pub(super) struct Stamp {
    schema: u32,
    /// Carrier (or output directory) name to the identity it was built from.
    carriers: BTreeMap<String, String>,
}

/// The stamp beside `prepared`; empty when absent, unreadable or from another schema.
pub(super) fn read(prepared: &Path) -> Stamp {
    fs::read(prepared.join(STAMP_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Stamp>(&bytes).ok())
        .filter(|stamp| stamp.schema == SCHEMA)
        .unwrap_or_default()
}

pub(super) fn write(dir: &Path, carriers: &BTreeMap<String, String>) -> Result<()> {
    let stamp = Stamp {
        schema: SCHEMA,
        carriers: carriers.clone(),
    };
    let path = dir.join(STAMP_FILE);
    fs::write(&path, serde_json::to_vec_pretty(&stamp)?)
        .with_context(|| format!("write {}", path.display()))
}

/// What a preparation run must do.
#[derive(Debug)]
pub(super) struct Selection {
    /// Per plan step: whether it runs.
    pub run: Vec<bool>,
    /// Every `assetc` step's current identity, keyed like [`Stamp`].
    pub identities: BTreeMap<String, String>,
    /// Some running step reads the pack, so it must be downloaded and extracted.
    pub needs_pack: bool,
    /// The output name of each running `assetc` step, cleared before it runs.
    pub outputs: Vec<String>,
}

impl Selection {
    pub(super) fn is_current(&self) -> bool {
        !self.run.contains(&true)
    }
}

/// Compares the plan built from `kit` with the carriers in `prepared`. Without every required
/// carrier and a stamp, everything reruns.
pub(super) fn select(steps: &[Step], kit: &Path, prepared: &Path) -> Result<Selection> {
    let stamp = if plan::carriers_present(prepared) {
        read(prepared)
    } else {
        Stamp::default()
    };
    let pack_dir = plan::cache_dir(kit)?;
    let manifest = super::runner::kit_file(kit, VANILLA_MANIFEST)
        .with_context(|| format!("the preparation kit lacks {VANILLA_MANIFEST}"))?;
    let pack = file_sha256(&manifest)?;
    let compiler = file_sha256(&kit.join("bin").join(super::runner::assetc_name()))?;
    let resolve = |arg: &str| super::runner::kit_file(kit, arg);
    let mut selection = Selection {
        run: vec![false; steps.len()],
        identities: BTreeMap::new(),
        needs_pack: false,
        outputs: Vec::new(),
    };
    let mut needs_font = false;
    for (index, step) in steps.iter().enumerate() {
        let Action::Assetc(args) = &step.action else {
            continue;
        };
        let (key, identity) = identity(
            args,
            &pack_dir,
            &pack,
            &compiler,
            &resolve,
            &selection.identities,
        )?;
        let stale = stamp.carriers.get(&key) != Some(&identity);
        if stale {
            selection.needs_pack |= args.iter().any(|arg| arg.starts_with(&pack_dir));
            needs_font |= args.iter().any(|arg| arg.starts_with(FONT_CACHE));
            selection.outputs.push(key.clone());
        }
        selection.run[index] = stale;
        selection.identities.insert(key, identity);
    }
    for (index, step) in steps.iter().enumerate() {
        if let Action::Script(name) = step.action {
            selection.run[index] = match name {
                "fetch-vanilla-assets" => selection.needs_pack,
                "fetch-ui-font" => needs_font,
                _ => true,
            };
        }
    }
    Ok(selection)
}

/// The step's output name and identity. Outputs of earlier steps count through their identity.
fn identity(
    args: &[String],
    pack_dir: &str,
    pack: &str,
    compiler: &str,
    resolve: &dyn Fn(&str) -> Option<PathBuf>,
    earlier: &BTreeMap<String, String>,
) -> Result<(String, String)> {
    let mut hasher = Sha256::new();
    hasher.update(compiler.as_bytes());
    hasher.update([0]);
    let mut key = None;
    let mut flag: Option<&str> = None;
    for arg in args {
        hasher.update(arg.as_bytes());
        hasher.update([0]);
        match flag {
            Some("--out" | "--out-dir") => key = Some(file_name(arg)),
            Some("--report") => {}
            _ if arg.starts_with(pack_dir) => hasher.update(pack.as_bytes()),
            _ if arg.starts_with(COMPILED) => {
                if let Some(dependency) = earlier.get(&file_name(arg)) {
                    hasher.update(dependency.as_bytes());
                }
            }
            _ => {
                if let Some(path) = resolve(arg) {
                    hasher.update(file_sha256(&path)?.as_bytes());
                }
            }
        }
        flag = Some(arg.as_str());
    }
    let key = key.context("assetc step names no output")?;
    hasher.update(carrier_format(&key));
    Ok((key, format!("{:x}", hasher.finalize())))
}

fn file_name(arg: &str) -> String {
    Path::new(arg).file_name().map_or_else(
        || arg.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The magic and version the runtime decoder checks, so a format bump rebuilds that carrier.
fn carrier_format(key: &str) -> Vec<u8> {
    let (magic, version): (&[u8], u32) = match key.rsplit('.').next().unwrap_or(key) {
        "mcbea" => (&assets::BLOB_MAGIC, assets::BLOB_VERSION),
        "mcbeatm" => (
            &assets::ATMOSPHERE_BLOB_MAGIC,
            assets::ATMOSPHERE_BLOB_VERSION,
        ),
        "mcbeent" => (&assets::ENTITY_BLOB_MAGIC, assets::ENTITY_BLOB_VERSION),
        "mcbefont" => (&assets::FONT_CARRIER_MAGIC, assets::FONT_CARRIER_SCHEMA),
        "mcbehud" => (&assets::HUD_CARRIER_MAGIC, assets::HUD_CARRIER_VERSION),
        "mcbelang" | "lang" => (&assets::LANG_CARRIER_MAGIC, assets::LANG_CARRIER_VERSION),
        "mcbeico" => (&assets::ICON_CARRIER_MAGIC, assets::ICON_CARRIER_VERSION),
        "mcbeaud" => (assets::AUDIO_CARRIER_MAGIC, 0),
        "mcbeact" => (&assets::ACTOR_CARRIER_MAGIC, assets::ACTOR_CARRIER_VERSION),
        "mcbesnd" => (assets::SOUND_BANK_MAGIC, 0),
        "mcbeeqp" => (
            &assets::EQUIPMENT_CARRIER_MAGIC,
            assets::EQUIPMENT_CARRIER_VERSION,
        ),
        "mcbeui" => (&assets::UI_CARRIER_MAGIC, assets::UI_CARRIER_VERSION),
        "mcbept" => (
            &assets::PARTICLE_CARRIER_MAGIC,
            assets::PARTICLE_CARRIER_VERSION,
        ),
        "mcbeben" => (
            &assets::BLOCK_ENTITY_CARRIER_MAGIC,
            assets::BLOCK_ENTITY_CARRIER_VERSION,
        ),
        "mcbewth" => (
            &assets::WEATHER_TEXTURES_MAGIC,
            assets::WEATHER_TEXTURES_VERSION,
        ),
        "mcbehxt" => (&assets::HUD_EXTRAS_MAGIC, assets::HUD_EXTRAS_VERSION),
        _ => (b"", 0),
    };
    // The world carrier's sidecar is read beside it; its schema rebuilds the carrier too.
    let sidecar = if magic == assets::BLOB_MAGIC.as_slice() {
        assets::MATERIAL_KEYS_SCHEMA
    } else {
        0
    };
    [magic, &version.to_le_bytes(), &sidecar.to_le_bytes()].concat()
}

fn file_sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::first_run::{plan::REQUIRED_CARRIERS, test_support::Dir};

    fn kit(dir: &Dir) -> PathBuf {
        let kit = dir.path().join("kit");
        for sub in ["assets", "data", "scripts"] {
            fs::create_dir_all(kit.join(sub)).unwrap();
        }
        fs::create_dir_all(kit.join("bin")).unwrap();
        fs::write(
            kit.join("bin").join(super::super::runner::assetc_name()),
            b"compiler-v1",
        )
        .unwrap();
        set_pack(&kit, "v1");
        set_font(&kit, "aa");
        fs::write(kit.join("assets/hud-source-v2193.json"), b"{}").unwrap();
        for name in ["block-registry", "block-light-registry", "biome-registry"] {
            fs::write(kit.join(format!("data/{name}-v2193.bin")), name).unwrap();
        }
        kit
    }

    fn set_pack(kit: &Path, tag: &str) {
        let json =
            format!(r#"{{"tag":"{tag}","cache_dir":".local/assets/bedrock-samples/{tag}/full"}}"#);
        fs::write(kit.join(VANILLA_MANIFEST), json).unwrap();
    }

    fn set_font(kit: &Path, commit: &str) {
        let json = format!(
            r#"{{"commit":"{commit}","font_file":"F.ttf","fallback_commit":"bb","fallback_font_file":"N.otf"}}"#
        );
        fs::write(kit.join("assets/ui-font-source.json"), json).unwrap();
    }

    /// Publishes every required carrier with a stamp matching the kit as it is now.
    fn prepare(kit: &Path, prepared: &Path) {
        fs::create_dir_all(prepared).unwrap();
        for name in REQUIRED_CARRIERS {
            fs::write(prepared.join(name), b"x").unwrap();
        }
        let steps = plan::steps(kit).unwrap();
        write(prepared, &select(&steps, kit, prepared).unwrap().identities).unwrap();
    }

    fn running(kit: &Path, prepared: &Path) -> (Vec<&'static str>, bool) {
        let steps = plan::steps(kit).unwrap();
        let selection = select(&steps, kit, prepared).unwrap();
        let labels = steps
            .iter()
            .zip(&selection.run)
            .filter(|(_, run)| **run)
            .map(|(step, _)| step.label)
            .collect();
        (labels, selection.needs_pack)
    }

    #[test]
    fn review_changed_compiler_invalidates_prepared_carriers() {
        let dir = Dir::new("compiler-change");
        let (kit, prepared) = (kit(&dir), dir.path().join("compiled"));
        prepare(&kit, &prepared);
        fs::write(
            kit.join("bin").join(super::super::runner::assetc_name()),
            b"compiler-v2",
        )
        .unwrap();
        let (labels, _) = running(&kit, &prepared);
        assert!(labels.contains(&"Compiling world assets"));
        assert!(labels.contains(&"Compiling the UI font"));
    }

    #[test]
    fn a_matching_stamp_skips_preparation() {
        let dir = Dir::new("stamp-match");
        let (kit, prepared) = (kit(&dir), dir.path().join("compiled"));
        prepare(&kit, &prepared);
        assert_eq!(running(&kit, &prepared), (vec![], false));
    }

    #[test]
    fn carriers_without_a_stamp_rebuild_everything() {
        let dir = Dir::new("stamp-missing");
        let (kit, prepared) = (kit(&dir), dir.path().join("compiled"));
        prepare(&kit, &prepared);
        fs::remove_file(prepared.join(STAMP_FILE)).unwrap();
        let (labels, needs_pack) = running(&kit, &prepared);
        assert_eq!(labels.len(), plan::steps(&kit).unwrap().len());
        assert!(needs_pack);
    }

    #[test]
    fn a_new_pack_pin_rebuilds_every_pack_carrier() {
        let dir = Dir::new("stamp-pack");
        let (kit, prepared) = (kit(&dir), dir.path().join("compiled"));
        prepare(&kit, &prepared);
        set_pack(&kit, "v2");
        let (labels, needs_pack) = running(&kit, &prepared);
        assert!(needs_pack);
        assert!(labels.contains(&"Compiling world assets"));
        assert!(labels.contains(&"Compiling item icons"));
        assert!(!labels.contains(&"Compiling the UI font"));
    }

    #[test]
    fn one_changed_input_rebuilds_only_its_carrier() {
        let dir = Dir::new("stamp-one");
        let (kit, prepared) = (kit(&dir), dir.path().join("compiled"));
        prepare(&kit, &prepared);
        set_font(&kit, "cc");
        assert_eq!(
            running(&kit, &prepared),
            (
                vec!["Downloading the open UI font", "Compiling the UI font"],
                false
            )
        );
        prepare(&kit, &prepared);
        fs::write(kit.join("data/block-registry-v2193.bin"), b"changed").unwrap();
        let (labels, needs_pack) = running(&kit, &prepared);
        // The icon carrier reads the world carrier, so it follows it.
        assert_eq!(
            labels,
            [
                "Unpacking the Minecraft sample resource pack",
                "Compiling world assets",
                "Compiling item icons"
            ]
        );
        assert!(needs_pack);
    }

    #[test]
    fn carrier_formats_are_distinct() {
        assert_ne!(
            carrier_format("vanilla-v1.mcbeui"),
            carrier_format("vanilla-v1.mcbehud")
        );
        assert_eq!(
            carrier_format("lang"),
            carrier_format("vanilla-v1.mcbelang")
        );
    }
}
