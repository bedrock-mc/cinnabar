//! Command-line surface: the subcommand enum and its help text.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    about = "Compile verified local Bedrock resource-pack assets",
    after_help = "Compile inputs:\n  assetc compile --pack <RESOURCE_PACK> --source-manifest <VANILLA_SOURCE_JSON> --registry <BLOCK_REGISTRY_BIN> --light-registry <LIGHT_REGISTRY_BIN> --biome-registry <BIOME_REGISTRY_BIN> --out <IGNORED_DIR>/vanilla-v2193.mcbea\n\nAtmosphere inputs:\n  assetc atmosphere --pack <RESOURCE_PACK> --source-manifest <VANILLA_SOURCE_JSON> --out <IGNORED_DIR>/vanilla-v1.mcbeatm --report <IGNORED_DIR>/atmosphere-assets.json\n\nEntity catalog and geometry payloads:\n  assetc entity-assets --pack <RESOURCE_PACK> --source-manifest <VANILLA_SOURCE_JSON> --out <IGNORED_DIR>/vanilla-v1.mcbeent --report <IGNORED_DIR>/entity-assets.json\n\nDormant sound-definition lookup:\n  assetc audio-assets --pack <RESOURCE_PACK> --source-manifest <VANILLA_SOURCE_JSON> --out <IGNORED_DIR>/vanilla-v1.mcbeaud --report <IGNORED_DIR>/audio-assets.json\n\nBitmap font payloads:\n  assetc font-assets --pack <RESOURCE_PACK> --source-manifest <VANILLA_SOURCE_JSON> --out <IGNORED_DIR>/vanilla-v1.mcbefont --report <IGNORED_DIR>/font-assets.json\n\nPinned official Mojang sample HUD sprites:\n  assetc hud-assets --pack <RESOURCE_PACK> --source-manifest assets/hud-source-v2193.json --out <IGNORED_DIR>/vanilla-v1.mcbehud --report <IGNORED_DIR>/hud-assets.json\n\nJSON-UI atlas, sidecars, and raw ui json:\n  assetc ui-assets --pack <RESOURCE_PACK> --source-manifest <VANILLA_SOURCE_JSON> --out <IGNORED_DIR>/vanilla-v1.mcbeui --report <IGNORED_DIR>/ui-assets.json\n\nParticle effects and textures:\n  assetc particle-assets --pack <RESOURCE_PACK> --source-manifest <VANILLA_SOURCE_JSON> --out <IGNORED_DIR>/vanilla-v1.mcbept --report <IGNORED_DIR>/particle-assets.json\n\nAnimation inventory:\n  assetc animation-inventory --pack <RESOURCE_PACK> --source-manifest <VANILLA_SOURCE_JSON> --max-layers-per-page 2048 --max-pages 2 --out <IGNORED_DIR>/animation-inventory.json"
)]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) command: Command,
}

