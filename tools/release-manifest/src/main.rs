//! Generates update signing keys and signs release manifests (CI only).
#![expect(
    clippy::print_stderr,
    reason = "release-manifest reports usage and command failures on stderr"
)]

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    process::ExitCode,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use update_manifest::{Artifact, KEY_BYTES, Manifest, SCHEMA};

const KEY_ENV: &str = "CINNABAR_UPDATE_SIGNING_KEY";
const USAGE: &str = "usage: release-manifest keygen | sign [flags] | payload-names ENVELOPE";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let seed = std::env::var(KEY_ENV).ok();
    match run(&args, seed.as_deref(), &mut std::io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("release-manifest: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs one command; `seed` is the base64 signing seed from the environment.
fn run(args: &[String], seed: Option<&str>, out: &mut impl Write) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("keygen") => keygen(out),
        Some("sign") => sign(&args[1..], seed, out),
        Some("payload-names") => payload_names(&args[1..], out),
        Some(other) => Err(format!("unknown command {other:?}")),
        None => Err(USAGE.to_owned()),
    }
}

/// Reads an archived envelope only to retain downloads; it never authorizes installation.
fn payload_names(args: &[String], out: &mut impl Write) -> Result<(), String> {
    let [file] = args else {
        return Err(USAGE.to_owned());
    };
    let bytes = std::fs::read(file).map_err(|error| format!("read {file}: {error}"))?;
    if bytes.len() > update_manifest::MAX_ENVELOPE_BYTES {
        return Err("archived manifest exceeds size limit".to_owned());
    }
    let envelope: update_manifest::Envelope =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let payload = STANDARD
        .decode(envelope.manifest)
        .map_err(|error| error.to_string())?;
    let manifest: Manifest = serde_json::from_slice(&payload).map_err(|error| error.to_string())?;
    if manifest.schema != SCHEMA || manifest.artifacts.is_empty() {
        return Err("archived manifest has no supported payload list".to_owned());
    }
    let mut names = BTreeSet::new();
    for artifact in manifest.artifacts.values() {
        update_manifest::validate_artifact(artifact).map_err(|error| error.to_string())?;
        let name = artifact.url.rsplit('/').next().unwrap_or_default();
        if !name.starts_with(update_manifest::platforms::payload_prefix())
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err("archived payload has an unsafe asset name".to_owned());
        }
        names.insert(name);
    }
    for name in names {
        writeln!(out, "{name}").map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn keygen(out: &mut impl Write) -> Result<(), String> {
    let seed = update_manifest::generate_seed().map_err(|error| error.to_string())?;
    let public = update_manifest::public_key(&seed).map_err(|error| error.to_string())?;
    writeln!(
        out,
        "public (add to UPDATE_TRUSTED_KEYS as <id>:<this>): {}",
        STANDARD.encode(public)
    )
    .and_then(|()| {
        writeln!(
            out,
            "private (store as {KEY_ENV}, never commit): {}",
            STANDARD.encode(seed)
        )
    })
    .map_err(|error| error.to_string())
}

/// `sign` flags, accepted as `-name value`, `-name=value` or with `--`.
struct SignFlags {
    version: String,
    channel: String,
    build: u64,
    key_id: String,
    notes_url: String,
    validity: Duration,
    artifacts: Vec<String>,
}

impl SignFlags {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut flags = Self {
            version: String::new(),
            channel: update_manifest::channel::Channel::default()
                .name()
                .to_owned(),
            build: update_manifest::build_number(),
            key_id: "k1".to_owned(),
            notes_url: String::new(),
            validity: Duration::days(30),
            artifacts: Vec::new(),
        };
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let name = arg
                .strip_prefix("--")
                .or_else(|| arg.strip_prefix('-'))
                .ok_or_else(|| format!("unexpected argument {arg:?}"))?;
            let (name, value) = match name.split_once('=') {
                Some((name, value)) => (name, value.to_owned()),
                None => {
                    let value = args
                        .next()
                        .ok_or_else(|| format!("flag -{name} needs a value"))?;
                    (name, value.clone())
                }
            };
            match name {
                "version" => flags.version = value,
                "channel" => flags.channel = value,
                "build-number" => {
                    flags.build = value.parse().map_err(|_| "invalid build number")?
                }
                "key-id" => flags.key_id = value,
                "notes-url" => flags.notes_url = value,
                "validity" => flags.validity = parse_duration(&value)?,
                "artifact" => flags.artifacts.push(value),
                _ => return Err(format!("flag provided but not defined: -{name}")),
            }
        }
        Ok(flags)
    }
}

/// A lifetime such as `720h` or `1h30m`; units are `h`, `m` and `s`.
fn parse_duration(raw: &str) -> Result<Duration, String> {
    let invalid = || format!("invalid validity {raw:?}");
    let mut total = Duration::ZERO;
    let mut rest = raw;
    while !rest.is_empty() {
        let digits = rest
            .find(|c: char| !c.is_ascii_digit())
            .ok_or_else(invalid)?;
        let amount: i64 = rest[..digits].parse().map_err(|_| invalid())?;
        let mut unit = rest[digits..].chars();
        total += match unit.next() {
            Some('h') => Duration::hours(amount),
            Some('m') => Duration::minutes(amount),
            Some('s') => Duration::seconds(amount),
            _ => return Err(invalid()),
        };
        rest = unit.as_str();
    }
    if total <= Duration::ZERO {
        return Err(invalid());
    }
    Ok(total)
}

fn sign(args: &[String], seed: Option<&str>, out: &mut impl Write) -> Result<(), String> {
    let flags = SignFlags::parse(args)?;
    if flags.build == 0 {
        return Err("provide a positive -build-number".to_owned());
    }
    let seed = seed
        .and_then(|seed| STANDARD.decode(seed).ok())
        .filter(|seed| seed.len() == KEY_BYTES)
        .ok_or_else(|| format!("{KEY_ENV} must hold a base64 {KEY_BYTES}-byte seed"))?;
    let mut artifacts = BTreeMap::new();
    for spec in &flags.artifacts {
        let (platform, artifact) = describe(spec)?;
        artifacts.insert(platform, artifact);
    }
    if artifacts.is_empty() {
        return Err("at least one -artifact is required".to_owned());
    }
    let manifest = Manifest {
        schema: SCHEMA,
        channel: flags.channel,
        version: flags.version,
        build_number: flags.build,
        expires: OffsetDateTime::now_utc() + flags.validity,
        notes_url: flags.notes_url,
        artifacts,
    };
    let mut body = update_manifest::sign(&manifest, &flags.key_id, &seed)
        .map_err(|error| error.to_string())?;
    body.push(b'\n');
    out.write_all(&body).map_err(|error| error.to_string())
}

/// `platform=url=localfile` as the platform key and the file's published digest and size.
fn describe(spec: &str) -> Result<(String, Artifact), String> {
    let mut parts = spec.splitn(3, '=');
    let (Some(platform), Some(url), Some(file)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(format!("artifact {spec:?} must be platform=url=localfile"));
    };
    let data = std::fs::read(file).map_err(|error| format!("read {file}: {error}"))?;
    let size = i64::try_from(data.len()).map_err(|_| format!("{file} is too large"))?;
    let sha256 = Sha256::digest(&data)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((
        platform.to_owned(),
        Artifact {
            url: url.to_owned(),
            sha256,
            size,
        },
    ))
}

#[cfg(test)]
mod tests;
