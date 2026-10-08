use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
};

use super::*;
use crate::install_layout::{InstallEnvironment, Platform};
use test_support::Dir;

struct Fake {
    accept: bool,
    asked: Cell<u32>,
}

impl Prompter for Fake {
    fn confirm(&self, _: &str, _: &str) -> Consent {
        self.asked.set(self.asked.get() + 1);
        if self.accept {
            Consent::Accepted
        } else {
            Consent::Declined
        }
    }
    fn info(&self, _: &str, _: &str) {}
    fn alert(&self, _: &str, _: &str) {}
}

pub(super) fn installed_layout(data: &Dir, executable: &str) -> InstallLayout {
    let mut layout = InstallLayout::resolve(
        Platform::Linux,
        &InstallEnvironment {
            executable: PathBuf::from(executable),
            user_root: None,
            home: Some(PathBuf::from("/home/dev")),
            local_app_data: None,
            xdg_config_home: Some(data.path().join("cfg")),
            xdg_data_home: Some(data.path().join("data")),
            xdg_runtime_dir: None,
        },
    )
    .unwrap();
    // Linux layout fixtures also run on Windows, where native scratch paths are not XDG
    // absolute paths. Keep their filesystem writes out of the shared Linux home fallback.
    layout.user_config_root = data.path().join("cfg/cinnabar");
    layout.user_data_root = data.path().join("data/cinnabar");
    layout.with_prepared_assets()
}

#[test]
fn development_layout_needs_no_preparation() {
    let data = Dir::new("dev");
    let layout = installed_layout(&data, "/work/cinnabar/target/release/bedrock-client");
    let fake = Fake {
        accept: false,
        asked: Cell::new(0),
    };
    assert_eq!(
        ensure_with(&layout, &fake, false).unwrap(),
        Outcome::NotNeeded
    );
    assert_eq!(fake.asked.get(), 0);
}

#[test]
fn declined_consent_quits_without_running_anything() {
    let data = Dir::new("decline");
    let layout = installed_layout(&data, "/nonexistent/opt/cinnabar/bin/bedrock-client");
    let fake = Fake {
        accept: false,
        asked: Cell::new(0),
    };
    assert_eq!(ensure_with(&layout, &fake, false).unwrap(), Outcome::Quit);
    assert!(!consent_marker(&layout).exists());
}

#[test]
fn recorded_consent_to_the_same_terms_is_not_asked_again() {
    let data = Dir::new("consent");
    let layout = installed_layout(&data, "/nonexistent/opt/cinnabar/bin/bedrock-client");
    let fake = Fake {
        accept: false,
        asked: Cell::new(0),
    };
    record_consent(&layout).unwrap();
    let _ = ensure_with(&layout, &fake, false);
    assert_eq!(fake.asked.get(), 0);
    // Consent recorded for other terms (or by an older build) asks again.
    fs::write(consent_marker(&layout), b"accepted\n").unwrap();
    assert_eq!(ensure_with(&layout, &fake, false).unwrap(), Outcome::Quit);
    assert_eq!(fake.asked.get(), 1);
}

#[test]
fn missing_kit_is_reported_after_consent_and_recorded() {
    let data = Dir::new("nokit");
    let layout = installed_layout(&data, "/nonexistent/opt/cinnabar/bin/bedrock-client");
    let fake = Fake {
        accept: true,
        asked: Cell::new(0),
    };
    let error = ensure_with(&layout, &fake, false).unwrap_err();
    assert!(format!("{error:#}").contains("preparation kit"));
    let status = fs::read_to_string(layout.log_dir().join("first-run-status.json")).unwrap();
    assert!(status.contains("\"failed\"") && status.contains("preparation kit"));
    // Consent is remembered, so the retry does not prompt again.
    let _ = ensure_with(&layout, &fake, false);
    assert_eq!(fake.asked.get(), 1);
}

#[derive(Default)]
struct Recorder {
    no_prompt: bool,
    alerts: RefCell<Vec<String>>,
}

impl Prompter for Recorder {
    fn confirm(&self, _: &str, _: &str) -> Consent {
        if self.no_prompt {
            Consent::Unavailable
        } else {
            Consent::Accepted
        }
    }
    fn info(&self, _: &str, _: &str) {}
    fn alert(&self, _: &str, message: &str) {
        self.alerts.borrow_mut().push(message.to_owned());
    }
}

