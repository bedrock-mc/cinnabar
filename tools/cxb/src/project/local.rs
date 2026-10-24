//! Local tool preparation and the mod development loop.

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

/// Finds the repository that owns the tooling, independent of the authored project's directory.
fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("tool repository exists")
}

/// Builds a Rust tool and reads Cargo's artifact path instead of assuming a target directory.
fn rust_tool(package: &str) -> Result<PathBuf> {
    let output = Command::new("cargo")
        .current_dir(repository())
        .args([
            "build",
            "--locked",
            "-p",
            package,
            "--message-format=json-render-diagnostics",
        ])
        .output()?;
    executable(output, package)
}

/// Finds one executable in Cargo output, preserving diagnostics and surrounding Make progress.
fn executable(output: std::process::Output, package: &str) -> Result<PathBuf> {
    std::io::stderr().write_all(&output.stderr)?;
    let mut artifact = None;
    for line in output
        .stdout
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
    {
        if !line.starts_with(b"{") {
            std::io::stderr().write_all(line)?;
            std::io::stderr().write_all(b"\n")?;
            continue;
        }
        let event: Value = serde_json::from_slice(line)?;
        if let Some(rendered) = event["message"]["rendered"].as_str() {
            std::io::stderr().write_all(rendered.as_bytes())?;
        }
        if event["reason"] == "compiler-artifact" && event["target"]["name"] == package {
            artifact = event["executable"].as_str().map(PathBuf::from);
        }
    }
    ensure!(output.status.success(), "building {package} failed");
    artifact.context("Cargo produced no tool executable")
}

/// Uses the normal client preparation pipeline for carriers, the core and execution helpers.
fn build_client() -> Result<PathBuf> {
    let output = Command::new("make")
        .current_dir(repository())
        .args([
            "play-build",
            "PROFILE=dev",
            "CLIENT_MESSAGE_FORMAT=json-render-diagnostics",
        ])
        .env(server_experience::policy::DEVELOPER_ENV, "1")
        .output()
        .context("preparing the development client")?;
    executable(output, "cinnabar")
}

/// Builds the existing local server once per command using Go's normal incremental cache.
fn local_server() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CINNABAR_LOCALSERVER_BIN") {
        return Ok(path.into());
    }
    let root = repository();
    let binary = root.join("target/mod-tools").join(format!(
        "bedrock-local-server{}",
        std::env::consts::EXE_SUFFIX
    ));
    fs::create_dir_all(binary.parent().context("tool directory missing")?)?;
    let status = Command::new("go")
        .args(["build", "-o"])
        .arg(&binary)
        .arg(".")
        .env("GOWORK", "off")
        .current_dir(root.join("tools/localserver"))
        .status()?;
    ensure!(status.success(), "building the local server failed");
    Ok(binary)
}

/// Validates registration in the real helper and uses Dragonfly's existing resource-pack builder.
pub(super) fn export_pack(artifact: &Path, out: &Path) -> Result<()> {
    let runtime = if let Some(path) = std::env::var_os("CINNABAR_EXPERIENCE_RUNTIME_BIN") {
        path.into()
    } else {
        rust_tool("experience-runtime")?
    };
    let status = Command::new(local_server()?)
        .arg("export-mod-pack")
        .arg(artifact)
        .arg(runtime)
        .arg(out)
        .status()?;
    ensure!(
        status.success(),
        "server registration or resource-pack generation failed"
    );
    Ok(())
}

