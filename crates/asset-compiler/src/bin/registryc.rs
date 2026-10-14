//! Compiles exported library facts into the active client registry carriers.

use asset_compiler::registry_policy::{self, Result};
use clap::{Parser, ValueEnum};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, ValueEnum)]
enum Kind {
    Blocks,
    Physics,
    Biomes,
    All,
}
#[derive(Parser)]
struct Args {
    #[arg(long)]
    root: PathBuf,
    #[arg(long)]
    facts: PathBuf,
    #[arg(long, value_enum)]
    kind: Kind,
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    legacy: Option<PathBuf>,
    #[arg(long)]
    allowlist: Option<PathBuf>,
    #[arg(long)]
    light_out: Option<PathBuf>,
    #[arg(long)]
    manifest: Option<PathBuf>,
    #[arg(long)]
    breg: Option<PathBuf>,
    #[arg(long)]
    sha: Option<PathBuf>,
    #[arg(long)]
    check: bool,
}

/// Dispatches a compiler pass or regenerates every carrier for an exact byte comparison.
fn main() -> Result<()> {
    let args = Args::parse();
    let facts = registry_policy::read_facts(&args.facts)?;
    match args.kind {
        Kind::Blocks => registry_policy::blocks(
            &args.root,
            &facts,
            required(&args.legacy, "--legacy")?,
            required(&args.allowlist, "--allowlist")?,
            &args.out,
            required(&args.light_out, "--light-out")?,
            required(&args.manifest, "--manifest")?,
        )?,
        Kind::Physics => registry_policy::physics(
            &args.root,
            &facts,
            required(&args.breg, "--breg")?,
            &args.out,
            args.sha.as_deref(),
            args.manifest.as_deref(),
        )?,
        Kind::Biomes => registry_policy::biomes(
            &args.root,
            &facts,
            required(&args.allowlist, "--allowlist")?,
            &args.out,
            required(&args.manifest, "--manifest")?,
        )?,
        Kind::All => all(&args, &facts)?,
    }
    Ok(())
}

/// Rejects missing destinations before a selected pass starts writing.
fn required<'a>(path: &'a Option<PathBuf>, flag: &str) -> Result<&'a Path> {
    path.as_deref()
        .ok_or_else(|| format!("{flag} is required").into())
}

/// Regenerates into one directory and optionally compares every byte with the tracked carrier.
fn all(args: &Args, facts: &registry_policy::Facts) -> Result<()> {
    let mut target: serde_json::Value = serde_json::from_slice(&std::fs::read(
        args.root.join("assets/bedrock-target.json"),
    )?)?;
    let block: serde_json::Value = serde_json::from_slice(&std::fs::read(
        args.root.join("assets/block-projection-v2193.json"),
    )?)?;
    let biome: serde_json::Value = serde_json::from_slice(&std::fs::read(
        args.root.join("assets/biome-projection-v2193.json"),
    )?)?;
    let artifact = |name: &str| -> Result<PathBuf> {
        Ok(PathBuf::from(
            target["artifacts"][name]
                .as_str()
                .ok_or("target artifact path missing")?,
        ))
    };
    let block_path = artifact("block_registry")?;
    let light_path = artifact("light_registry")?;
    let biome_path = artifact("biome_registry")?;
    let physics_path = artifact("physics_registry")?;
    let filename = physics_path
        .file_name()
        .ok_or("physics filename missing")?
        .to_string_lossy();
    let seed_path =
        physics_path.with_file_name(filename.replace("block-physics-", "block-physics-seed-"));
    let block_manifest = PathBuf::from("assets/block-projection-v2193.json");
    let biome_manifest = PathBuf::from("assets/biome-projection-v2193.json");
    registry_policy::blocks(
        &args.root,
        facts,
        &args
            .root
            .join("crates/assets/data/block-registry-v1001.bin"),
        &args.root.join(
            block["allowlist"]["path"]
                .as_str()
                .ok_or("retail item path missing")?,
        ),
        &args.out.join(&block_path),
        &args.out.join(&light_path),
        &args.out.join(&block_manifest),
    )?;
    registry_policy::physics(
        &args.root,
        facts,
        &args.out.join(&block_path),
        &args.out.join(&seed_path),
        None,
        Some(&args.out.join(&block_manifest)),
    )?;
    registry_policy::biomes(
        &args.root,
        facts,
        &args.root.join(
            biome["allowlist"]["path"]
                .as_str()
                .ok_or("retail biome path missing")?,
        ),
        &args.out.join(&biome_path),
        &args.out.join(&biome_manifest),
    )?;
    let breg_bytes = std::fs::read(args.out.join(&block_path))?;
    let seed_bytes = std::fs::read(args.out.join(&seed_path))?;
    let protocol = assets::registry_header_protocol(&breg_bytes)?;
    let records = assets::read_registry_for_protocol(&breg_bytes, protocol)?;
    let seed = assets::read_physics_registry_seed_for_protocol(
        &seed_bytes,
        &breg_bytes,
        &records,
        protocol,
    )?;
    let final_bytes =
        asset_compiler::block_shapes::compile_registry(protocol, &breg_bytes, &records, &seed)?;
    let final_path = args.out.join(&physics_path);
    std::fs::write(&final_path, &final_bytes)?;
    use sha2::{Digest, Sha256};
    std::fs::write(
        final_path.with_extension("sha256"),
        format!("{:x}\n", Sha256::digest(&final_bytes)),
    )?;
    for (key, path) in [
        ("block_registry", &block_path),
        ("light_registry", &light_path),
        ("biome_registry", &biome_path),
        ("physics_registry", &physics_path),
    ] {
        target["hashes"][key] =
            format!("{:x}", Sha256::digest(std::fs::read(args.out.join(path))?)).into();
    }
    let target_path = PathBuf::from("assets/bedrock-target.json");
    std::fs::write(
        args.out.join(&target_path),
        serde_json::to_string_pretty(&target)? + "\n",
    )?;
    if args.check {
        for path in [
            &target_path,
            &PathBuf::from("crates/assets/data/facts-expansion-v2193.tsv"),
            &PathBuf::from("crates/assets/data/vocabulary-v2193.txt"),
            &block_path,
            &light_path,
            &biome_path,
            &seed_path,
            &physics_path,
            &block_manifest,
            &biome_manifest,
        ] {
            if std::fs::read(args.out.join(path))? != std::fs::read(args.root.join(path))? {
                return Err(format!("regeneration changed {}", path.display()).into());
            }
        }
        for (path, filename) in [
            (&block_path, true),
            (&light_path, true),
            (&biome_path, false),
            (&seed_path, false),
            (&physics_path, false),
        ] {
            let sha = if filename {
                path.with_file_name(format!(
                    "{}.sha256",
                    path.file_name()
                        .ok_or("carrier filename missing")?
                        .to_string_lossy()
                ))
            } else {
                path.with_extension("sha256")
            };
            if std::fs::read(args.out.join(&sha))? != std::fs::read(args.root.join(&sha))? {
                return Err(format!("regeneration changed {}", sha.display()).into());
            }
        }
    }
    Ok(())
}
