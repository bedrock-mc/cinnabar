//! Startup carrier loading and required registry verification.
mod menu;
mod timing;
pub(crate) mod world;
pub(super) use menu::begin;
pub(super) use timing::install_first_frame_log;
use timing::{LoadTimes, join};

use super::{PHYSICS_REGISTRY_GENERATION_GUIDANCE, PHYSICS_REGISTRY_SHA256};
use crate::args;
use anyhow::{Context, Result, bail};
use assets::carriers::startup::{
    self as asset_startup, AssetSelection, AssetStartupError, LoadedAssets, LoadedHudAssets,
    LoadedIconAssets, LoadedLangAssets, select_asset_path_from_environment,
};
use assets::{
    RuntimeActorCatalog, RuntimeAudioCatalog, RuntimeAudioPcm, RuntimeBlockEntityAssets,
    RuntimeEquipmentCatalog, RuntimeParticleAssets,
};
use client_interaction::movement::PhysicsCollisionRegistries;
use client_ui::ui_runtime::json_ui_assets::LoadedUiAssets;
use client_ui::ui_runtime::presentation::PreparedJsonUi;
use launcher::install_layout::InstallLayout;
use launcher_host::session_cleanup::ScopedSessionDirectory;
use sha2::Digest;
use sha2::Sha256;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::Instant,
};

/// Every carrier startup reads, each read and decoded exactly once. Results are checked by the
/// caller in the serial order startup always used, so the first failure reported never changes.
pub(super) struct StartupCarriers {
    pub(super) core: Result<CoreCarriers, AssetStartupError>,
    pub(super) hud: Result<LoadedHudAssets, AssetStartupError>,
    pub(super) icons: Result<LoadedIconAssets, AssetStartupError>,
    pub(super) lang: Result<LoadedLangAssets, AssetStartupError>,
    pub(super) audio: Result<AudioCarriers>,
    pub(super) block_entities: Option<Arc<RuntimeBlockEntityAssets>>,
    pub(super) particles: Option<Arc<RuntimeParticleAssets>>,
    pub(super) ui: Result<PreparingJsonUi>,
    pub(super) weather: render_scene::WeatherTextureAssets,
    pub(super) collision: Result<PhysicsCollisionRegistries>,
}

/// The validated UI carrier's catalog, parsing on its own thread while startup continues.
pub(super) struct PreparingJsonUi(std::thread::JoinHandle<Result<PreparedJsonUi, String>>);

impl PreparingJsonUi {
    /// Parses the validated carrier on a worker, reusing its decoded item glint.
    fn start(assets: LoadedUiAssets) -> Result<Self> {
        std::thread::Builder::new()
            .name("json-ui-catalog".to_owned())
            .spawn(move || PreparedJsonUi::from_loaded(assets))
            .map(Self)
            .context("start the JSON-UI catalog thread")
    }

    /// Waits for the parsed catalog, re-raising a panic from its thread.
    pub(super) fn join(self) -> Result<PreparedJsonUi, String> {
        self.0
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    }
}

/// The carriers bound to the decoded entity catalog.
pub(super) struct CoreCarriers {
    pub(super) assets: LoadedAssets,
    pub(super) actor: Result<RuntimeActorCatalog, AssetStartupError>,
    pub(super) equipment: Option<Arc<RuntimeEquipmentCatalog>>,
    pub(super) authored_texture_loading:
        crate::render_mode::authored_textures::AuthoredTextureLoading,
}

impl CoreCarriers {
    /// Installs the world sidecars and base equipment catalog process-wide. Only the run that
    /// uses these carriers does this, so an unused preload is released in full when dropped.
    fn publish(&mut self) {
        crate::asset_bindings::install_world_sidecars(&mut self.assets);
        if let Some(catalog) = &self.equipment {
            pack_runtime::entity_pack::set_base_equipment_catalog(Arc::clone(catalog));
        }
    }
}

pub(super) struct AudioCarriers {
    pub(super) catalog: Option<Arc<RuntimeAudioCatalog>>,
    pub(super) pcm: Option<Arc<RuntimeAudioPcm>>,
    pub(super) sound_bank: Option<client_audio::audio::SoundBank>,
}

/// Startup carriers loaded before `run`, with the paths they were read from.
struct Preloaded<T> {
    world: PathBuf,
    physics_registry: PathBuf,
    carriers: T,
}

