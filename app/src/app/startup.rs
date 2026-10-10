//! Startup carrier loading and required registry verification.
use super::{PHYSICS_REGISTRY_GENERATION_GUIDANCE, PHYSICS_REGISTRY_SHA256};
use crate::asset_startup::{
    self, AssetSelection, AssetStartupError, LoadTimes, LoadedAssets, LoadedHudAssets,
    LoadedIconAssets, LoadedLangAssets, join,
};
use crate::movement::PhysicsCollisionRegistries;
use anyhow::{Context, Result, bail};
use assets::{
    RuntimeActorCatalog, RuntimeAudioCatalog, RuntimeAudioPcm, RuntimeBlockEntityAssets,
    RuntimeEquipmentCatalog, RuntimeParticleAssets, RuntimeUiAssets,
};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, sync::Arc, time::Instant};

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
    pub(super) ui: Result<Arc<RuntimeUiAssets>>,
    pub(super) weather: render::WeatherTextureAssets,
    pub(super) collision: Result<PhysicsCollisionRegistries>,
}

/// The carriers bound to the decoded entity catalog.
pub(super) struct CoreCarriers {
    pub(super) assets: LoadedAssets,
    pub(super) actor: Result<RuntimeActorCatalog, AssetStartupError>,
    pub(super) equipment: Option<Arc<RuntimeEquipmentCatalog>>,
}

pub(super) struct AudioCarriers {
    pub(super) catalog: Option<Arc<RuntimeAudioCatalog>>,
    pub(super) pcm: Option<Arc<RuntimeAudioPcm>>,
    pub(super) sound_bank: Option<crate::audio::SoundBank>,
}

/// Loads every startup carrier in parallel and logs each one's load time.
pub(super) fn load_startup_carriers(
    selection: AssetSelection,
    physics_registry: &Path,
) -> StartupCarriers {
    let start = Instant::now();
    let times = LoadTimes::default();
    let carriers = load_with(selection, physics_registry, &times);
    eprintln!("{}", times.summary(start.elapsed()));
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
            times.time("ui", || {
                client_ui::ui_runtime::json_ui_assets::require_ui_assets(world)
            })
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
    let assets = asset_startup::load_runtime_assets_timed(selection, times)?;
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
    })
}

/// The sound-definition catalog, then the PCM carrier and sound bank bound to it. Absent
/// carriers degrade to silence; a present-but-invalid catalog or PCM carrier fails startup.
fn load_audio(world: &Path, times: &LoadTimes) -> Result<AudioCarriers> {
    let loaded = match times.time("audio", || asset_startup::load_audio_assets(world)) {
        Ok(Some(loaded)) => {
            eprintln!("{}", loaded.startup_summary());
            Some(loaded)
        }
        Ok(None) => {
            eprintln!(
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
        crate::audio::SoundBank::open(&crate::audio::sound_bank_path(world), catalog.clone())
    }) {
        Ok(Some(mut bank)) => {
            times.time("sound prewarm queue", || bank.prewarm_common());
            eprintln!("loaded sound bank ({} sound files)", bank.file_count());
            Some(bank)
        }
        Ok(None) => {
            eprintln!(
                "optional sound bank was not found; run `make audio-bank-assets` (or `make assets`) to enable playback"
            );
            None
        }
        Err(error) => {
            eprintln!("sound bank unusable, audio stays silent: {error}");
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
    let expected_protocol = asset_startup::active_content_registry_protocol();
    let preg = read_verified_physics_registry(
        physics_registry,
        PHYSICS_REGISTRY_SHA256,
        expected_protocol,
    )?;
    PhysicsCollisionRegistries::bind_coherent_assets(
        asset_startup::pinned_block_registry_bytes(),
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
    use crate::asset_startup::{
        AssetPathSource, actor_asset_path, atmosphere_asset_path, entity_asset_path,
        test_carriers::{synthetic_atmosphere_blob, synthetic_entity_blob},
    };
    use std::path::PathBuf;

    struct Directory(PathBuf);
    impl Directory {
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
                "equipment",
                "font",
                "hud",
                "icon",
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
                "--phase3-evidence-target",
                "Bds",
                "--acceptance-seconds",
                "1",
                "--metrics-out",
                "metrics.json",
                "--phase3-candidate-physics",
            ],
        ] {
            let ParseOutcome::Run(args) =
                ClientArgs::parse_from(std::iter::once("bedrock-client").chain(flags)).unwrap()
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
