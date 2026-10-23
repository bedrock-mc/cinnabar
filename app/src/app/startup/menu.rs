//! Loads menu carriers without admitting world decoding.

use super::*;

/// Menu data plus the inputs for a world bundle that has not started loading.
pub(in crate::app) struct MenuBoot {
    pub assets: Result<asset_startup::MenuAssets, AssetStartupError>,
    pub hud: Result<LoadedHudAssets, AssetStartupError>,
    pub icons: Result<LoadedIconAssets, AssetStartupError>,
    pub lang: Result<LoadedLangAssets, AssetStartupError>,
    pub audio: Result<AudioCarriers>,
    pub ui: Result<PreparingJsonUi>,
    pub world: world::WorldInput,
    pub path: PathBuf,
}

/// Reuses a menu preload; direct connections retain their eager validated world path.
pub(in crate::app) fn begin(
    selection: AssetSelection,
    physics: &Path,
    direct: bool,
) -> Result<MenuBoot> {
    if !direct {
        return Ok(take_preloaded(&PRELOADED, &selection.path, physics)
            .unwrap_or_else(|| load(selection, physics)));
    }
    let path = selection.path.clone();
    let StartupCarriers {
        core,
        hud,
        icons,
        lang,
        audio,
        ui,
        block_entities,
        particles,
        weather,
        collision,
    } = load_startup_carriers(selection, physics);
    let mut core = core.context("load startup world carriers")?;
    core.actor = Ok(core
        .actor
        .context("load exact entity-linked neutral actor artwork")?);
    let assets = Ok(asset_startup::MenuAssets {
        fonts: core.assets.fonts.clone(),
        atmosphere: core.assets.atmosphere.clone(),
    });
    Ok(MenuBoot {
        assets,
        hud,
        icons,
        lang,
        audio,
        ui,
        world: world::WorldInput::Loaded(Box::new(world::WorldCarriers {
            core: Ok(core),
            block_entities,
            particles,
            weather,
            collision,
        })),
        path,
    })
}

/// Reads only the menu carrier set, retaining world inputs for post-presentation admission.
pub(super) fn load(selection: AssetSelection, physics: &Path) -> MenuBoot {
    let times = LoadTimes::default();
    let path = selection.path.clone();
    let (assets, hud, icons, lang, audio, ui) =
        render_scene::startup_timing::measure("carriers", || {
            std::thread::scope(|scope| {
                let hud =
                    scope.spawn(|| times.time("hud", || asset_startup::require_hud_assets(&path)));
                let icons = scope.spawn(|| {
                    times.time("icon", || {
                        asset_startup::require_icon_assets(
                            &path,
                            asset_startup::vanilla_source_manifest_json(),
                        )
                    })
                });
                let lang = scope.spawn(|| {
                    times.time("lang", || {
                        asset_startup::require_lang_assets(
                            &path,
                            asset_startup::vanilla_source_manifest_json(),
                        )
                    })
                });
                let audio = scope.spawn(|| load_audio(&path, &times));
                let ui = scope.spawn(|| {
                    times
                        .time("ui", || {
                            client_ui::ui_runtime::json_ui_assets::require_ui_assets(&path)
                        })
                        .and_then(PreparingJsonUi::start)
                });
                (
                    asset_startup::load_menu_assets(&path, &times),
                    join(hud),
                    join(icons),
                    join(lang),
                    join(audio),
                    join(ui),
                )
            })
        });
    MenuBoot {
        assets,
        hud,
        icons,
        lang,
        audio,
        ui,
        path,
        world: world::WorldInput::Deferred {
            selection,
            physics: physics.to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_and_android_preload_leave_world_carriers_and_collisions_unstarted() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(assets::carriers::WORLD.output);
        std::fs::write(
            asset_startup::atmosphere_asset_path(&path),
            asset_startup::test_carriers::synthetic_atmosphere_blob(),
        )
        .unwrap();
        std::fs::write(&path, b"unread invalid world").unwrap();
        std::fs::write(
            asset_startup::entity_asset_path(&path),
            b"unread invalid entities",
        )
        .unwrap();
        let physics = directory.path().join("unread-physics.bin");
        let selection = asset_startup::select_asset_path(Some(&path), None);
        let slot = Mutex::new(None);
        preload_into(&slot, selection, &physics, false, load);
        let boot = take_preloaded(&slot, &path, &physics).unwrap();
        assert!(boot.assets.is_ok());
        assert!(matches!(boot.world, world::WorldInput::Deferred { .. }));
    }
}
