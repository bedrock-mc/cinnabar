use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use assets::{
    AnimationInventory, AssetError, AtmosphereCompileOptions, AtmosphereRole,
    MATERIAL_FLAG_ALPHA_CUTOUT, compile_atmosphere_assets_with_options, compile_pack_with_biomes,
    encode_atmosphere_blob, encode_blob, inspect_animation_inventory, read_biome_registry,
    read_light_registry, read_registry, write_blob_atomic,
};
use clap::{Parser, Subcommand};
use serde::Serialize;
use sha2::{Digest, Sha256};

const MAX_REGISTRY_FILE_BYTES: usize = 128 * 1024 * 1024;
const MAX_SOURCE_MANIFEST_BYTES: usize = 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    about = "Compile verified local Bedrock resource-pack assets",
    after_help = "Compile inputs:\n  assetc compile --pack <RESOURCE_PACK> --registry <BLOCK_REGISTRY_BIN> --light-registry <LIGHT_REGISTRY_BIN> --biome-registry <BIOME_REGISTRY_BIN> --out <IGNORED_DIR>/vanilla-v1001.mcbea\n\nAtmosphere inputs:\n  assetc atmosphere --pack <RESOURCE_PACK> --source-manifest <VANILLA_SOURCE_JSON> --out <IGNORED_DIR>/vanilla-v1.mcbeatm --report <IGNORED_DIR>/atmosphere-assets.json\n\nAnimation inventory:\n  assetc animation-inventory --pack <RESOURCE_PACK> --source-manifest <VANILLA_SOURCE_JSON> --max-layers-per-page 2048 --max-pages 2 --out <IGNORED_DIR>/animation-inventory.json"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Compile the fixed vanilla sun, moon-phase, and cloud textures.
    Atmosphere {
        /// Root of the pinned vanilla resource pack.
        #[arg(long)]
        pack: PathBuf,
        /// Tracked manifest that pins the local resource-pack source.
        #[arg(long)]
        source_manifest: PathBuf,
        /// Exact local-only Bedrock 1.26.33.1 clouds.png override.
        #[arg(long)]
        clouds_override: Option<PathBuf>,
        /// Ignored/local MCBEATM1 output path.
        #[arg(long)]
        out: PathBuf,
        /// Ignored/local deterministic JSON provenance report path.
        #[arg(long)]
        report: PathBuf,
    },
    /// Compile a resource pack and Dragonfly registry into a runtime blob.
    Compile {
        /// Root containing blocks.json and the textures directory.
        #[arg(long)]
        pack: PathBuf,
        /// BREG1003 registry exported by tools/registrygen.
        #[arg(long)]
        registry: PathBuf,
        /// LREG1001 state light metadata bound to the exact BREG1003 input.
        #[arg(long)]
        light_registry: PathBuf,
        /// BIOREG01 registry exported by tools/registrygen.
        #[arg(long)]
        biome_registry: PathBuf,
        /// Ignored/local output path, conventionally ending in .mcbea.
        #[arg(long)]
        out: PathBuf,
    },
    /// Compile a bounded read-only animation plan and write its deterministic inventory.
    AnimationInventory {
        /// Root containing blocks.json and the textures directory.
        #[arg(long)]
        pack: PathBuf,
        /// Pinned source manifest whose exact bytes identify the local pack source.
        #[arg(long)]
        source_manifest: PathBuf,
        /// Maximum physical array layers in each texture page (1..=2048).
        #[arg(long)]
        max_layers_per_page: u32,
        /// Maximum physical texture pages (1..=2).
        #[arg(long)]
        max_pages: u32,
        /// Ignored/local deterministic JSON report path.
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Serialize)]
struct AnimationInventoryReport {
    schema: u32,
    source_manifest_sha256: Box<str>,
    canonical_pack_path: Box<str>,
    limits: AnimationInventoryLimits,
    inventory: AnimationInventory,
}

#[derive(Serialize)]
struct AnimationInventoryLimits {
    max_layers_per_page: u32,
    max_pages: u32,
}

#[derive(Serialize)]
struct AtmosphereReport {
    schema: u32,
    source: serde_json::Value,
    source_manifest_sha256: Box<str>,
    blob_sha256: Box<str>,
    textures: Box<[AtmosphereTextureReport]>,
}

