//! Required startup registry verification.
use super::PHYSICS_REGISTRY_GENERATION_GUIDANCE;
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

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
