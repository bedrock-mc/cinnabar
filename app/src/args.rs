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
  --metrics-warmup-seconds <N> Exclude the first N timed-session seconds from frame metrics
  --metrics-sample-seconds <N> Freeze frame metrics after N post-warmup seconds
  --auto-fly                   Fly the camera automatically for acceptance
  --vsync                      Force FIFO presentation and disable driver workarounds
  --no-vsync                   Use immediate presentation when supported
  --frame-cap <FPS>            Cap acceptance updates to 1-1000 FPS
  --full-view-teleport-gate    Measure a dedicated no-overlap teleport
  --require-transparent-presentation
                               Wait up to 2s for GPU-presented water at timed exit
  --transparent-witness-request <PATH>
                               Poll an ignored-local exact transparent witness request
  --model-witness-request <PATH>
                               Poll an ignored-local exact packed-model witness request
  --phase3-evidence-target <TARGET>
                               Bind Phase 3 evidence to Bds, Lunar, Zeqa, or Lbsg
  --phase3-candidate-physics  Request fail-closed candidate Physics authority for Phase 3 evidence
  -h, --help                   Print this help
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase3Target {
    Bds,
    Lunar,
    Zeqa,
    Lbsg,
}

impl Phase3Target {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bds => "Bds",
            Self::Lunar => "Lunar",
            Self::Zeqa => "Zeqa",
            Self::Lbsg => "Lbsg",
        }
    }

    fn parse(value: String) -> Result<Self, ArgsError> {
        match value.as_str() {
            "Bds" => Ok(Self::Bds),
            "Lunar" => Ok(Self::Lunar),
            "Zeqa" => Ok(Self::Zeqa),
            "Lbsg" => Ok(Self::Lbsg),
            _ => Err(ArgsError::InvalidPhase3Target(value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientArgs {
    pub socket_dir: PathBuf,
    pub assets: Option<PathBuf>,
    pub display_name: String,
    pub acceptance_seconds: Option<u64>,
    pub metrics_out: Option<PathBuf>,
    pub metrics_warmup_seconds: u64,
    pub metrics_sample_seconds: Option<u64>,
    pub auto_fly: bool,
    pub force_vsync: bool,
    pub no_vsync: bool,
    pub frame_cap: Option<u32>,
    pub full_view_teleport_gate: bool,
    pub require_transparent_presentation: bool,
    pub transparent_witness_request: Option<PathBuf>,
    pub model_witness_request: Option<PathBuf>,
    pub phase3_evidence_target: Option<Phase3Target>,
    pub phase3_candidate_physics: bool,
}

impl Default for ClientArgs {
    fn default() -> Self {
        Self {
            socket_dir: PathBuf::from(".local/run"),
            assets: None,
            display_name: "RustMCBE".to_owned(),
            acceptance_seconds: None,
            metrics_out: None,
            metrics_warmup_seconds: 0,
            metrics_sample_seconds: None,
            auto_fly: false,
            force_vsync: false,
            no_vsync: false,
            frame_cap: None,
            full_view_teleport_gate: false,
            require_transparent_presentation: false,
            transparent_witness_request: None,
            model_witness_request: None,
            phase3_evidence_target: None,
            phase3_candidate_physics: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseOutcome {
    Run(Box<ClientArgs>),
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

    #[error("--metrics-warmup-seconds must be a nonnegative integer, got {0:?}")]
    InvalidMetricsWarmupSeconds(String),

    #[error("--metrics-sample-seconds must be a positive integer, got {0:?}")]
    InvalidMetricsSampleSeconds(String),

    #[error("--frame-cap must be an integer from 1 through 1000, got {0:?}")]
    InvalidFrameCap(String),

    #[error("--display-name cannot be empty")]
    EmptyDisplayName,

    #[error("--vsync and --no-vsync cannot be used together")]
    ConflictingVsyncFlags,

    #[error("--phase3-evidence-target must be one of Bds, Lunar, Zeqa, or Lbsg, got {0:?}")]
    InvalidPhase3Target(String),

    #[error("--phase3-candidate-physics requires an attributable --phase3-evidence-target run")]
    Phase3CandidateRequiresEvidence,

    #[error(
        "--phase3-evidence-target requires --acceptance-seconds and --metrics-out for attributable evidence"
    )]
    Phase3EvidenceRequiresAttributableRun,
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
                Some("--vsync") => parsed.force_vsync = true,
                Some("--no-vsync") => parsed.no_vsync = true,
                Some("--full-view-teleport-gate") => parsed.full_view_teleport_gate = true,
                Some("--require-transparent-presentation") => {
                    parsed.require_transparent_presentation = true;
                }
                Some("--phase3-candidate-physics") => parsed.phase3_candidate_physics = true,
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
                Some("--metrics-warmup-seconds") => {
                    let value = next_value(&mut arguments, "--metrics-warmup-seconds")?
                        .into_string()
                        .map_err(|_| ArgsError::InvalidUtf8 {
                            flag: "--metrics-warmup-seconds",
                        })?;
                    parsed.metrics_warmup_seconds = value
                        .parse::<u64>()
                        .map_err(|_| ArgsError::InvalidMetricsWarmupSeconds(value.clone()))?;
                }
                Some("--metrics-sample-seconds") => {
                    let value = next_value(&mut arguments, "--metrics-sample-seconds")?
                        .into_string()
                        .map_err(|_| ArgsError::InvalidUtf8 {
                            flag: "--metrics-sample-seconds",
                        })?;
                    parsed.metrics_sample_seconds = Some(
                        value
                            .parse::<u64>()
                            .ok()
                            .filter(|&seconds| seconds != 0)
                            .ok_or_else(|| ArgsError::InvalidMetricsSampleSeconds(value.clone()))?,
                    );
                }
                Some("--transparent-witness-request") => {
                    parsed.transparent_witness_request = Some(PathBuf::from(next_value(
                        &mut arguments,
                        "--transparent-witness-request",
                    )?));
                }
                Some("--model-witness-request") => {
                    parsed.model_witness_request = Some(PathBuf::from(next_value(
                        &mut arguments,
                        "--model-witness-request",
                    )?));
                }
                Some("--phase3-evidence-target") => {
                    let value = next_value(&mut arguments, "--phase3-evidence-target")?
                        .into_string()
                        .map_err(|_| ArgsError::InvalidUtf8 {
                            flag: "--phase3-evidence-target",
                        })?;
                    parsed.phase3_evidence_target = Some(Phase3Target::parse(value)?);
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
        if parsed.phase3_candidate_physics && parsed.phase3_evidence_target.is_none() {
            return Err(ArgsError::Phase3CandidateRequiresEvidence);
        }
        if parsed.force_vsync && parsed.no_vsync {
            return Err(ArgsError::ConflictingVsyncFlags);
        }
        if parsed.phase3_evidence_target.is_some()
            && (parsed.acceptance_seconds.is_none() || parsed.metrics_out.is_none())
        {
            return Err(ArgsError::Phase3EvidenceRequiresAttributableRun);
        }
        Ok(ParseOutcome::Run(Box::new(parsed)))
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
        assert_eq!(args.metrics_warmup_seconds, 0);
        assert_eq!(args.metrics_sample_seconds, None);
        assert!(!args.auto_fly);
        assert!(!args.force_vsync);
        assert!(!args.no_vsync);
        assert_eq!(args.frame_cap, None);
        assert!(!args.full_view_teleport_gate);
        assert!(!args.require_transparent_presentation);
        assert_eq!(args.transparent_witness_request, None);
        assert_eq!(args.model_witness_request, None);
        assert_eq!(args.phase3_evidence_target, None);
        assert!(!args.phase3_candidate_physics);
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
            "--metrics-warmup-seconds",
            "30",
            "--metrics-sample-seconds",
            "120",
            "--auto-fly",
            "--no-vsync",
            "--require-transparent-presentation",
            "--transparent-witness-request",
            "run/transparent-witness-request.json",
            "--model-witness-request",
            "run/model-witness-request.json",
            "--phase3-evidence-target",
            "Zeqa",
            "--phase3-candidate-physics",
        ])
        .unwrap() else {
            panic!("expected run args")
        };
        assert_eq!(args.socket_dir, PathBuf::from("run/socket"));
        assert_eq!(args.assets, Some(PathBuf::from("assets/vanilla.mcbea")));
        assert_eq!(args.display_name, "TestBot");
        assert_eq!(args.acceptance_seconds, Some(900));
        assert_eq!(args.metrics_out, Some(PathBuf::from("metrics.json")));
        assert_eq!(args.metrics_warmup_seconds, 30);
        assert_eq!(args.metrics_sample_seconds, Some(120));
        assert!(args.auto_fly);
        assert!(args.no_vsync);
        assert_eq!(args.frame_cap, None);
        assert!(!args.full_view_teleport_gate);
        assert!(args.require_transparent_presentation);
        assert_eq!(
            args.transparent_witness_request,
            Some(PathBuf::from("run/transparent-witness-request.json"))
        );
        assert_eq!(
            args.model_witness_request,
            Some(PathBuf::from("run/model-witness-request.json"))
        );
        assert_eq!(args.phase3_evidence_target, Some(super::Phase3Target::Zeqa));
        assert!(args.phase3_candidate_physics);
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
    fn parses_model_witness_request_path() {
        let ParseOutcome::Run(args) = ClientArgs::parse_from([
            "client",
            "--model-witness-request",
            "run/model-witness-request.json",
        ])
        .unwrap() else {
            panic!("expected run args")
        };
        assert_eq!(
            args.model_witness_request,
            Some(PathBuf::from("run/model-witness-request.json"))
        );
    }

    #[test]
    fn parses_explicit_vsync_override() {
        let ParseOutcome::Run(args) = ClientArgs::parse_from(["client", "--vsync"]).unwrap() else {
            panic!("expected run args")
        };
        assert!(args.force_vsync);
        assert!(!args.no_vsync);
    }

    #[test]
    fn help_documents_all_acceptance_flags() {
        assert_eq!(
            ClientArgs::parse_from(["client", "--help"]).unwrap(),
            ParseOutcome::Help
        );
        for flag in [
            "--socket-dir",
            "--assets",
            "--acceptance-seconds",
            "--metrics-out",
            "--metrics-warmup-seconds",
            "--metrics-sample-seconds",
            "--auto-fly",
            "--vsync",
            "--no-vsync",
            "--frame-cap",
            "--full-view-teleport-gate",
            "--require-transparent-presentation",
            "--transparent-witness-request",
            "--model-witness-request",
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
            ClientArgs::parse_from(["client", "--vsync", "--no-vsync"]),
            Err(ArgsError::ConflictingVsyncFlags)
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
        assert!(matches!(
            ClientArgs::parse_from([
                "client",
                "--acceptance-seconds",
                "30",
                "--phase3-candidate-physics"
            ]),
            Err(ArgsError::Phase3CandidateRequiresEvidence)
        ));
        assert!(matches!(
            ClientArgs::parse_from(["client", "--phase3-evidence-target", "Unknown"]),
            Err(ArgsError::InvalidPhase3Target(_))
        ));
    }
}