#[derive(Debug, Subcommand)]
pub(super) enum Command {
    /// Compile the fixed vanilla sun, moon-phase, and cloud textures.
    Atmosphere {
        /// Root of the pinned vanilla resource pack.
        #[arg(long)]
        pack: PathBuf,
        /// Tracked manifest that pins the local resource-pack source.
        #[arg(long)]
        source_manifest: PathBuf,
        #[arg(long)]
        clouds_override: Option<PathBuf>,
        /// Ignored/local MCBEATM2 output path.
        #[arg(long)]
        out: PathBuf,
        /// Ignored/local deterministic JSON provenance report path.
        #[arg(long)]
        report: PathBuf,
    },
    /// Compile bounded entity geometry, animation, controller, and texture metadata.
    EntityAssets {
        /// Root of the pinned vanilla resource pack.
        #[arg(long)]
        pack: PathBuf,
        /// Tracked manifest that pins the local resource-pack source.
        #[arg(long)]
        source_manifest: PathBuf,
        /// Ignored/local MCBEENT3 output path.
        #[arg(long)]
        out: PathBuf,
        /// Ignored/local deterministic JSON provenance report path.
        #[arg(long)]
        report: PathBuf,
    },
    /// Compile the pinned pack's attachable equipment bindings into the
    /// equipment carrier, pinned to the sibling entity carrier.
    EquipmentAssets {
        /// Root of the pinned vanilla resource pack.
        #[arg(long)]
        pack: PathBuf,
        /// Tracked manifest that pins the local resource-pack source.
        #[arg(long)]
        source_manifest: PathBuf,
        /// Ignored/local MCBEEQP1 output path.
        #[arg(long)]
        out: PathBuf,
        /// Ignored/local deterministic JSON provenance report path.
        #[arg(long)]
        report: PathBuf,
        /// Optional vanilla behavior pack root, read for item use durations.
        #[arg(long)]
        behavior_pack: Option<PathBuf>,
    },
    /// Compile bounded bitmap-font metrics and compressed texture pages.
    FontAssets {
        /// Root of the pinned vanilla resource pack.
        #[arg(long, required_unless_present = "font", conflicts_with = "font")]
        pack: Option<PathBuf>,
        /// Outline font pinned by the source manifest, rasterized with its own advances.
        #[arg(long)]
        font: Option<PathBuf>,
        /// Tracked manifest that pins the local resource-pack source.
        #[arg(long)]
        source_manifest: PathBuf,
        /// Ignored/local MCBEFONT1 output path.
        #[arg(long)]
        out: PathBuf,
        /// Ignored/local deterministic JSON provenance report path.
        #[arg(long)]
        report: PathBuf,
    },
    /// Compile the optional precipitation sheet and End sky carrier.
    WeatherAssets {
        #[arg(long)]
        pack: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Compile the optional starter skin and skin geometry carrier.
    StarterSkinAssets {
        #[arg(long)]
        pack: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Compile the optional hardcore-heart sprite carrier.
    HudExtrasAssets {
        #[arg(long)]
        pack: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    HudAssets {
        #[arg(long)]
        pack: PathBuf,
        #[arg(long)]
        source_manifest: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        report: PathBuf,
    },
    /// Pack block-entity model textures and the version-pinned inventory into
    /// the block-entity carrier.
    BlockEntityAssets {
        #[arg(long)]
        pack: PathBuf,
        #[arg(long)]
        source_manifest: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        report: PathBuf,
    },
    /// Pack the pinned pack's `textures/ui` sprites into atlas pages and store
    /// the nine-slice sidecars plus the raw `ui/*.json` catalog for the JSON-UI
    /// engine. Not yet wired into startup.
    UiAssets {
        #[arg(long)]
        pack: PathBuf,
        #[arg(long)]
        source_manifest: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        report: PathBuf,
    },
    /// Compile the pack's `particles/*.json` effects and particle textures into the
    /// particle carrier.
    ParticleAssets {
        #[arg(long)]
        pack: PathBuf,
        #[arg(long)]
        source_manifest: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        report: PathBuf,
    },
    /// Compile the pinned pack's sprite-routed item icons into the bounded
    /// icon carrier.
    IconAssets {
        #[arg(long)]
        pack: PathBuf,
        #[arg(long)]
        source_manifest: PathBuf,
        /// Optional checked world carrier for ordinary opaque-cube thumbnails.
        #[arg(long)]
        block_assets: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        report: PathBuf,
    },
    /// Compile actor artwork from geometry and material contracts.
    ActorAssets {
        #[arg(long)]
        pack: PathBuf,
        #[arg(long)]
        source_manifest: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        report: PathBuf,
    },
    /// Compile the pinned pack's en_US language table into the bounded
    /// localization carrier.
    LangAssets {
        #[arg(long)]
        pack: PathBuf,
        #[arg(long)]
        source_manifest: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        report: PathBuf,
    },
    /// Compile every other language the pack lists as optional
    /// `<out-dir>/<code>.mcbelang` carriers.
    LanguageAssets {
        #[arg(long)]
        pack: PathBuf,
        #[arg(long)]
        source_manifest: PathBuf,
        #[arg(long)]
        out_dir: PathBuf,
    },
    /// Compile the pinned vanilla sound definitions into a dormant lookup catalog.
    AudioAssets {
        /// Root of the pinned vanilla resource pack.
        #[arg(long)]
        pack: PathBuf,
        /// Tracked manifest that pins the local resource-pack source.
        #[arg(long)]
        source_manifest: PathBuf,
        /// Ignored/local MCBEAUD1 output path.
        #[arg(long)]
        out: PathBuf,
        /// Ignored/local deterministic JSON provenance report path.
        #[arg(long)]
        report: PathBuf,
    },
    /// Pack sound-event routing JSON and every FSB sound file into a streaming bank.
    AudioBank {
        /// Root of the vanilla resource pack.
        #[arg(long)]
        pack: PathBuf,
        /// Ignored/local MCBESND1 output path.
        #[arg(long)]
        out: PathBuf,
        /// Ignored/local deterministic JSON report path.
        #[arg(long)]
        report: PathBuf,
    },
    /// Compile one reviewed sample into finite PCM; does not activate playback.
    AudioPcmAssets {
        #[arg(long)]
        pack: PathBuf,
        #[arg(long)]
        catalog: PathBuf,
        #[arg(long)]
        source_manifest: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        report: PathBuf,
    },
    /// Rasterize a pinned open-licensed outline font into a bounded bitmap carrier.
    OutlineFontAssets {
        /// Exact hash-verified local TTF/OTF source.
        #[arg(long)]
        font: PathBuf,
        /// Exact hash-verified secondary source required by a fallback manifest.
        #[arg(long)]
        fallback_font: Option<PathBuf>,
        /// Compile the primary font alone even when the manifest declares a fallback.
        #[arg(long)]
        primary_only: bool,
        /// Tracked manifest pinning font URL, hash, license, and raster settings.
        #[arg(long)]
        source_manifest: PathBuf,
        /// Ignored/local MCBEFONT1 output path.
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
        /// Tracked manifest that pins the local resource-pack source.
        #[arg(long)]
        source_manifest: PathBuf,
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
    /// Build every stale carrier in the carrier table in one parallel pass.
    Prepare {
        /// Checkout that tracked inputs and the pinned pack resolve against.
        #[arg(long, default_value = ".", conflicts_with = "kit")]
        root: PathBuf,
        /// Installer preparation kit holding the tracked inputs; builds installed carriers only.
        #[arg(long, requires = "workspace")]
        kit: Option<PathBuf>,
        /// Directory the pinned pack unpacks below when building from a kit.
        #[arg(long)]
        workspace: Option<PathBuf>,
        /// Carrier directory; defaults to the checkout's compiled-asset directory.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Carrier names to build, plus the carriers they read.
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        /// Print the stale set as JSON and build nothing.
        #[arg(long)]
        check: bool,
        /// Report progress as JSON lines.
        #[arg(long)]
        json: bool,
        /// Fetch the pinned pack when a stale carrier needs it; confirms the Minecraft EULA.
        #[arg(long)]
        accept_eula: bool,
        #[arg(long)]
        clouds_override: Option<PathBuf>,
    },
    /// Download (when missing), verify and unpack the pinned sample pack below `.local/assets`.
    VanillaPack {
        /// Tracked manifest pinning the pack; its paths resolve against the current directory.
        #[arg(long)]
        source_manifest: PathBuf,
        /// Confirms acceptance of the Minecraft EULA.
        #[arg(long)]
        accept_eula: bool,
    },
}
