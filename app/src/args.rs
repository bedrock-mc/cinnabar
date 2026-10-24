use std::{ffi::OsString, path::PathBuf};

use thiserror::Error;
use ui::RenderMode;

/// The settings screen's GUI-scale step when `--gui-scale` is auto.
pub const DEFAULT_GUI_SCALE: u8 = 2;

pub const HELP: &str = concat!(
    "\
cinnabar — ",
    launcher::product_name!(),
    ", a Minecraft: Bedrock Edition client

Usage: cinnabar [OPTIONS] [PACK.mcpack|PACK.mcaddon|PACK.zip]...

Desktop builds with native-networking use Rust addressed discovery by default.
Builds without it cannot open native server connections.

Options:
  --address <HOST:PORT>       Directly join a server
  --native-networking <KIND>  Select Rust transport: addressed, raknet or nethernet-lan
  --parallel-resource-pack-urls
                               Opt native pack URL downloads into bounded parallel reads
  --import-pack <PATH>        Import an optional global resource pack
  --assets <PATH>              Compiled vanilla asset blob
  --display-name <NAME>        Offline display name (default: ",
    launcher::product_name!(),
    ")
  --acceptance-seconds <N>     Exit after N seconds and write metrics
  --metrics-out <PATH>         Deterministic JSON metrics output path
  --metrics-warmup-seconds <N> Exclude the first N timed-session seconds from frame metrics
  --metrics-sample-seconds <N> Freeze frame metrics after N post-warmup seconds
  --auto-fly                   Fly the camera automatically for acceptance
  --freecam                    Use non-authoritative free-camera movement
  --vsync                      Force FIFO presentation and disable driver workarounds
  --no-vsync                   Allow tearing and disable display-rate synchronization
  --frame-cap <FPS>            Cap acceptance updates to 1-1000 FPS
  --render-mode <MODE>         vanilla, enhanced or shaderpack; CINNABAR_RENDER_MODE is the fallback
  --shader-pack <PATH>         Draw this Java shader pack (zip or folder) for the session only
  --hide-toasts                Developer builds: never draw toasts (join requests, achievements) in captures
  --fixed-time <TICKS>         Developer builds: draw at this time of day (0 = sunrise), ignoring the server clock
  --frame-time-counter <SECONDS>  Developer builds: freeze shader packs' frameTimeCounter (0 to <3600)
  --gui-scale <1-4|auto>       Fix the GUI scale (default: auto, the Bedrock desktop rule)
  --dev-debug-overlay          Compatibility alias; F3 debug controls are always available
  --language <ll_CC>           UI language (default: from LC_ALL/LC_MESSAGES/LANG, else en_US)
  --full-view-teleport-gate    Measure a dedicated no-overlap teleport
  --require-transparent-presentation
                               Wait up to 2s for GPU-presented water at timed exit
  --transparent-witness-request <PATH>
                               Poll an ignored-local exact transparent witness request
  --model-witness-request <PATH>
                               Poll an ignored-local exact packed-model witness request
  --movement-evidence-target <TARGET>
                               Bind movement evidence to Bds, Lunar, Zeqa, Lbsg, or Zeno
  --movement-candidate-physics
                               Request fail-closed candidate Physics authority for movement evidence
  -h, --help                   Print this help
"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceTarget {
    Bds,
    Lunar,
    Zeqa,
    Lbsg,
    Zeno,
    Venity,
}

impl EvidenceTarget {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bds => "Bds",
            Self::Lunar => "Lunar",
            Self::Zeqa => "Zeqa",
            Self::Lbsg => "Lbsg",
            Self::Zeno => "Zeno",
            Self::Venity => "Venity",
        }
    }

    fn parse(value: String) -> Result<Self, ArgsError> {
        match value.as_str() {
            "Bds" => Ok(Self::Bds),
            "Lunar" => Ok(Self::Lunar),
            "Zeqa" => Ok(Self::Zeqa),
            "Lbsg" => Ok(Self::Lbsg),
            "Zeno" => Ok(Self::Zeno),
            "Venity" => Ok(Self::Venity),
            _ => Err(ArgsError::InvalidEvidenceTarget(value)),
        }
    }
}

