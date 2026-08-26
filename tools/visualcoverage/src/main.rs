use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use visualcoverage::{
    AllowlistEntry, analyze_bytes, baseline_from_snapshot, gallery_inventory_bytes, parse_baseline,
    ratchet_protocol_1001, strict_bytes, write_deterministic_json_atomic,
};

const MAX_REGISTRY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ASSET_BYTES: u64 = 16 * 1024 * 1024;
const MAX_BASELINE_BYTES: u64 = visualcoverage::MAX_BASELINE_BYTES as u64;
const MAX_ALLOWLIST_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Parser)]
#[command(about = "Legacy protocol-1001 visual-coverage evidence tools")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generates a historical protocol-1001 diagnostic baseline.
    LegacyBaseline1001 {
        #[arg(long)]
        registry: PathBuf,
        #[arg(long)]
        assets: PathBuf,
        #[arg(long)]
        invisible_allowlist: PathBuf,
        #[arg(long = "out")]
        out: PathBuf,
    },
    /// Replays the historical protocol-1001 inventory ratchet.
    LegacyRatchet1001 {
        #[arg(long)]
        registry: PathBuf,
        #[arg(long)]
        assets: PathBuf,
        #[arg(long)]
        baseline: PathBuf,
        #[arg(long = "out")]
        out: PathBuf,
    },
    /// Replays the historical protocol-1001 strict visual-route gate.
    LegacyStrict1001 {
        #[arg(long)]
        registry: PathBuf,
        #[arg(long)]
        assets: PathBuf,
        #[arg(long)]
        baseline: PathBuf,
        #[arg(long = "out")]
        out: PathBuf,
    },
    /// Compiles the historical protocol-1001 GPU gallery inventory.
    LegacyGalleryInventory1001 {
        #[arg(long)]
        registry: PathBuf,
        #[arg(long)]
        assets: PathBuf,
        #[arg(long)]
        baseline: PathBuf,
        #[arg(long = "out")]
        out: PathBuf,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Cli { command } = Cli::parse();
    match command {
        Command::LegacyBaseline1001 {
            registry,
            assets,
            invisible_allowlist,
            out,
        } => {
            let snapshot = analyze_bytes(
                &read_bounded(&registry, MAX_REGISTRY_BYTES, "registry")?,
                &read_bounded(&assets, MAX_ASSET_BYTES, "asset blob")?,
            )?;
            let allowlist: Vec<AllowlistEntry> = serde_json::from_slice(&read_bounded(
                &invisible_allowlist,
                MAX_ALLOWLIST_BYTES,
                "invisible allowlist",
            )?)?;
            let baseline = baseline_from_snapshot(&snapshot, allowlist)?;
            write_deterministic_json_atomic(&out, &baseline)?;
        }
        Command::LegacyRatchet1001 {
            registry,
            assets,
            baseline,
            out,
        } => {
            let registry_bytes = read_bounded(&registry, MAX_REGISTRY_BYTES, "registry")?;
            let assets_bytes = read_bounded(&assets, MAX_ASSET_BYTES, "asset blob")?;
            let baseline =
                parse_baseline(&read_bounded(&baseline, MAX_BASELINE_BYTES, "baseline")?)?;
            let report =
                ratchet_protocol_1001(analyze_bytes(&registry_bytes, &assets_bytes)?, &baseline)?;
            write_deterministic_json_atomic(&out, &report)?;
        }
        Command::LegacyStrict1001 {
            registry,
            assets,
            baseline,
            out,
        } => {
            let registry_bytes = read_bounded(&registry, MAX_REGISTRY_BYTES, "registry")?;
            let assets_bytes = read_bounded(&assets, MAX_ASSET_BYTES, "asset blob")?;
            let baseline =
                parse_baseline(&read_bounded(&baseline, MAX_BASELINE_BYTES, "baseline")?)?;
            let report = strict_bytes(&registry_bytes, &assets_bytes, &baseline)?;
            write_deterministic_json_atomic(&out, &report)?;
        }
        Command::LegacyGalleryInventory1001 {
            registry,
            assets,
            baseline,
            out,
        } => {
            let report = gallery_inventory_bytes(
                &read_bounded(&registry, MAX_REGISTRY_BYTES, "registry")?,
                &read_bounded(&assets, MAX_ASSET_BYTES, "asset blob")?,
                &read_bounded(&baseline, MAX_BASELINE_BYTES, "baseline")?,
            )?;
            write_deterministic_json_atomic(&out, &report)?;
        }
    }
    Ok(())
}

fn read_bounded(path: &Path, max_bytes: u64, label: &str) -> io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    if length > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{label} exceeds {max_bytes}-byte ceiling"),
        ));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(max_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{label} grew beyond {max_bytes}-byte ceiling while reading"),
        ));
    }
    Ok(bytes)
}