#[derive(Serialize)]
struct AtmosphereTextureReport {
    role: &'static str,
    source_path: Box<str>,
    width: u32,
    height: u32,
    source_bytes: usize,
    decoded_rgba8_bytes: usize,
    source_sha256: Box<str>,
    pixels_sha256: Box<str>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command {
        Command::Atmosphere {
            pack,
            source_manifest,
            clouds_override,
            out,
            report,
        } => {
            compile_atmosphere_command(
                &pack,
                &source_manifest,
                clouds_override.as_deref(),
                &out,
                &report,
                compile_atmosphere_assets_with_options,
            )?;
        }
        Command::Compile {
            pack,
            registry,
            light_registry,
            biome_registry,
            out,
        } => {
            let registry_bytes = read_bounded(&registry)?;
            let records = read_registry(&registry_bytes)?;
            let light_registry_bytes = read_bounded(&light_registry)?;
            let light_properties =
                read_light_registry(&light_registry_bytes, &registry_bytes, records.len())?;
            let biome_registry_bytes = read_bounded(&biome_registry)?;
            let biome_records = read_biome_registry(&biome_registry_bytes)?;
            let behavior_pack = pack
                .parent()
                .ok_or("resource-pack path has no parent for behavior_pack")?
                .join("behavior_pack");
            let compiled = compile_pack_with_biomes(
                &pack,
                &behavior_pack,
                &records,
                &biome_records,
                &light_properties,
            )?;
            let blob = encode_blob(&compiled)?;
            write_blob_atomic(&out, &blob)?;
            let cutout_materials = compiled
                .materials
                .iter()
                .filter(|material| material.flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0)
                .count();
            println!(
                "compiled {} visuals, {} materials ({} alpha cutout), {} texture layers, and {} biome rules to {}",
                compiled.visuals.len(),
                compiled.materials.len(),
                cutout_materials,
                compiled
                    .texture_pages
                    .iter()
                    .map(|page| page.texture.layers)
                    .sum::<u32>(),
                compiled.biomes.rules.len(),
                out.display()
            );
        }
        Command::AnimationInventory {
            pack,
            source_manifest,
            max_layers_per_page,
            max_pages,
            out,
        } => {
            let canonical_pack = fs::canonicalize(&pack).map_err(|source| AssetError::Io {
                path: pack.clone(),
                source,
            })?;
            let manifest_bytes = read_bounded_with_limit(
                &source_manifest,
                MAX_SOURCE_MANIFEST_BYTES,
                "source manifest",
            )?;
            serde_json::from_slice::<serde_json::Value>(&manifest_bytes).map_err(|source| {
                AssetError::Json {
                    path: source_manifest.clone(),
                    source,
                }
            })?;
            let source_manifest_sha256 = format!("{:x}", Sha256::digest(&manifest_bytes));
            let inventory =
                inspect_animation_inventory(&canonical_pack, max_layers_per_page, max_pages)?;
            let report = AnimationInventoryReport {
                schema: 1,
                source_manifest_sha256: source_manifest_sha256.into_boxed_str(),
                canonical_pack_path: canonical_pack
                    .to_string_lossy()
                    .into_owned()
                    .into_boxed_str(),
                limits: AnimationInventoryLimits {
                    max_layers_per_page,
                    max_pages,
                },
                inventory,
            };
            let mut bytes =
                serde_json::to_vec_pretty(&report).map_err(|source| AssetError::Json {
                    path: out.clone(),
                    source,
                })?;
            bytes.push(b'\n');
            write_blob_atomic(&out, &bytes)?;
            println!(
                "inspected {} reachable animations, {} physical frames, {} deduplicated layers across {} pages to {}",
                report.inventory.reachable_animations,
                report.inventory.physical_animation_frames,
                report.inventory.deduplicated_layers,
                report.inventory.pages,
                out.display()
            );
        }
    }
    Ok(())
}