/// The latest preload, until `run` takes or setup discards it.
static PRELOADED: Mutex<Option<Preloaded<menu::MenuBoot>>> = Mutex::new(None);

/// Loads the default startup carriers for the next `run`. A preload of the same files is kept
/// unless `rewritten` says setup has since rewritten them.
pub(crate) fn preload(layout: &InstallLayout, rewritten: bool) {
    let selection = select_asset_path_from_environment(None, &layout.world_assets());
    preload_into(
        &PRELOADED,
        selection,
        &layout.physics_registry,
        rewritten,
        menu::load,
    );
}

/// Fills `slot` with `load`'s carriers unless it already holds a current preload of the same
/// files. A stale preload is released before loading, so two sets are never held at once.
fn preload_into<T>(
    slot: &Mutex<Option<Preloaded<T>>>,
    selection: AssetSelection,
    physics_registry: &Path,
    rewritten: bool,
    load: impl FnOnce(AssetSelection, &Path) -> T,
) {
    {
        let mut held = slot.lock().unwrap_or_else(PoisonError::into_inner);
        if !rewritten
            && held.as_ref().is_some_and(|preloaded| {
                preloaded.world == selection.path && preloaded.physics_registry == physics_registry
            })
        {
            return;
        }
        held.take();
    }
    let world = selection.path.clone();
    let carriers = load(selection, physics_registry);
    *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(Preloaded {
        world,
        physics_registry: physics_registry.to_path_buf(),
        carriers,
    });
}

/// Drops a preload whose carrier files have since been rewritten.
pub(crate) fn discard_preloaded() {
    PRELOADED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
}

/// Empties `slot`, returning its carriers only when they were read from these paths.
fn take_preloaded<T>(
    slot: &Mutex<Option<Preloaded<T>>>,
    world: &Path,
    physics_registry: &Path,
) -> Option<T> {
    let preloaded = slot.lock().unwrap_or_else(PoisonError::into_inner).take()?;
    (preloaded.world == world && preloaded.physics_registry == physics_registry)
        .then_some(preloaded.carriers)
}

/// Loads every startup carrier in parallel and logs each one's load time.
fn load_startup_carriers(selection: AssetSelection, physics_registry: &Path) -> StartupCarriers {
    let start = Instant::now();
    let times = LoadTimes::default();
    let carriers = render_scene::startup_timing::measure("carriers", || {
        load_with(selection, physics_registry, &times)
    });
    bevy::log::info!("{}", times.summary(start.elapsed()));
    carriers
}

fn load_with(
    selection: AssetSelection,
    physics_registry: &Path,
    times: &LoadTimes,
) -> StartupCarriers {
    let world = selection.path.clone();
    let world = world.as_path();
    let manifest = asset_startup::vanilla_source_manifest_json();
    std::thread::scope(|scope| {
        let hud = scope.spawn(|| times.time("hud", || asset_startup::require_hud_assets(world)));
        let icons = scope.spawn(|| {
            times.time("icon", || {
                asset_startup::require_icon_assets(world, manifest)
            })
        });
        let lang = scope.spawn(|| {
            times.time("lang", || {
                asset_startup::require_lang_assets(world, manifest)
            })
        });
        let audio = scope.spawn(|| load_audio(world, times));
        let block_entities = scope.spawn(|| {
            times.time("block entity", || {
                crate::block_entities::load_block_entity_carrier(world)
            })
        });
        let particles = scope.spawn(|| {
            times.time("particle", || {
                crate::particles::load_optional_carrier(world)
            })
        });
        let ui = scope.spawn(|| {
            times
                .time("ui", || {
                    client_ui::ui_runtime::json_ui_assets::require_ui_assets(world)
                })
                .and_then(PreparingJsonUi::start)
        });
        let weather = scope.spawn(|| {
            times.time("weather", || {
                crate::environment::load_optional_weather_textures(world)
            })
        });
        let collision = scope.spawn(|| {
            times.time("physics", || {
                load_collision_registries(physics_registry, world)
            })
        });
        let core = load_core(selection, times);
        StartupCarriers {
            core,
            hud: join(hud),
            icons: join(icons),
            lang: join(lang),
            audio: join(audio),
            block_entities: join(block_entities),
            particles: join(particles),
            ui: join(ui),
            weather: join(weather),
            collision: join(collision),
        }
    })
}

