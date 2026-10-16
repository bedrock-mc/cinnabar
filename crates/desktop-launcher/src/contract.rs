//! Keeps the web UI's side of the bridge derived from Rust: `types.ts` is generated from the
//! command and event types, and the bridge must invoke exactly the registered commands.
//! Run with `CINNABAR_WRITE_UI_TYPES=1` to regenerate `types.ts`.

use crate::{
    changelog::Changelog,
    featured::{FeaturedServer, RecentServer},
    game::{GameState, LastExit, LaunchOptions, ServerTarget},
    install::{InstallState, Platform},
    news::{News, NewsItem},
};
use std::path::PathBuf;
use ts_rs::{Config, TS};
use update_manifest::channel::Channel;
use update_service::{UpdateStage, UpdateState};

const WRITE: &str = "CINNABAR_WRITE_UI_TYPES";

fn ui_file(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("ui/src/api")
        .join(name)
}

fn generated() -> String {
    // Every size and count fits a JavaScript number.
    let config = Config::new().with_large_int("number");
    let declarations = [
        Channel::decl(&config),
        Platform::decl(&config),
        InstallState::decl(&config),
        UpdateStage::decl(&config),
        UpdateState::decl(&config),
        LastExit::decl(&config),
        GameState::decl(&config),
        NewsItem::decl(&config),
        News::decl(&config),
        FeaturedServer::decl(&config),
        RecentServer::decl(&config),
        Changelog::decl(&config),
        ServerTarget::decl(&config),
        LaunchOptions::decl(&config),
    ];
    let mut text = String::from(
        "// Generated from the launcher's Rust types by `cargo test -p desktop-launcher contract`\n\
         // with CINNABAR_WRITE_UI_TYPES=1; do not edit by hand.\n\n",
    );
    for declaration in declarations {
        text.push_str("export ");
        text.push_str(&declaration);
        text.push_str("\n\n");
    }
    // Product identity comes from the Rust owners and the repository's license file.
    let license = include_str!("../../../LICENSE")
        .lines()
        .next()
        .and_then(|heading| heading.strip_prefix("# "))
        .expect("the license file starts with its name");
    for (name, value) in [
        ("PRODUCT_NAME", launcher::PRODUCT_NAME.to_owned()),
        (
            "SOURCE_URL",
            format!(
                "https://github.com/{}",
                update_manifest::platforms::repository()
            ),
        ),
        ("DISCORD_URL", launcher::DISCORD_INVITE_URL.to_owned()),
        ("LICENSE_NAME", license.to_owned()),
    ] {
        text.push_str(&format!(
            "export const {name} = {};\n",
            serde_json::to_string(&value).unwrap()
        ));
    }
    text.push_str("\nexport type { Api } from './api';\n");
    text
}

#[test]
fn ui_types_match_the_rust_contract() {
    let path = ui_file("types.ts");
    let expected = generated();
    if std::env::var_os(WRITE).is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == expected,
        "{} is stale; rerun this test with {WRITE}=1",
        path.display()
    );
}

/// The quoted first argument of every `call(` in `source`.
#[cfg(webview)]
fn quoted_calls(source: &str, call: &str) -> std::collections::BTreeSet<String> {
    source
        .match_indices(call)
        .filter_map(|(index, _)| {
            let rest = &source[index + call.len()..];
            let rest = rest.strip_prefix('<').map_or(rest, |generic| {
                generic.split_once(">(").map_or(generic, |(_, rest)| rest)
            });
            let rest = rest.strip_prefix('(').unwrap_or(rest);
            let rest = rest.strip_prefix('\'')?;
            Some(rest.split_once('\'')?.0.to_owned())
        })
        .collect()
}

#[cfg(webview)]
#[test]
fn the_bridge_invokes_every_registered_command_and_event() {
    use crate::{commands::COMMANDS, surface::EVENTS};
    let bridge = std::fs::read_to_string(ui_file("tauri.ts")).unwrap();
    let expected = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(quoted_calls(&bridge, "invoke"), expected(COMMANDS));
    assert_eq!(quoted_calls(&bridge, "subscribe"), expected(EVENTS));
}

#[test]
fn the_window_identity_matches_the_installed_app() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    assert_eq!(config["productName"], launcher::PRODUCT_NAME);
    // The macOS bundle keeps the identity existing installs upgrade in place.
    let packager = include_str!("../../../packaging/macos/build-app.sh");
    let bundle_id = packager
        .split_once("BUNDLE_ID:-")
        .and_then(|(_, rest)| rest.split_once('}'))
        .map(|(id, _)| id)
        .unwrap();
    assert_eq!(config["identifier"], bundle_id);
}