#[test]
fn no_consent_surface_fails_visibly_instead_of_quitting() {
    let data = Dir::new("no-prompt");
    let layout = installed_layout(&data, "/nonexistent/opt/cinnabar/bin/bedrock-client");
    let prompter = Recorder {
        no_prompt: true,
        ..Recorder::default()
    };
    let error = format!("{:#}", ensure_with(&layout, &prompter, false).unwrap_err());
    assert!(error.contains(CONSENT_ENV), "{error}");
    assert_eq!(prompter.alerts.borrow().as_slice(), [error.as_str()]);
    let status = fs::read_to_string(layout.log_dir().join("first-run-status.json")).unwrap();
    assert!(status.contains("\"failed\"") && status.contains(CONSENT_ENV));
    assert!(!consent_marker(&layout).exists());
}

/// The bundled compiler stands in as a script whose check reports a stale pack carrier.
#[cfg(unix)]
#[test]
fn a_failing_step_shows_its_underlying_error_in_the_dialog() {
    use std::{io::Write, os::unix::fs::PermissionsExt};
    use test_support::write_vanilla_manifest;

    let data = Dir::new("failing-step");
    let mut layout = installed_layout(&data, "/nonexistent/opt/cinnabar/bin/bedrock-client");
    layout.resource_root = data.path().join("resources");
    let kit = layout.prep_kit();
    let compiler = assets::carriers::kit_compiler(&kit);
    fs::create_dir_all(compiler.parent().unwrap()).unwrap();
    fs::write(
        &compiler,
        "#!/bin/sh\necho '{\"current\":false,\"stale\":[\"world\"],\"needs_pack\":true}'\n",
    )
    .unwrap();
    fs::set_permissions(&compiler, fs::Permissions::from_mode(0o755)).unwrap();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("../escape.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"x").unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let sha = format!("{:x}", Sha256::digest(&archive));
    write_vanilla_manifest(&kit, "https://example.invalid/pack.zip", &sha, "pack.zip");
    // A verified download is reused, so the run reaches the unpack step offline.
    let downloads = layout
        .prepare_workspace()
        .join(assets::vanilla_pack::DOWNLOAD_DIR);
    fs::create_dir_all(&downloads).unwrap();
    fs::write(downloads.join("pack.zip"), &archive).unwrap();

    let prompter = Recorder::default();
    ensure_with(&layout, &prompter, true).unwrap_err();
    let alerts = prompter.alerts.borrow();
    let [alert] = alerts.as_slice() else {
        panic!("expected one alert, got {alerts:?}");
    };
    assert!(
        alert.contains(
            "Unpacking the Minecraft sample resource pack: unsafe ZIP entry \
             '../escape.txt': traversal components are not allowed"
        ),
        "{alert}"
    );
    assert!(alert.contains("first-run.log"), "{alert}");
}

/// Startup fails closed without these, so setup must refuse to finish without them too.
#[test]
fn every_carrier_startup_requires_is_required_by_the_carrier_table() {
    use crate::asset_startup::{
        atmosphere_asset_path, entity_asset_path, hud_asset_path, icon_asset_path, lang_asset_path,
    };
    let world = Path::new("compiled").join(assets::carriers::WORLD.output);
    let startup = [
        atmosphere_asset_path(&world),
        entity_asset_path(&world),
        hud_asset_path(&world),
        icon_asset_path(&world),
        lang_asset_path(&world),
        client_ui::ui_runtime::json_ui_assets::ui_asset_path(&world),
    ];
    for path in startup {
        let name = path.file_name().unwrap().to_str().unwrap();
        let carrier = assets::carriers::CARRIERS
            .iter()
            .find(|carrier| carrier.output == name)
            .unwrap_or_else(|| panic!("{name} is missing from the carrier table"));
        assert!(carrier.required, "{name} must be a required carrier");
    }
}

#[test]
fn native_setup_title_uses_embedding_identity_with_default_fallback() {
    assert_eq!(
        super::setup_title(Some("Zeno Client")),
        "Zeno Client first-time setup"
    );
    for title in [None, Some(""), Some("  ")] {
        assert_eq!(
            super::setup_title(title),
            format!("{} first-time setup", launcher::PRODUCT_NAME)
        );
    }
}
