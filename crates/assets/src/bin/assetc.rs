use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};

use assets::{
    AnimationInventory, AssetError, AtmosphereRole, MATERIAL_FLAG_ALPHA_CUTOUT,
    compile_atmosphere_assets, compile_pack_with_biomes, encode_atmosphere_blob, encode_blob,
    inspect_animation_inventory, read_biome_registry, read_light_registry, read_registry,
    write_blob_atomic,
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
            out,
            report,
        } => {
            let manifest_bytes = read_bounded_with_limit(
                &source_manifest,
                MAX_SOURCE_MANIFEST_BYTES,
                "source manifest",
            )?;
            let source =
                serde_json::from_slice::<serde_json::Value>(&manifest_bytes).map_err(|source| {
                    AssetError::Json {
                        path: source_manifest.clone(),
                        source,
                    }
                })?;
            let compiled = compile_atmosphere_assets(&pack, &manifest_bytes)?;
            let blob = encode_atmosphere_blob(&compiled)?;
            let texture_reports = compiled
                .textures
                .iter()
                .map(|texture| {
                    Ok(AtmosphereTextureReport {
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
                })
                .collect::<Result<Vec<_>, AssetError>>()?
                .into_boxed_slice();
            let report_data = AtmosphereReport {
                schema: 1,
                source,
                source_manifest_sha256: hex(&compiled.source_manifest_sha256).into_boxed_str(),
                blob_sha256: format!("{:x}", Sha256::digest(&blob)).into_boxed_str(),
                textures: texture_reports,
            };
            let mut report_bytes =
                serde_json::to_vec_pretty(&report_data).map_err(|source| AssetError::Json {
                    path: report.clone(),
                    source,
                })?;
            report_bytes.push(b'\n');
            validate_output_bundle(&out, &report)?;
            write_blob_atomic(&out, &blob)?;
            write_blob_atomic(&report, &report_bytes)?;
            println!(
                "compiled {} pinned atmosphere textures to {} and {}",
                report_data.textures.len(),
                out.display(),
                report.display()
            );
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn validate_output_bundle(blob: &Path, report: &Path) -> Result<(), AssetError> {
    if blob == report {
        return Err(AssetError::Io {
            path: blob.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "atmosphere blob and report paths must be distinct",
            ),
        });
    }
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
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(AssetError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
    Ok(())
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
