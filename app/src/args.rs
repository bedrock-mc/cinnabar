use std::{ffi::OsString, path::PathBuf};

use thiserror::Error;

pub const HELP: &str = "\
bedrock-client — Rust Minecraft Bedrock phase-zero renderer

Usage: bedrock-client [OPTIONS]

Options:
  --socket-dir <PATH>          Core socket directory (default: .local/run)
  --assets <PATH>              Compiled vanilla asset blob
  --display-name <NAME>        Offline display name (default: RustMCBE)
  --acceptance-seconds <N>     Exit after N seconds and write metrics
  --metrics-out <PATH>         Deterministic JSON metrics output path
  --auto-fly                   Fly the camera automatically for acceptance
  --no-vsync                   Use immediate presentation when supported
  --frame-cap <FPS>            Cap acceptance updates to 1-1000 FPS
  --full-view-teleport-gate    Measure a dedicated no-overlap teleport
  -h, --help                   Print this help
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientArgs {
    pub socket_dir: PathBuf,
    pub assets: Option<PathBuf>,
    pub display_name: String,
    pub acceptance_seconds: Option<u64>,
    pub metrics_out: Option<PathBuf>,
    pub auto_fly: bool,
    pub no_vsync: bool,
    pub frame_cap: Option<u32>,
    pub full_view_teleport_gate: bool,
}