#[cfg(feature = "acceptance")]
impl acceptance::movement_evidence::EvidenceTargetLabel for EvidenceTarget {
    /// Publishes the argument's stable label to the optional evidence consumer.
    fn evidence_label(self) -> &'static str {
        self.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientArgs {
    /// Optional upstream address for the fast direct-connect path. Without it the client opens the menu.
    pub address: Option<String>,
    /// Selected native transport; absent when this build cannot open native connections.
    pub native_transport: Option<protocol::NativeTransport>,
    /// Native-only URL scheduling choice retained across joins and server transfers.
    pub parallel_resource_pack_urls: bool,
    pub assets: Option<PathBuf>,
    /// Files opened by the user, imported after the window starts.
    pub import_packs: Vec<PathBuf>,
    pub display_name: String,
    pub acceptance_seconds: Option<u64>,
    pub metrics_out: Option<PathBuf>,
    pub metrics_warmup_seconds: u64,
    pub metrics_sample_seconds: Option<u64>,
    pub auto_fly: bool,
    /// Use non-authoritative camera movement instead of local physics.
    pub freecam: bool,
    pub force_vsync: bool,
    pub no_vsync: bool,
    pub frame_cap: Option<u32>,
    /// Fixed Java GUI scale (1..=4) for the pinned capture matrix. `None`
    /// selects the Java auto rule; the normal client default is scale 2.
    pub gui_scale: Option<u8>,
    /// Session-only override of the saved rendering mode.
    pub render_mode: Option<RenderMode>,
    /// Session-only shader pack, opened from this path instead of the saved selection;
    /// implies `render_mode == Some(RenderMode::ShaderPack)`.
    pub shader_pack: Option<PathBuf>,
    /// Toasts are never drawn (developer-control builds only), so an account's join requests
    /// or achievements cannot land in a capture.
    pub hide_toasts: bool,
    /// Rendered time of day pinned for the whole session (developer-control builds only).
    pub fixed_time: Option<u32>,
    /// Shader packs' `frameTimeCounter` frozen at this many milliseconds (developer-control
    /// builds only), for captures that match a reference's animation phase.
    pub frame_time_counter_ms: Option<u32>,
    /// Enables F3 debug controls; the overlay starts hidden until F3 is pressed.
    /// `--dev-debug-overlay` remains accepted for older launch commands.
    pub dev_debug_overlay: bool,
    /// Requested UI language code; `None` follows the environment locale.
    pub language: Option<String>,
    pub full_view_teleport_gate: bool,
    pub require_transparent_presentation: bool,
    pub transparent_witness_request: Option<PathBuf>,
    pub model_witness_request: Option<PathBuf>,
    pub movement_evidence_target: Option<EvidenceTarget>,
    pub movement_candidate_physics: bool,
}

impl Default for ClientArgs {
    fn default() -> Self {
        Self {
            address: None,
            native_transport: cfg!(all(
                feature = "native-networking",
                not(target_arch = "wasm32")
            ))
            .then_some(protocol::NativeTransport::Addressed),
            parallel_resource_pack_urls: false,
            assets: None,
            import_packs: Vec::new(),
            display_name: launcher::PRODUCT_NAME.to_owned(),
            acceptance_seconds: None,
            metrics_out: None,
            metrics_warmup_seconds: 0,
            metrics_sample_seconds: None,
            auto_fly: false,
            freecam: false,
            force_vsync: false,
            no_vsync: false,
            frame_cap: None,
            gui_scale: None,
            render_mode: None,
            shader_pack: None,
            hide_toasts: false,
            fixed_time: None,
            frame_time_counter_ms: None,
            dev_debug_overlay: true,
            language: None,
            full_view_teleport_gate: false,
            require_transparent_presentation: false,
            transparent_witness_request: None,
            model_witness_request: None,
            movement_evidence_target: None,
            movement_candidate_physics: false,
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
    #[error(
        "native networking requires --features native-networking; direct sessions also require --address"
    )]
    NativeNetworkingUnavailable,
    #[error("native friend-world startup requires a verified friend selection")]
    NativeFriendSelectionRequired,
    #[error("--native-networking must be addressed, raknet or nethernet-lan")]
    InvalidNativeTransport,
    #[error("--parallel-resource-pack-urls requires --native-networking")]
    ParallelPackUrlsRequireNativeNetworking,
    #[error(
        "acceptance and evidence options require the app's `acceptance` feature; rebuild without --no-default-features or enable --features acceptance"
    )]
    AcceptanceFeatureRequired,

    #[error("{0} needs a developer-control build; rebuild with --features developer-control")]
    DeveloperControlFeatureRequired(&'static str),

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

    #[error("--gui-scale must be an integer from 1 through 4, got {0:?}")]
    InvalidGuiScale(String),

    #[error("--render-mode must be vanilla, enhanced or shaderpack, got {0:?}")]
    InvalidRenderMode(String),

    #[error("--shader-pack requires a non-empty path")]
    EmptyShaderPack,

    #[error("--fixed-time must be a nonnegative integer tick count, got {0:?}")]
    InvalidFixedTime(String),

    #[error("--frame-time-counter must be seconds from 0 up to 3600, got {0:?}")]
    InvalidFrameTimeCounter(String),

    #[error("--shader-pack selects the shaderpack render mode, but --render-mode is {0:?}")]
    ConflictingShaderPackRenderMode(&'static str),

    #[error("--language must be a code like de_DE, got {0:?}")]
    InvalidLanguage(String),

    #[error("--display-name cannot be empty")]
    EmptyDisplayName,

    #[error("--vsync and --no-vsync cannot be used together")]
    ConflictingVsyncFlags,

    #[error(
        "--movement-evidence-target must be one of Bds, Lunar, Zeqa, Lbsg, Zeno, or Venity, got {0:?}"
    )]
    InvalidEvidenceTarget(String),

    #[error("--movement-candidate-physics requires an attributable --movement-evidence-target run")]
    MovementCandidateRequiresEvidence,

    #[error(
        "--movement-evidence-target requires --acceptance-seconds and --metrics-out for attributable evidence"
    )]
    MovementEvidenceRequiresAttributableRun,
}

