//! A standalone Cargo project whose shared source generates all package metadata.

use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};

/// Creates a new project without replacing any existing directory or publisher identity.
pub fn create(destination: &Path, name: &str, sdk: &Path) -> Result<()> {
    ensure!(
        experience_sdk::project::valid_name(name),
        "use a lowercase project name with letters, digits and underscores"
    );
    let sdk = sdk.canonicalize().context("locating experience-sdk")?;
    ensure!(
        sdk.join("Cargo.toml").is_file(),
        "SDK path has no Cargo.toml"
    );
    fs::create_dir(destination).with_context(|| format!("creating {}", destination.display()))?;
    fs::create_dir(destination.join("src"))?;
    fs::create_dir(destination.join("assets"))?;
    fs::write(
        destination.join("Cargo.toml"),
        format!(
            r#"[package]
name = {name:?}
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
experience-sdk = {{ path = {sdk:?}, default-features = false, features = ["messages"] }}

[features]
default = ["client"]
client = ["experience-sdk/client"]
server = ["experience-sdk/server"]

[build-dependencies]
experience-sdk = {{ path = {sdk:?}, default-features = false, features = ["project"] }}

[workspace]
"#,
            sdk = sdk.to_str().context("SDK path is not UTF-8")?
        ),
    )?;
    fs::write(
        destination.join("build.rs"),
        "/// Generates messages, asset constants and package metadata from shared Rust source.\nfn main() {\n    experience_sdk::project::generate().expect(\"generate mod definitions\");\n}\n",
    )?;
    fs::write(
        destination.join("src/lib.rs"),
        "#[cfg(all(feature = \"server\", feature = \"client\"))]\ncompile_error!(\"build exactly one mod half\");\n#[cfg(feature = \"client\")]\nmod client;\n#[cfg(feature = \"server\")]\nmod server;\npub mod shared {\n    experience_sdk::include_shared!();\n}\n",
    )?;
    fs::write(destination.join("src/client.rs"), CLIENT)?;
    fs::write(destination.join("src/server.rs"), SERVER)?;
    fs::create_dir(destination.join("assets/server"))?;
    fs::write(
        destination.join("src/shared.rs"),
        format!(
            r##"#![project(id = {name:?}, permissions(messaging))]

use experience_sdk::{{id, message::WorldId}};

/// A stable world definition shared with the server.
pub const HOME: WorldId = id!("{name}:home");

/// Requests travel; the server decides whether it is allowed.
#[message(id = "{name}.travel", schema = 1, to_server)]
#[derive(Clone, Debug, PartialEq)]
pub struct Travel {{
    #[field(max_bytes = 96)]
    pub world: WorldId,
    pub request: u32,
}}
"##
        ),
    )?;
    fs::write(
        destination.join("assets/welcome.txt"),
        "Client assets belong here.\n",
    )?;
    fs::write(
        destination.join(".gitignore"),
        "/target/\n/dist/\n/.local/\n",
    )?;
    Ok(())
}

const CLIENT: &str = r#"use experience_sdk::{client::ClientPart, entry::client, message::Registry};

pub struct Client;

#[client]
impl ClientPart for Client {
    /// Confirms the generated definitions before client behavior starts.
    fn registry() -> Option<&'static Registry> { Some(&crate::shared::REGISTRY) }

    /// Starts after messages.RegisterRegistry confirms the shared definitions.
    fn init() {
        let _welcome = crate::shared::assets::ASSET_WELCOME_TXT;
        // Move portable personal-mod setup here. Use client::send_message for typed requests.
    }
}
"#;

const SERVER: &str = r#"use experience_sdk::{entry::server, server::Experience};

struct Server;

#[server]
impl Experience for Server {}
"#;