/// The world, atmosphere, entity and font carriers, then the two bound to the entity catalog.
fn load_core(
    selection: AssetSelection,
    times: &LoadTimes,
) -> Result<CoreCarriers, AssetStartupError> {
    let assets = asset_startup::load_runtime_assets_observed(selection, times)?;
    finish_core(assets, times)
}

/// Binds actor and equipment artwork to one admitted entity catalog.
fn finish_core(assets: LoadedAssets, times: &LoadTimes) -> Result<CoreCarriers, AssetStartupError> {
    let authored_texture_loading =
        crate::render_mode::authored_textures::AuthoredTextureLoading::new(
            Arc::clone(&assets.runtime),
            assets.material_keys.clone(),
        );
    let (world, entities) = (assets.selected_path.as_path(), &assets.entities);
    let (actor, equipment) = std::thread::scope(|scope| {
        let equipment = scope.spawn(|| {
            times.time("equipment", || {
                asset_startup::load_optional_equipment_assets(world, entities)
            })
        });
        let actor = times.time("actor", || {
            asset_startup::require_actor_assets(world, entities)
        });
        (actor, join(equipment))
    });
    Ok(CoreCarriers {
        assets,
        actor,
        equipment,
        authored_texture_loading,
    })
}

/// The sound-definition catalog, then the PCM carrier and sound bank bound to it. Absent
/// carriers degrade to silence; a present-but-invalid catalog or PCM carrier fails startup.
fn load_audio(world: &Path, times: &LoadTimes) -> Result<AudioCarriers> {
    let loaded = match times.time("audio", || asset_startup::load_audio_assets(world)) {
        Ok(Some(loaded)) => {
            bevy::log::info!("{}", loaded.startup_summary());
            Some(loaded)
        }
        Ok(None) => {
            bevy::log::warn!(
                "{}",
                asset_startup::audio_assets_missing_notice(&asset_startup::audio_asset_path(world))
            );
            None
        }
        Err(error) => {
            return Err(anyhow::Error::new(error))
                .context("load optional pinned sound-definition carrier");
        }
    };
    let pcm = times
        .time("pcm", || {
            asset_startup::load_audio_pcm_assets(world, loaded.as_ref())
        })
        .context("load optional reviewed finite PCM carrier")?;
    let catalog = loaded.map(|loaded| loaded.into_runtime());
    // Never fatal: absence or damage leaves playback silent.
    let sound_bank = match times.time("sound bank", || {
        client_audio::audio::SoundBank::open(
            &client_audio::audio::sound_bank_path(world),
            catalog.clone(),
        )
    }) {
        Ok(Some(mut bank)) => {
            times.time("sound prewarm queue", || bank.prewarm_common());
            bevy::log::info!("loaded sound bank ({} sound files)", bank.file_count());
            Some(bank)
        }
        Ok(None) => {
            bevy::log::warn!(
                "optional sound bank was not found; run `make audio-bank-assets` (or `make assets`) to enable playback"
            );
            None
        }
        Err(error) => {
            bevy::log::warn!("sound bank unusable, audio stays silent: {error}");
            None
        }
    };
    Ok(AudioCarriers {
        catalog,
        pcm,
        sound_bank,
    })
}

/// The pinned physics registry, bound against the world carrier's block registry.
fn load_collision_registries(
    physics_registry: &Path,
    world: &Path,
) -> Result<PhysicsCollisionRegistries> {
    // One shared authority drives both startup registry gates: the world-carrier provenance pins
    // and this physics binding derive their protocol from it, so a partially flipped carrier set
    // fails closed instead of aliasing live block identities.
    let expected_protocol = assets::active_content_registry_protocol();
    assets::block_presentation_state::pinned_block_presentation_states();
    let preg = read_verified_physics_registry(
        physics_registry,
        PHYSICS_REGISTRY_SHA256,
        expected_protocol,
    )?;
    PhysicsCollisionRegistries::bind_coherent_assets(
        assets::pinned_block_registry_bytes(),
        &preg,
        physics_registry,
        world,
        expected_protocol,
    )
    .context("decode and bind the active-content-protocol collision registries")
}

