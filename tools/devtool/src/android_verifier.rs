//! Packages the locked Android TLS verifier's JVM component and checked runtime dependencies.

use crate::DevtoolError;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

const INPUT: &str = "packaging/android/platform-verifier.json";
const MAX_ARTIFACT_BYTES: u64 = 8 * 1024 * 1024;
type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Deserialize)]
struct Inputs {
    verifier_version: String,
    shim_version: String,
    aar: Artifact,
    runtime_jars: Vec<Artifact>,
    checked_poms: Vec<Artifact>,
}

#[derive(Deserialize)]
struct Artifact {
    url: String,
    sha256: String,
}

/// Uses one caller-owned runtime for a finite package preparation command.
pub(crate) fn run(args: &[String]) -> std::result::Result<(), DevtoolError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| DevtoolError::Android(error.to_string()))?;
    runtime
        .block_on(prepare(args))
        .map_err(|error| DevtoolError::Android(error.to_string()))
}

/// Publishes only a completely verified bundle; a failed attempt leaves no usable partial output.
async fn prepare(args: &[String]) -> Result<()> {
    let [flag, output] = args else {
        return Err("prepare-android-verifier --out <new-directory>".into());
    };
    if flag != "--out" {
        return Err("prepare-android-verifier --out <new-directory>".into());
    }
    let output = Path::new(output);
    if output.exists() {
        return Err("output directory already exists".into());
    }
    let inputs: Inputs = serde_json::from_slice(&fs::read(INPUT)?)?;
    check_versions(&fs::read_to_string("Cargo.lock")?, &inputs)?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".android-verifier-")
        .tempdir_in(parent)?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(60))
        .build()?;
    let archive = download(&client, &inputs.aar, stage.path(), "verifier.aar").await?;
    for (index, pom) in inputs.checked_poms.iter().enumerate() {
        download(
            &client,
            pom,
            stage.path(),
            &format!("dependency-{index}.pom"),
        )
        .await?;
    }
    let mut archive = zip::ZipArchive::new(fs::File::open(archive)?)?;
    // Preserve the upstream manifest and generated CRL policy; the APK references that resource.
    extract(
        &mut archive,
        "AndroidManifest.xml",
        &stage.path().join("AndroidManifest.xml"),
    )?;
    let resource = stage.path().join("res/xml/network_security_config.xml");
    fs::create_dir_all(resource.parent().ok_or("resource has no parent")?)?;
    extract(
        &mut archive,
        "res/xml/network_security_config.xml",
        &resource,
    )?;
    let jars = stage.path().join("jars");
    fs::create_dir(&jars)?;
    extract(&mut archive, "classes.jar", &jars.join("verifier.jar"))?;
    drop(archive);
    for (index, jar) in inputs.runtime_jars.iter().enumerate() {
        download(&client, jar, &jars, &format!("runtime-{index}.jar")).await?;
    }
    fs::copy(INPUT, stage.path().join("artifact-inputs.json"))?;
    fs::rename(stage.path(), output)?;
    Ok(())
}

/// Both selected Rust packages must agree with the reviewed JVM component's lock record.
fn check_versions(lock: &str, inputs: &Inputs) -> Result<()> {
    let lock: toml::Value = toml::from_str(lock)?;
    let packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or("Cargo.lock lacks packages")?;
    for (name, expected) in [
        ("rustls-platform-verifier", &inputs.verifier_version),
        ("rustls-platform-verifier-android", &inputs.shim_version),
    ] {
        let versions: Vec<_> = packages
            .iter()
            .filter(|p| p.get("name").and_then(toml::Value::as_str) == Some(name))
            .filter_map(|p| p.get("version").and_then(toml::Value::as_str))
            .collect();
        if versions != [expected.as_str()] {
            return Err(format!(
                "{name} changed; review and update {INPUT} from its official JVM artifacts"
            )
            .into());
        }
    }
    Ok(())
}

/// Downloads a finite pinned input, validating complete bytes before extraction or compilation.
async fn download(
    client: &reqwest::Client,
    artifact: &Artifact,
    output: &Path,
    name: &str,
) -> Result<PathBuf> {
    let mut response = client.get(&artifact.url).send().await?.error_for_status()?;
    let mut data = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if chunk.len() as u64 > MAX_ARTIFACT_BYTES.saturating_sub(data.len() as u64) {
            return Err("artifact exceeds the bounded packaging input".into());
        }
        data.extend_from_slice(&chunk);
    }
    verify_bytes(&data, &artifact.sha256)?;
    let path = output.join(name);
    fs::write(&path, data)?;
    Ok(path)
}

/// Enforces the manifest's exact reviewed digest before extraction or DEX compilation.
fn verify_bytes(data: &[u8], expected: &str) -> Result<()> {
    if data.len() as u64 > MAX_ARTIFACT_BYTES || format!("{:x}", Sha256::digest(data)) != expected {
        return Err("artifact size or SHA-256 differs from the checked packaging input".into());
    }
    Ok(())
}

/// Reads only one named upstream ZIP member, with no paths selected by the archive itself.
fn extract(archive: &mut zip::ZipArchive<fs::File>, name: &str, output: &Path) -> Result<()> {
    let entry = archive.by_name(name)?;
    if entry.size() > MAX_ARTIFACT_BYTES {
        return Err(format!("oversized verifier entry: {name}").into());
    }
    let mut bounded = entry.take(MAX_ARTIFACT_BYTES + 1);
    let mut data = Vec::new();
    bounded.read_to_end(&mut data)?;
    if data.len() as u64 > MAX_ARTIFACT_BYTES {
        return Err(format!("oversized verifier entry: {name}").into());
    }
    fs::write(output, data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_jvm_component_refuses_cargo_version_drift_and_duplicate_authorities() {
        let inputs: Inputs = serde_json::from_str(include_str!(
            "../../../packaging/android/platform-verifier.json"
        ))
        .unwrap();
        check_versions(include_str!("../../../Cargo.lock"), &inputs).unwrap();
        let lock = format!(
            "[[package]]\nname = 'rustls-platform-verifier'\nversion = '{}'\n[[package]]\nname = 'rustls-platform-verifier-android'\nversion = '{}'\n",
            inputs.verifier_version, inputs.shim_version
        );
        check_versions(&lock, &inputs).unwrap();
        assert!(check_versions(&lock.replace(&inputs.shim_version, "99.0.0"), &inputs).is_err());
        assert!(check_versions(&format!("{lock}[[package]]\nname = 'rustls-platform-verifier-android'\nversion = '99.0.0'\n"), &inputs).is_err());
    }

    #[test]
    fn changed_download_bytes_are_refused_before_dex_or_resource_use() {
        let body = b"original upstream artifact";
        let expected = format!("{:x}", Sha256::digest(body));
        verify_bytes(body, &expected).unwrap();
        assert!(verify_bytes(b"different artifact", &expected).is_err());
    }
}
