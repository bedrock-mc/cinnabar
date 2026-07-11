use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};

use assets::{
    AssetError, MATERIAL_FLAG_ALPHA_CUTOUT, compile_pack_with_biomes, encode_blob,
    read_biome_registry, read_registry, write_blob_atomic,
};
use clap::{Parser, Subcommand};

const MAX_REGISTRY_FILE_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    about = "Compile verified local Bedrock resource-pack assets",
    after_help = "Compile inputs:\n  assetc compile --pack <RESOURCE_PACK> --registry <BLOCK_REGISTRY_BIN> --biome-registry <BIOME_REGISTRY_BIN> --out <IGNORED_DIR>/vanilla-v1001.mcbea"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Compile a resource pack and Dragonfly registry into a runtime blob.
    Compile {
        /// Root containing blocks.json and the textures directory.
        #[arg(long)]
        pack: PathBuf,
        /// BREG1002 registry exported by tools/registrygen.
        #[arg(long)]
        registry: PathBuf,
        /// BIOREG01 registry exported by tools/registrygen.
        #[arg(long)]
        biome_registry: PathBuf,
        /// Ignored/local output path, conventionally ending in .mcbea.
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command {
        Command::Compile {
            pack,
            registry,
            biome_registry,
            out,
        } => {
            let registry_bytes = read_bounded(&registry)?;
            let records = read_registry(&registry_bytes)?;
            let biome_registry_bytes = read_bounded(&biome_registry)?;
            let biome_records = read_biome_registry(&biome_registry_bytes)?;
            let behavior_pack = pack
                .parent()
                .ok_or("resource-pack path has no parent for behavior_pack")?
                .join("behavior_pack");
            let compiled =
                compile_pack_with_biomes(&pack, &behavior_pack, &records, &biome_records)?;
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
                compiled.textures.layers,
                compiled.biomes.rules.len(),
                out.display()
            );
        }
    }
    Ok(())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, AssetError> {
    let file = File::open(path).map_err(|source| AssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_REGISTRY_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > MAX_REGISTRY_FILE_BYTES {
        return Err(AssetError::Io {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidData,
                format!("registry exceeds the {MAX_REGISTRY_FILE_BYTES}-byte compiler input limit"),
            ),
        });
    }
    Ok(bytes)
}