/// Owns the server child so every return path stops and reaps only that process.
struct ServerChild(Child);
impl Drop for ServerChild {
    /// Stops the child by its exact process handle and waits for it to exit.
    fn drop(&mut self) {
        if let Some(mut stdin) = self.0.stdin.take() {
            let _ = writeln!(stdin, "stop");
        } else {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

/// Builds, installs, seeds the verified client cache and joins an isolated development world.
pub(super) fn run(root: &Path, release: bool) -> Result<()> {
    crate::client_build::refuse_output_overrides("mod run", |name| std::env::var_os(name))?;
    let root = root.canonicalize()?;
    let package = super::build(&root, release)?;
    let world = root.join(".local/world");
    super::package::replace(&package, &world)?;
    let client = build_client()?;
    let verifier = rust_tool("cinnabar-cxb")?;
    let runtime = rust_tool("experience-runtime")?;
    let user = root.join(".local/client");
    let server = local_server()?;
    seed_client_cache(&server, &world, &user.join("data"))?;
    let socket = std::net::UdpSocket::bind("127.0.0.1:0")?;
    let address = socket.local_addr()?.to_string();
    drop(socket);
    let mut server = ServerChild(
        Command::new(server)
            .arg("-addr")
            .arg(&address)
            .args(["-game-mode", "creative", "-dir"])
            .arg(&world)
            .arg("-experience-runtime")
            .arg(runtime)
            .env("CINNABAR_CXB_BIN", verifier)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?,
    );
    let mut lines = BufReader::new(server.0.stdout.take().context("server stdout missing")?);
    loop {
        let mut line = String::new();
        ensure!(
            lines.read_line(&mut line)? != 0,
            "server exited before readiness"
        );
        if line.trim() == "ready" {
            break;
        }
    }
    let status = Command::new(client)
        .arg("--address")
        .arg(address)
        .env("CINNABAR_USER_ROOT", user)
        .env(server_experience::policy::DEVELOPER_ENV, "1")
        .status()?;
    ensure!(status.success(), "development client failed");
    Ok(())
}

/// Seeds every installed client bundle using the local server's own mod discovery.
fn seed_client_cache(server: &Path, world: &Path, user_data: &Path) -> Result<()> {
    let output = Command::new(server)
        .arg("list-mod-clients")
        .arg(world)
        .output()
        .context("listing installed client bundles")?;
    std::io::stderr().write_all(&output.stderr)?;
    ensure!(
        output.status.success(),
        "listing installed client bundles failed"
    );
    let bundles: Vec<PathBuf> = serde_json::from_slice(&output.stdout)
        .context("reading the local server's installed client bundles")?;
    for bundle in bundles {
        crate::seed_cache(&bundle, user_data)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use server_experience::{cache, crypto};

    #[test]
    fn seeds_every_installed_mod_into_the_client_cache() {
        let temporary = tempfile::tempdir().unwrap();
        let world = temporary.path().join("world with spaces");
        let user_data = temporary.path().join("client/data");
        let publisher = crate::keys::pair(&[7; 32]).unwrap();
        let mut bundles = Vec::new();
        for id in ["first", "second"] {
            let bundle = crate::bundle::build(
                crate::bundle::Source {
                    id: id.into(),
                    package_version: "0.1.0".into(),
                    ..Default::default()
                },
                b"\0asm\x0d\0\x01\0",
                &[],
                &publisher,
            )
            .unwrap()
            .bytes;
            let installed = world.join("mods").join(id);
            fs::create_dir_all(installed.join("server")).unwrap();
            fs::write(
                installed
                    .join("server")
                    .join(experience_sdk::artifact::MANIFEST_FILE),
                format!("id = {id:?}\n"),
            )
            .unwrap();
            fs::create_dir_all(installed.join("client")).unwrap();
            fs::write(installed.join("client").join(format!("{id}.cxb")), &bundle).unwrap();
            bundles.push(bundle);
        }
        fs::create_dir_all(world.join("mods/.staging")).unwrap();
        fs::write(world.join("mods/README"), "not an installed mod").unwrap();

        seed_client_cache(&local_server().unwrap(), &world, &user_data).unwrap();

        let cache = cache::BundleCache::open(&cache::objects_dir(&user_data)).unwrap();
        for bytes in bundles {
            assert_eq!(cache.read(&crypto::digest(&bytes)).unwrap(), Some(bytes));
        }
    }
}