impl ClientArgs {
    /// Rejects evidence options before first-run setup when this build omits the optional plugin.
    pub(crate) fn validate_acceptance_support(&self, available: bool) -> Result<(), ArgsError> {
        let requested = self.acceptance_seconds.is_some()
            || self.metrics_out.is_some()
            || self.full_view_teleport_gate
            || self.require_transparent_presentation
            || self.transparent_witness_request.is_some()
            || self.model_witness_request.is_some()
            || self.movement_evidence_target.is_some()
            || self.movement_candidate_physics;
        if !available && requested {
            return Err(ArgsError::AcceptanceFeatureRequired);
        }
        Ok(())
    }

    /// Rejects developer-only options when this build omits the developer-control feature.
    pub(crate) fn validate_developer_support(&self, available: bool) -> Result<(), ArgsError> {
        if !available && self.fixed_time.is_some() {
            return Err(ArgsError::DeveloperControlFeatureRequired("--fixed-time"));
        }
        if !available && self.frame_time_counter_ms.is_some() {
            return Err(ArgsError::DeveloperControlFeatureRequired(
                "--frame-time-counter",
            ));
        }
        if !available && self.hide_toasts {
            return Err(ArgsError::DeveloperControlFeatureRequired("--hide-toasts"));
        }
        Ok(())
    }

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
                Some("--freecam") => parsed.freecam = true,
                Some("--dev-debug-overlay") => parsed.dev_debug_overlay = true,
                Some("--vsync") => parsed.force_vsync = true,
                Some("--no-vsync") => parsed.no_vsync = true,
                Some("--full-view-teleport-gate") => parsed.full_view_teleport_gate = true,
                Some("--require-transparent-presentation") => {
                    parsed.require_transparent_presentation = true;
                }
                Some("--movement-candidate-physics") => parsed.movement_candidate_physics = true,
                Some("--parallel-resource-pack-urls") => parsed.parallel_resource_pack_urls = true,
                Some("--address" | "-address") => {
                    let value = next_value(&mut arguments, "--address")?
                        .into_string()
                        .map_err(|_| ArgsError::InvalidUtf8 { flag: "--address" })?;
                    if value.trim().is_empty() {
                        return Err(ArgsError::MissingValue { flag: "--address" });
                    }
                    parsed.address = Some(value);
                }
                Some("--native-networking") => {
                    let value = next_value(&mut arguments, "--native-networking")?;
                    parsed.native_transport = Some(match value.to_str() {
                        Some("addressed") => protocol::NativeTransport::Addressed,
                        Some("raknet") => protocol::NativeTransport::RakNet,
                        Some("nethernet-lan") => protocol::NativeTransport::NetherNetLan,
                        _ => return Err(ArgsError::InvalidNativeTransport),
                    });
                }
                Some("--import-pack") => {
                    parsed
                        .import_packs
                        .push(PathBuf::from(next_value(&mut arguments, "--import-pack")?));
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
                Some("--movement-evidence-target") => {
                    let value = next_value(&mut arguments, "--movement-evidence-target")?
                        .into_string()
                        .map_err(|_| ArgsError::InvalidUtf8 {
                            flag: "--movement-evidence-target",
                        })?;
                    parsed.movement_evidence_target = Some(EvidenceTarget::parse(value)?);
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
                Some("--language") => {
                    let value = next_value(&mut arguments, "--language")?
                        .into_string()
                        .map_err(|_| ArgsError::InvalidUtf8 { flag: "--language" })?;
                    if !assets::is_language_code(&value) {
                        return Err(ArgsError::InvalidLanguage(value));
                    }
                    parsed.language = Some(value);
                }
                Some("--gui-scale") => {
                    let value = next_value(&mut arguments, "--gui-scale")?
                        .into_string()
                        .map_err(|_| ArgsError::InvalidUtf8 {
                            flag: "--gui-scale",
                        })?;
                    parsed.gui_scale = if value.eq_ignore_ascii_case("auto") {
                        None
                    } else {
                        Some(
                            value
                                .parse::<u8>()
                                .ok()
                                .filter(|scale| (1..=4).contains(scale))
                                .ok_or_else(|| ArgsError::InvalidGuiScale(value.clone()))?,
                        )
                    };
                }
                Some("--render-mode") => {
                    let value = next_value(&mut arguments, "--render-mode")?
                        .into_string()
                        .map_err(|_| ArgsError::InvalidUtf8 {
                            flag: "--render-mode",
                        })?;
                    parsed.render_mode =
                        Some(RenderMode::parse(&value).ok_or(ArgsError::InvalidRenderMode(value))?);
                }
                Some("--shader-pack") => {
                    let value = next_value(&mut arguments, "--shader-pack")?;
                    if value.is_empty() {
                        return Err(ArgsError::EmptyShaderPack);
                    }
                    parsed.shader_pack = Some(PathBuf::from(value));
                }
                Some("--hide-toasts") => parsed.hide_toasts = true,
                Some("--fixed-time") => {
                    let value = next_value(&mut arguments, "--fixed-time")?
                        .into_string()
                        .map_err(|_| ArgsError::InvalidUtf8 {
                            flag: "--fixed-time",
                        })?;
                    parsed.fixed_time = Some(
                        value
                            .parse::<u32>()
                            .map_err(|_| ArgsError::InvalidFixedTime(value.clone()))?,
                    );
                }
                Some("--frame-time-counter") => {
                    let value = next_value(&mut arguments, "--frame-time-counter")?
                        .into_string()
                        .map_err(|_| ArgsError::InvalidUtf8 {
                            flag: "--frame-time-counter",
                        })?;
                    let seconds = value
                        .parse::<f64>()
                        .ok()
                        .filter(|seconds| (0.0..3600.0).contains(seconds))
                        .ok_or_else(|| ArgsError::InvalidFrameTimeCounter(value.clone()))?;
                    parsed.frame_time_counter_ms = Some((seconds * 1000.0).round() as u32);
                }
                _ if resource_pack::is_pack_import_path(std::path::Path::new(&argument)) => {
                    parsed.import_packs.push(PathBuf::from(argument));
                }
                _ => return Err(ArgsError::Unknown(argument)),
            }
        }
        if parsed.movement_candidate_physics && parsed.movement_evidence_target.is_none() {
            return Err(ArgsError::MovementCandidateRequiresEvidence);
        }
        if parsed.shader_pack.is_some() {
            match parsed.render_mode {
                None | Some(RenderMode::ShaderPack) => {
                    parsed.render_mode = Some(RenderMode::ShaderPack);
                }
                Some(mode) => {
                    return Err(ArgsError::ConflictingShaderPackRenderMode(mode.as_str()));
                }
            }
        }
        if parsed.force_vsync && parsed.no_vsync {
            return Err(ArgsError::ConflictingVsyncFlags);
        }
        if parsed.movement_evidence_target.is_some()
            && (parsed.acceptance_seconds.is_none() || parsed.metrics_out.is_none())
        {
            return Err(ArgsError::MovementEvidenceRequiresAttributableRun);
        }
        if parsed.native_transport.is_some()
            && (!cfg!(all(
                feature = "native-networking",
                not(target_arch = "wasm32")
            )) || (parsed.connection_requested() && parsed.address.is_none()))
        {
            return Err(ArgsError::NativeNetworkingUnavailable);
        }
        if parsed.parallel_resource_pack_urls && parsed.native_transport.is_none() {
            return Err(ArgsError::ParallelPackUrlsRequireNativeNetworking);
        }
        parsed.validate_acceptance_support(cfg!(feature = "acceptance"))?;
        parsed.validate_developer_support(cfg!(feature = "developer-control"))?;
        Ok(ParseOutcome::Run(Box::new(parsed)))
    }

    #[must_use]
    pub fn connection_requested(&self) -> bool {
        self.address.is_some() || self.acceptance_seconds.is_some() || self.metrics_out.is_some()
    }

    /// Selects the native target; builds without native capability refuse the request.
    pub(crate) fn session_endpoint(&self) -> Result<protocol::SessionEndpoint, ArgsError> {
        if let Some(transport) = self.native_transport {
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            if let Some(address) = &self.address {
                return match crate::session::destination::classify(address, transport, false) {
                    crate::session::destination::Destination::Network(target) => {
                        Ok(protocol::SessionEndpoint::Native(target))
                    }
                    crate::session::destination::Destination::FriendSelectionRequired => {
                        Err(ArgsError::NativeFriendSelectionRequired)
                    }
                    crate::session::destination::Destination::LocalWorld => {
                        Err(ArgsError::NativeNetworkingUnavailable)
                    }
                };
            }
            let _ = transport;
            return Err(ArgsError::NativeNetworkingUnavailable);
        }
        Err(ArgsError::NativeNetworkingUnavailable)
    }
}

fn next_value<I>(arguments: &mut I, flag: &'static str) -> Result<OsString, ArgsError>
where
    I: Iterator<Item = OsString>,
{
    arguments.next().ok_or(ArgsError::MissingValue { flag })
}

#[cfg(test)]
mod tests;