fn compile_atmosphere_command<F>(
    pack: &Path,
    source_manifest: &Path,
    clouds_override: Option<&Path>,
    out: &Path,
    report: &Path,
    compile: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: for<'a> FnOnce(
        &Path,
        &[u8],
        AtmosphereCompileOptions<'a>,
    ) -> Result<assets::CompiledAtmosphereAssets, AssetError>,
{
    let manifest_bytes = read_bounded_with_limit(
        source_manifest,
        MAX_SOURCE_MANIFEST_BYTES,
        "source manifest",
    )?;
    let source =
        serde_json::from_slice::<serde_json::Value>(&manifest_bytes).map_err(|source| {
            AssetError::Json {
                path: source_manifest.to_path_buf(),
                source,
            }
        })?;
    let compiled = compile(
        pack,
        &manifest_bytes,
        AtmosphereCompileOptions { clouds_override },
    )?;
    let blob = encode_atmosphere_blob(&compiled)?;
    let report_data = build_atmosphere_report(source, &compiled, &blob);
    let mut report_bytes =
        serde_json::to_vec_pretty(&report_data).map_err(|source| AssetError::Json {
            path: report.to_path_buf(),
            source,
        })?;
    report_bytes.push(b'\n');
    validate_output_bundle(out, report)?;
    write_blob_atomic(out, &blob)?;
    write_blob_atomic(report, &report_bytes)?;
    println!(
        "compiled {} pinned atmosphere textures to {} and {}",
        report_data.textures.len(),
        out.display(),
        report.display()
    );
    Ok(())
}

