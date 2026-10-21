#![expect(
    clippy::print_stdout,
    reason = "registryc reports Java mapping coverage on stdout"
)]

//! Compiles exported library facts into the active client registry carriers.

use asset_compiler::registry_policy::{self, Result};
use clap::{Parser, ValueEnum};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, ValueEnum)]
enum Kind {
    Entities,
    Blocks,
    Physics,
    Biomes,
    Items,
    BlockEntities,
    Events,
    Identities,
    JavaMapping,
    Presentation,
    All,
}
#[derive(Parser)]
struct Args {
    #[arg(long)]
    root: PathBuf,
    #[arg(long)]
    facts: Option<PathBuf>,
    #[arg(long)]
    archive: Option<PathBuf>,
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
    if matches!(args.kind, Kind::JavaMapping) {
        let summary =
            registry_policy::java_mapping::write(&args.root, args.archive.as_deref(), &args.out)?;
        println!(
            "Mapped {} of {} block states ({} from a similar state) and {} of {} biomes to the {} Java Edition biomes, {} Java entity types to {} Bedrock entities and {} Java items to {} Bedrock items",
            summary.mapped_states,
            summary.states,
            summary.approximate_states,
            summary.mapped_biomes,
            summary.biomes,
            summary.java_biomes,
            summary.entities,
            summary.bedrock_entities,
            summary.items,
            summary.bedrock_items
        );
        if args.check {
            let expected = args.root.join(assets::facts::java_mapping::SOURCE_PATH);
            compare(&args.out, &expected)?;
            compare(
                &args.out.with_extension("sha256"),
                &expected.with_extension("sha256"),
            )?;
        }
        return Ok(());
    }
    if matches!(args.kind, Kind::BlockEntities) {
        registry_policy::block_entities::write(
            &args.root,
            required(&args.breg, "--breg")?,
            &args.out,
        )?;
        if args.check {
            compare(
                &args.out,
                &args.root.join(assets::facts::BLOCK_ENTITY_FACTS_PATH),
            )?;
        }
        return Ok(());
    }
    if matches!(args.kind, Kind::Entities) {
        if let Some(archive) = &args.archive {
            let bytes = registry_policy::entities::components_from_archive(archive)?;
            let path = args.root.join(registry_policy::entities::COMPONENTS_PATH);
            std::fs::create_dir_all(path.parent().ok_or("components parent missing")?)?;
            std::fs::write(path, bytes)?;
        }
        return registry_policy::entities::compile(
            &args.root,
            &args.out,
            &std::fs::read(
                args.root
                    .join("crates/asset-compiler/data/registry-policy.json"),
            )?,
        );
    }
    if matches!(args.kind, Kind::Items) {
        registry_policy::items::write(&args.root, &args.out, args.manifest.as_deref())?;
        if args.check {
            let expected = args.root.join(assets::facts::ITEM_FACTS_SOURCE_PATH);
            compare(&args.out, &expected)?;
            compare(
                &args.out.with_extension("sha256"),
                &expected.with_extension("sha256"),
            )?;
            if let Some(expansion) = &args.manifest {
                compare(
                    expansion,
                    &args.root.join(assets::facts::ITEM_FACTS_EXPANSION_PATH),
                )?;
            }
        }
        return Ok(());
    }
    if matches!(args.kind, Kind::Identities) {
        registry_policy::identities::write(&args.out)?;
        if args.check {
            compare(
                &args.out,
                &args.root.join(registry_policy::identities::GENERATED_PATH),
            )?;
        }
        return Ok(());
    }
    if matches!(args.kind, Kind::Presentation) {
        registry_policy::presentation::write(&args.out)?;
        if args.check {
            compare(
                &args.out,
                &args
                    .root
                    .join(registry_policy::presentation::GENERATED_PATH),
            )?;
        }
        return Ok(());
    }
    if matches!(args.kind, Kind::Events) {
        registry_policy::events::write(&args.out)?;
        if args.check {
            compare(
                &args.out,
                &args.root.join(registry_policy::events::GENERATED_PATH),
            )?;
        }
        return Ok(());
    }
    let facts = registry_policy::read_facts(required(&args.facts, "--facts")?)?;
    match args.kind {
        Kind::Entities | Kind::Identities | Kind::Presentation | Kind::JavaMapping => {
            unreachable!()
        }
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
        Kind::Items | Kind::BlockEntities | Kind::Events => unreachable!(),
    }
    Ok(())
}

/// Rejects a regenerated artifact that differs from its reviewed bytes.
fn compare(actual: &Path, expected: &Path) -> Result<()> {
    if std::fs::read(actual)? != std::fs::read(expected)? {
        return Err(format!("regeneration changed {}", expected.display()).into());
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
    registry_policy::entities::compile(
        &args.root,
        &args.out,
        &std::fs::read(
            args.root
                .join("crates/asset-compiler/data/registry-policy.json"),
        )?,
    )?;
    let entity_path = PathBuf::from(assets::facts::entity::SOURCE_PATH);
    let entity_expansion = PathBuf::from(assets::facts::entity::EXPANSION_PATH);
    let item_path = PathBuf::from(assets::facts::ITEM_FACTS_SOURCE_PATH);
    let item_expansion = PathBuf::from(assets::facts::ITEM_FACTS_EXPANSION_PATH);
    registry_policy::items::write(
        &args.root,
        &args.out.join(&item_path),
        Some(&args.out.join(&item_expansion)),
    )?;
    let block_entities = PathBuf::from(assets::facts::BLOCK_ENTITY_FACTS_PATH);
    registry_policy::block_entities::write(
        &args.root,
        &args.out.join(&block_path),
        &args.out.join(&block_entities),
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
    registry_policy::events::write(&args.out.join(registry_policy::events::GENERATED_PATH))?;
    registry_policy::identities::write(
        &args.out.join(registry_policy::identities::GENERATED_PATH),
    )?;
    registry_policy::presentation::write(
        &args.out.join(registry_policy::presentation::GENERATED_PATH),
    )?;
    if args.check {
        for path in [
            &target_path,
            &entity_path,
            &entity_expansion,
            &block_entities,
            &PathBuf::from(registry_policy::events::GENERATED_PATH),
            &PathBuf::from(registry_policy::identities::GENERATED_PATH),
            &PathBuf::from(registry_policy::presentation::GENERATED_PATH),
            &PathBuf::from("crates/assets/data/facts-expansion-v2193.tsv"),
            &PathBuf::from(assets::facts::BLOCK_PLACEMENT_SOURCE_PATH),
            &PathBuf::from("crates/assets/data/vocabulary-v2193.txt"),
            &item_path,
            &item_expansion,
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
            (&entity_path, false),
            (&item_path, false),
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