impl Default for ClientArgs {
    fn default() -> Self {
        Self {
            socket_dir: PathBuf::from(".local/run"),
            assets: None,
            display_name: "RustMCBE".to_owned(),
            acceptance_seconds: None,
            metrics_out: None,
            auto_fly: false,
            no_vsync: false,
            frame_cap: None,
            full_view_teleport_gate: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseOutcome {
    Run(ClientArgs),
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ArgsError {
    #[error("unknown argument {0:?}\n\n{HELP}")]
    Unknown(OsString),

    #[error("{flag} requires a value")]
    MissingValue { flag: &'static str },

    #[error("{flag} value must be valid UTF-8")]
    InvalidUtf8 { flag: &'static str },

    #[error("--acceptance-seconds must be a positive integer, got {0:?}")]
    InvalidAcceptanceSeconds(String),

    #[error("--frame-cap must be an integer from 1 through 1000, got {0:?}")]
    InvalidFrameCap(String),

    #[error("--display-name cannot be empty")]
    EmptyDisplayName,
}

impl ClientArgs {
    pub fn parse_env() -> Result<ParseOutcome, ArgsError> {
        Self::parse_from(std::env::args_os())
    }

    pub fn parse_from<I, S>(arguments: I) -> Result<ParseOutcome, ArgsError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut arguments = arguments.into_iter().map(Into::into);
        let _program = arguments.next();
        let mut parsed = Self::default();

        while let Some(argument) = arguments.next() {
            match argument.to_str() {
                Some("-h" | "--help") => return Ok(ParseOutcome::Help),
                Some("--auto-fly") => parsed.auto_fly = true,
                Some("--no-vsync") => parsed.no_vsync = true,
                Some("--full-view-teleport-gate") => parsed.full_view_teleport_gate = true,
                Some("--socket-dir") => {
                    parsed.socket_dir = PathBuf::from(next_value(&mut arguments, "--socket-dir")?);
                }
                Some("--assets") => {
                    parsed.assets = Some(PathBuf::from(next_value(&mut arguments, "--assets")?));
                }
                Some("--metrics-out") => {
                    parsed.metrics_out =
                        Some(PathBuf::from(next_value(&mut arguments, "--metrics-out")?));
                }
                Some("--display-name") => {
                    let value = next_value(&mut arguments, "--display-name")?
                        .into_string()
                        .map_err(|_| ArgsError::InvalidUtf8 {
                            flag: "--display-name",
                        })?;
                    if value.is_empty() {
                        return Err(ArgsError::EmptyDisplayName);
                    }
                    parsed.display_name = value;
                }
                Some("--acceptance-seconds") => {
                    let value = next_value(&mut arguments, "--acceptance-seconds")?
                        .into_string()
                        .map_err(|_| ArgsError::InvalidUtf8 {
                            flag: "--acceptance-seconds",
                        })?;
                    parsed.acceptance_seconds = Some(
                        value
                            .parse::<u64>()
                            .ok()
                            .filter(|&seconds| seconds != 0)
                            .ok_or_else(|| ArgsError::InvalidAcceptanceSeconds(value.clone()))?,
                    );
                }
                Some("--frame-cap") => {
                    let value = next_value(&mut arguments, "--frame-cap")?
                        .into_string()
                        .map_err(|_| ArgsError::InvalidUtf8 {
                            flag: "--frame-cap",
                        })?;
                    parsed.frame_cap = Some(
                        value
                            .parse::<u32>()
                            .ok()
                            .filter(|fps| (1..=1_000).contains(fps))
                            .ok_or_else(|| ArgsError::InvalidFrameCap(value.clone()))?,
                    );
                }
                _ => return Err(ArgsError::Unknown(argument)),
            }
        }
        Ok(ParseOutcome::Run(parsed))
    }
}

fn next_value<I>(arguments: &mut I, flag: &'static str) -> Result<OsString, ArgsError>
where
    I: Iterator<Item = OsString>,
{
    arguments.next().ok_or(ArgsError::MissingValue { flag })
}

#[cfg(test)]
mod tests {
    use super::{ArgsError, ClientArgs, HELP, ParseOutcome};
    use std::path::PathBuf;

    #[test]
    fn defaults_are_stable() {
        let ParseOutcome::Run(args) = ClientArgs::parse_from(["client"]).unwrap() else {
            panic!("expected run args")
        };
        assert_eq!(args.socket_dir, PathBuf::from(".local/run"));
        assert_eq!(args.assets, None);
        assert_eq!(args.display_name, "RustMCBE");
        assert_eq!(args.acceptance_seconds, None);
        assert!(!args.auto_fly);
        assert!(!args.no_vsync);
        assert_eq!(args.frame_cap, None);
        assert!(!args.full_view_teleport_gate);
    }

    #[test]
    fn parses_every_acceptance_flag() {
        let ParseOutcome::Run(args) = ClientArgs::parse_from([
            "client",
            "--socket-dir",
            "run/socket",
            "--assets",
            "assets/vanilla.mcbea",
            "--display-name",
            "TestBot",
            "--acceptance-seconds",
            "900",
            "--metrics-out",
            "metrics.json",
            "--auto-fly",
            "--no-vsync",
        ])
        .unwrap() else {
            panic!("expected run args")
        };
        assert_eq!(args.socket_dir, PathBuf::from("run/socket"));
        assert_eq!(args.assets, Some(PathBuf::from("assets/vanilla.mcbea")));
        assert_eq!(args.display_name, "TestBot");
        assert_eq!(args.acceptance_seconds, Some(900));
        assert_eq!(args.metrics_out, Some(PathBuf::from("metrics.json")));
        assert!(args.auto_fly);
        assert!(args.no_vsync);
        assert_eq!(args.frame_cap, None);
        assert!(!args.full_view_teleport_gate);
    }

    #[test]
    fn parses_full_view_teleport_gate_and_frame_cap() {
        let ParseOutcome::Run(args) =
            ClientArgs::parse_from(["client", "--full-view-teleport-gate", "--frame-cap", "60"])
                .unwrap()
        else {
            panic!("expected run args")
        };

        assert!(args.full_view_teleport_gate);
        assert_eq!(args.frame_cap, Some(60));
    }

    #[test]
    fn help_documents_all_four_required_app_flags() {
        assert_eq!(
            ClientArgs::parse_from(["client", "--help"]).unwrap(),
            ParseOutcome::Help
        );
        for flag in [
            "--socket-dir",
            "--assets",
            "--acceptance-seconds",
            "--metrics-out",
            "--auto-fly",
            "--frame-cap",
            "--full-view-teleport-gate",
        ] {
            assert!(HELP.contains(flag));
        }
    }

    #[test]
    fn malformed_arguments_are_rejected() {
        assert!(matches!(
            ClientArgs::parse_from(["client", "--socket-dir"]),
            Err(ArgsError::MissingValue {
                flag: "--socket-dir"
            })
        ));
        assert!(matches!(
            ClientArgs::parse_from(["client", "--acceptance-seconds", "0"]),
            Err(ArgsError::InvalidAcceptanceSeconds(_))
        ));
        assert!(matches!(
            ClientArgs::parse_from(["client", "--frame-cap", "0"]),
            Err(ArgsError::InvalidFrameCap(_))
        ));
        assert!(matches!(
            ClientArgs::parse_from(["client", "--unknown"]),
            Err(ArgsError::Unknown(_))
        ));
    }
}