fn build_atmosphere_report(
    source: serde_json::Value,
    compiled: &assets::CompiledAtmosphereAssets,
    blob: &[u8],
) -> AtmosphereReport {
    let textures = compiled
        .textures
        .iter()
        .map(|texture| AtmosphereTextureReport {
            role: match texture.role {
                AtmosphereRole::Sun => "sun",
                AtmosphereRole::MoonPhases => "moon_phases",
                AtmosphereRole::Clouds => "clouds",
            },
            source_path: texture.source_path.clone(),
            width: texture.width,
            height: texture.height,
            source_bytes: texture.source_bytes as usize,
            decoded_rgba8_bytes: texture.rgba8.len(),
            source_sha256: hex(&texture.source_sha256).into_boxed_str(),
            pixels_sha256: hex(&texture.pixels_sha256).into_boxed_str(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    AtmosphereReport {
        schema: 1,
        source,
        source_manifest_sha256: hex(&compiled.source_manifest_sha256).into_boxed_str(),
        blob_sha256: format!("{:x}", Sha256::digest(blob)).into_boxed_str(),
        textures,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn validate_output_bundle(blob: &Path, report: &Path) -> Result<(), AssetError> {
    let normalized_blob = normalized_absolute(blob)?;
    let normalized_report = normalized_absolute(report)?;
    if paths_alias(&normalized_blob, &normalized_report)
        || paths_alias(
            &canonicalized_location(&normalized_blob)?,
            &canonicalized_location(&normalized_report)?,
        )
    {
        return Err(output_alias_error(blob));
    }

    let mut both_exist = true;
    for path in [blob, report] {
        match fs::metadata(path) {
            Ok(metadata) if !metadata.is_file() => {
                return Err(AssetError::Io {
                    path: path.to_path_buf(),
                    source: io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "atmosphere output destination is not a regular file",
                    ),
                });
            }
            Ok(_) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => both_exist = false,
            Err(source) => {
                return Err(AssetError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
    if both_exist {
        match same_file::is_same_file(blob, report) {
            Ok(true) => return Err(output_alias_error(blob)),
            Ok(false) => {}
            Err(source) => {
                return Err(AssetError::Io {
                    path: blob.to_path_buf(),
                    source,
                });
            }
        }
    }
    Ok(())
}

fn output_alias_error(path: &Path) -> AssetError {
    AssetError::Io {
        path: path.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::InvalidInput,
            "atmosphere blob and report paths must identify distinct files",
        ),
    }
}

fn normalized_absolute(path: &Path) -> Result<PathBuf, AssetError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| AssetError::Io {
                path: path.to_path_buf(),
                source,
            })?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(AssetError::Io {
                        path: path.to_path_buf(),
                        source: io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "output path escapes its filesystem root",
                        ),
                    });
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    Ok(normalized)
}

fn canonicalized_location(path: &Path) -> Result<PathBuf, AssetError> {
    let mut ancestor = path;
    let mut suffix = Vec::new();
    loop {
        match fs::canonicalize(ancestor) {
            Ok(mut canonical) => {
                for component in suffix.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                let Some(file_name) = ancestor.file_name() else {
                    return Err(AssetError::Io {
                        path: path.to_path_buf(),
                        source,
                    });
                };
                suffix.push(file_name.to_os_string());
                let Some(parent) = ancestor.parent() else {
                    return Err(AssetError::Io {
                        path: path.to_path_buf(),
                        source,
                    });
                };
                ancestor = parent;
            }
            Err(source) => {
                return Err(AssetError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
}

fn paths_alias(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .to_lowercase()
        .eq(&right.to_string_lossy().to_lowercase())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, AssetError> {
    read_bounded_with_limit(path, MAX_REGISTRY_FILE_BYTES, "registry")
}

fn read_bounded_with_limit(
    path: &Path,
    max_bytes: usize,
    label: &'static str,
) -> Result<Vec<u8>, AssetError> {
    let file = File::open(path).map_err(|source| AssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take((max_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > max_bytes {
        return Err(AssetError::Io {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{label} exceeds the {max_bytes}-byte compiler input limit"),
            ),
        });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, fs};

    use assets::{
        AtmosphereRole, AtmosphereTexture, CompiledAtmosphereAssets, RuntimeAtmosphereAssets,
    };
    use clap::Parser;
    use sha2::{Digest, Sha256};

    use super::{Cli, Command, compile_atmosphere_command};

    #[test]
    fn synthetic_cli_override_builds_canonical_path_only_report() {
        let directory = tempfile::tempdir().unwrap();
        let pack = directory.path().join("pack");
        let manifest = directory.path().join("manifest.json");
        let physical_override = directory.path().join("private-clouds.png");
        let blob = directory.path().join("atmosphere.mcbeatm");
        let report = directory.path().join("atmosphere.json");
        fs::write(&manifest, br#"{"artifact_policy":"local-only"}"#).unwrap();
        let cli = Cli::try_parse_from([
            OsString::from("assetc"),
            OsString::from("atmosphere"),
            OsString::from("--pack"),
            pack.as_os_str().to_owned(),
            OsString::from("--source-manifest"),
            manifest.as_os_str().to_owned(),
            OsString::from("--clouds-override"),
            physical_override.as_os_str().to_owned(),
            OsString::from("--out"),
            blob.as_os_str().to_owned(),
            OsString::from("--report"),
            report.as_os_str().to_owned(),
        ])
        .unwrap();
        let Command::Atmosphere {
            pack,
            source_manifest,
            clouds_override,
            out,
            report,
        } = cli.command
        else {
            panic!("expected atmosphere command");
        };
        let compiled = synthetic_compiled();
        compile_atmosphere_command(
            &pack,
            &source_manifest,
            clouds_override.as_deref(),
            &out,
            &report,
            |actual_pack, manifest_bytes, options| {
                assert_eq!(actual_pack, pack);
                assert_eq!(manifest_bytes, br#"{"artifact_policy":"local-only"}"#);
                assert_eq!(options.clouds_override, Some(physical_override.as_path()));
                Ok(compiled.clone())
            },
        )
        .unwrap();

        let blob_bytes = fs::read(&out).unwrap();
        let runtime = RuntimeAtmosphereAssets::decode(&blob_bytes).unwrap();
        assert_eq!(runtime.textures(), compiled.textures.as_ref());
        let report_text = fs::read_to_string(&report).unwrap();
        let value: serde_json::Value = serde_json::from_str(&report_text).unwrap();
        assert_eq!(
            value["textures"][2]["source_path"],
            "textures/environment/clouds.png"
        );
        assert_eq!(
            value["textures"][2]["source_sha256"],
            hex(&compiled.textures[2].source_sha256)
        );
        assert_eq!(
            value["textures"][2]["pixels_sha256"],
            hex(&compiled.textures[2].pixels_sha256)
        );
        assert!(!report_text.contains(&physical_override.display().to_string()));
    }

    fn synthetic_compiled() -> CompiledAtmosphereAssets {
        let specs = [
            (AtmosphereRole::Sun, "textures/environment/sun.png", 32, 32),
            (
                AtmosphereRole::MoonPhases,
                "textures/environment/moon_phases.png",
                128,
                64,
            ),
            (
                AtmosphereRole::Clouds,
                "textures/environment/clouds.png",
                256,
                256,
            ),
        ];
        let textures = specs
            .into_iter()
            .enumerate()
            .map(|(index, (role, source_path, width, height))| {
                let rgba8 =
                    vec![index as u8 + 1; width as usize * height as usize * 4].into_boxed_slice();
                AtmosphereTexture {
                    role,
                    source_path: source_path.into(),
                    source_bytes: index as u32 + 1,
                    source_sha256: [index as u8 + 1; 32],
                    pixels_sha256: Sha256::digest(&rgba8).into(),
                    width,
                    height,
                    rgba8,
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        CompiledAtmosphereAssets {
            source_manifest_sha256: [0x44; 32],
            textures,
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