/// Verifies the installed physics registry against the startup pin.
pub(super) fn read_verified_physics_registry(
    path: &Path,
    expected_sha256: &str,
    expected_protocol: u32,
) -> Result<Vec<u8>> {
    let bytes = fs::read(path).with_context(|| {
        format!(
            "read required protocol-{expected_protocol} physics registry {}; {}",
            path.display(),
            PHYSICS_REGISTRY_GENERATION_GUIDANCE
        )
    })?;
    let actual_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let expected_sha256 = expected_sha256.trim();
    if actual_sha256 != expected_sha256 {
        bail!(
            "protocol-{expected_protocol} physics registry {} is stale or corrupt: expected sha256 {}, got {}; {}",
            path.display(),
            expected_sha256,
            actual_sha256,
            PHYSICS_REGISTRY_GENERATION_GUIDANCE
        );
    }
    Ok(bytes)
}
#[cfg(test)]
mod carrier_tests {
    use super::*;
    use assets::carriers::startup::{
        AssetPathSource, actor_asset_path, atmosphere_asset_path, entity_asset_path,
        test_carriers::{synthetic_atmosphere_blob, synthetic_entity_blob, write_pinned_facts},
    };
    use std::path::PathBuf;

    struct Directory(PathBuf);
    impl Directory {
        /// Creates an isolated carrier set with the required pinned content facts.
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "startup-carriers-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            write_pinned_facts(&path.join(assets::carriers::WORLD.output));
            Self(path)
        }
        fn world(&self) -> PathBuf {
            self.0.join("vanilla-v2193.mcbea")
        }
        fn load(&self, times: &LoadTimes) -> StartupCarriers {
            let selection = AssetSelection {
                path: self.world(),
                source: AssetPathSource::CommandLine,
            };
            load_with(selection, &self.0.join("absent-physics.bin"), times)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn every_startup_carrier_is_loaded_exactly_once() {
        let directory = Directory::new("once");
        let world = directory.world();
        let entity = synthetic_entity_blob(0);
        fs::write(atmosphere_asset_path(&world), synthetic_atmosphere_blob()).unwrap();
        fs::write(entity_asset_path(&world), &entity).unwrap();
        fs::write(
            actor_asset_path(&world),
            assets::encode_actor_catalog(&entity, &[], &[]).unwrap(),
        )
        .unwrap();
        let times = LoadTimes::default();
        let carriers = directory.load(&times);
        assert!(carriers.core.unwrap().actor.is_ok());
        let mut loaded = times.carriers();
        loaded.sort_unstable();
        assert_eq!(
            loaded,
            [
                "actor",
                "atmosphere",
                "audio",
                "block entity",
                "entity",
                "entity facts",
                "equipment",
                "font",
                "hud",
                "icon",
                "item-facts",
                "lang",
                "particle",
                "pcm",
                "physics",
                "sound bank",
                "ui",
                "weather",
                "world",
            ]
        );
    }

    #[test]
    fn a_preload_is_reused_only_for_the_paths_it_was_read_from() {
        let directory = Directory::new("preload");
        let physics = directory.0.join("absent-physics.bin");
        let slot = Mutex::new(None);
        let preload = |slot: &Mutex<Option<Preloaded<StartupCarriers>>>| {
            *slot.lock().unwrap() = Some(Preloaded {
                world: directory.world(),
                physics_registry: physics.clone(),
                carriers: directory.load(&LoadTimes::default()),
            });
        };

        preload(&slot);
        assert!(take_preloaded(&slot, &directory.world(), &physics).is_some());
        assert!(slot.lock().unwrap().is_none(), "a preload is consumed once");
        assert!(take_preloaded(&slot, &directory.world(), &physics).is_none());

        preload(&slot);
        let other_world = directory.0.join("other.mcbea");
        assert!(take_preloaded(&slot, &other_world, &physics).is_none());
        assert!(
            slot.lock().unwrap().is_none(),
            "a mismatched preload is released"
        );

        preload(&slot);
        assert!(
            take_preloaded(&slot, &directory.world(), &directory.0.join("other.bin")).is_none()
        );
    }

    #[test]
    fn a_current_preload_is_kept_instead_of_loading_a_second_set() {
        let directory = Directory::new("preload-reuse");
        let physics = directory.0.join("absent-physics.bin");
        let selection = |path: PathBuf| AssetSelection {
            path,
            source: AssetPathSource::CommandLine,
        };
        let slot = Mutex::new(None);
        let loads = std::cell::Cell::new(0);
        let load = |selection: AssetSelection, physics: &Path| {
            loads.set(loads.get() + 1);
            load_with(selection, physics, &LoadTimes::default())
        };

        preload_into(&slot, selection(directory.world()), &physics, false, load);
        preload_into(&slot, selection(directory.world()), &physics, false, load);
        assert_eq!(loads.get(), 1, "a retried setup reuses the current preload");

        preload_into(&slot, selection(directory.world()), &physics, true, load);
        assert_eq!(loads.get(), 2, "rewritten carriers are loaded again");

        let other = directory.0.join("other.mcbea");
        preload_into(&slot, selection(other.clone()), &physics, false, load);
        assert_eq!(loads.get(), 3, "other carrier files are loaded");
        assert!(take_preloaded(&slot, &other, &physics).is_some());
    }

    #[test]
    fn required_carrier_failures_report_the_serial_first_error_verbatim() {
        let directory = Directory::new("fail-closed");
        let world = directory.world();
        fs::write(entity_asset_path(&world), b"corrupt entity carrier").unwrap();
        let carriers = directory.load(&LoadTimes::default());
        // The corrupt entity carrier fails in parallel, but the absent atmosphere came first.
        let Err(error) = carriers.core else {
            panic!("startup accepted an absent atmosphere carrier");
        };
        assert!(matches!(error, AssetStartupError::AtmosphereRead { .. }));
        assert_eq!(
            error.to_string(),
            asset_startup::load_runtime_assets(AssetSelection {
                path: world.clone(),
                source: AssetPathSource::CommandLine,
            })
            .unwrap_err()
            .to_string()
        );
        let message = |result: Result<_, AssetStartupError>| result.err().unwrap().to_string();
        assert_eq!(
            message(carriers.hud.map(drop)),
            message(asset_startup::require_hud_assets(&world).map(drop))
        );
        let manifest = asset_startup::vanilla_source_manifest_json();
        assert_eq!(
            message(carriers.icons.map(drop)),
            message(asset_startup::require_icon_assets(&world, manifest).map(drop))
        );
        assert_eq!(
            message(carriers.lang.map(drop)),
            message(asset_startup::require_lang_assets(&world, manifest).map(drop))
        );
        assert!(carriers.ui.is_err());
        assert!(format!("{:#}", carriers.collision.err().unwrap()).contains("absent-physics.bin"));

        fs::write(atmosphere_asset_path(&world), synthetic_atmosphere_blob()).unwrap();
        let Err(error) = directory.load(&LoadTimes::default()).core else {
            panic!("startup accepted a corrupt entity carrier");
        };
        assert!(matches!(
            error,
            AssetStartupError::EntityAssetsDecode { .. }
        ));
    }
}

#[cfg(test)]
mod tests {
    use crate::args::{ClientArgs, ParseOutcome};

