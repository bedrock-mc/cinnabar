use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};

use assets::{AssetError, compile_pack, encode_blob, read_registry, write_blob_atomic};
use clap::{Parser, Subcommand};

const MAX_REGISTRY_FILE_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    about = "Compile verified local Bedrock resource-pack assets",
    after_help = "Compile inputs:\n  assetc compile --pack <RESOURCE_PACK> --registry <REGISTRY_BIN> --out <IGNORED_DIR>/vanilla-v1001.mcbea"
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
            out,
        } => {
            let registry_bytes = read_bounded(&registry)?;
            let records = read_registry(&registry_bytes)?;
            let compiled = compile_pack(&pack, &records)?;
            let blob = encode_blob(&compiled)?;
            write_blob_atomic(&out, &blob)?;
            println!(
                "compiled {} visuals, {} materials, and {} texture layers to {}",
                compiled.visuals.len(),
                compiled.materials.len(),
                compiled.textures.layers,
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
