//! Checks the development install layout used by commands that launch the client.

use std::ffi::OsString;

/// Cargo environment overrides that move the executable away from its development helpers.
const CARGO_OUTPUT_OVERRIDES: [&str; 3] = [
    "CARGO_TARGET_DIR",
    "CARGO_BUILD_TARGET_DIR",
    "CARGO_BUILD_TARGET",
];

/// Rejects environment overrides that move the client away from its development helpers.
pub fn refuse_output_overrides(
    command: &str,
    variable: impl Fn(&str) -> Option<OsString>,
) -> anyhow::Result<()> {
    for name in CARGO_OUTPUT_OVERRIDES {
        anyhow::ensure!(
            variable(name).is_none_or(|value| value.is_empty()),
            "{command} launches the client from target/<profile>; unset {name}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CARGO_OUTPUT_OVERRIDES, refuse_output_overrides};

    /// Each supported Cargo layout override fails with the command and variable named.
    #[test]
    fn output_overrides_require_the_development_layout() {
        for name in CARGO_OUTPUT_OVERRIDES {
            let error = refuse_output_overrides("mod run", |variable| {
                (variable == name).then(|| "/tmp/elsewhere".into())
            })
            .unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("mod run launches the client from target/<profile>; unset {name}")
            );
        }
        assert!(refuse_output_overrides("mod run", |_| Some("".into())).is_ok());
        assert!(refuse_output_overrides("mod run", |_| None).is_ok());
    }
}
