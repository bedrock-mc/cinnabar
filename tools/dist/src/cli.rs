use std::path::PathBuf;

use crate::{DistError, Options, Platform};

pub fn parse_args<I, S>(arguments: I) -> Result<Options, DistError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = arguments.into_iter().map(Into::into);
    let mut values = [None, None, None, None, None, None, None, None, None];
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| DistError::Usage(format!("{flag} requires a value")))?;
        let index = match flag.as_str() {
            "--platform" => 0,
            "--client" => 1,
            "--core" => 2,
            "--assets" => 3,
            "--physics" => 4,
            "--notices" => 5,
            "--target" => 6,
            "--git-commit" => 7,
            "--out" => 8,
            _ => return Err(DistError::Usage(format!("unknown option {flag}"))),
        };
        if values[index].replace(value).is_some() {
            return Err(DistError::Usage(format!("duplicate option {flag}")));
        }
    }
    let [
        platform,
        client,
        core,
        assets,
        physics,
        notices,
        target,
        commit,
        output,
    ] = values;
    Ok(Options {
        platform: parse_platform(required(platform, "--platform")?)?,
        client: PathBuf::from(required(client, "--client")?),
        core: PathBuf::from(required(core, "--core")?),
        assets: PathBuf::from(required(assets, "--assets")?),
        physics: PathBuf::from(required(physics, "--physics")?),
        notices: PathBuf::from(required(notices, "--notices")?),
        target_triple: validate_target(required(target, "--target")?)?,
        git_commit: validate_commit(required(commit, "--git-commit")?)?,
        output: PathBuf::from(required(output, "--out")?),
    })
}

fn required(value: Option<String>, flag: &str) -> Result<String, DistError> {
    value.ok_or_else(|| DistError::Usage(format!("missing {flag}")))
}

fn parse_platform(value: String) -> Result<Platform, DistError> {
    match value.as_str() {
        "windows" => Ok(Platform::Windows),
        "linux" => Ok(Platform::Linux),
        "macos" => Ok(Platform::Macos),
        _ => Err(DistError::Usage(format!("unsupported platform {value}"))),
    }
}

fn validate_target(value: String) -> Result<String, DistError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(DistError::Usage("invalid --target triple".into()));
    }
    Ok(value)
}

fn validate_commit(value: String) -> Result<String, DistError> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(DistError::Usage(
            "--git-commit must be exactly 40 lowercase hexadecimal characters".into(),
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn arguments() -> Vec<&'static str> {
        vec![
            "--platform",
            "linux",
            "--client",
            "client",
            "--core",
            "core",
            "--assets",
            "assets",
            "--physics",
            "physics",
            "--notices",
            "notices",
            "--target",
            "x86_64-test",
            "--git-commit",
            "0123456789abcdef0123456789abcdef01234567",
            "--out",
            "out",
        ]
    }

    #[test]
    fn metadata_inputs_are_required_bounded_and_not_duplicated() {
        assert!(parse_args(arguments()).is_ok());
        let mut bad_target = arguments();
        bad_target[13] = "target with spaces";
        assert!(parse_args(bad_target).is_err());
        let mut bad_commit = arguments();
        bad_commit[15] = "ABC";
        assert!(parse_args(bad_commit).is_err());
        let mut duplicate = arguments();
        duplicate.extend(["--out", "second"]);
        assert!(parse_args(duplicate).is_err());
    }
}