    #[test]
    fn evidence_options_fail_before_startup_without_the_optional_plugin() {
        for flags in [
            vec!["--acceptance-seconds", "1"],
            vec!["--metrics-out", "metrics.json"],
            vec!["--full-view-teleport-gate"],
            vec!["--require-transparent-presentation"],
            vec!["--transparent-witness-request", "witness.json"],
            vec!["--model-witness-request", "witness.json"],
            vec![
                "--movement-evidence-target",
                "Bds",
                "--acceptance-seconds",
                "1",
                "--metrics-out",
                "metrics.json",
                "--movement-candidate-physics",
            ],
        ] {
            let ParseOutcome::Run(args) =
                ClientArgs::parse_from(std::iter::once("cinnabar").chain(flags)).unwrap()
            else {
                panic!("expected runtime arguments")
            };
            let error = args.validate_acceptance_support(false).unwrap_err();
            assert!(error.to_string().contains("acceptance` feature"));
            args.validate_acceptance_support(true).unwrap();
        }
        ClientArgs::default()
            .validate_acceptance_support(false)
            .unwrap();
    }
}

/// Owns app-derived session directories and only creates operator-provided directories.
pub(super) fn bind_direct_session_directory(
    args: &args::ClientArgs,
    socket_dir: std::path::PathBuf,
) -> Result<ScopedSessionDirectory> {
    if args.address.is_some() && !args.socket_dir_explicit {
        return ScopedSessionDirectory::bind(socket_dir.clone()).with_context(|| {
            format!(
                "prepare direct-connect session directory {}",
                socket_dir.display()
            )
        });
    }
    if args.address.is_some() {
        fs::create_dir_all(&socket_dir)
            .with_context(|| format!("prepare socket directory {}", socket_dir.display()))?;
    }
    Ok(ScopedSessionDirectory::none())
}
