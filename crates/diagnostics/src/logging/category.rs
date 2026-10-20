//! Log categories: named groups of `tracing` targets that a filter spec can set together.
use tracing_subscriber::filter::{Directive, LevelFilter};

/// A named group of log targets. A spec directive `<name>=<level>` sets every target of the
/// category; a bare `<name>` enables all of its levels, as a bare target does in `RUST_LOG`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Net,
    World,
    Render,
    Ui,
    Packs,
    Audio,
    Input,
    Core,
    Session,
}

impl Category {
    pub const ALL: [Category; 9] = [
        Self::Net,
        Self::World,
        Self::Render,
        Self::Ui,
        Self::Packs,
        Self::Audio,
        Self::Input,
        Self::Core,
        Self::Session,
    ];

    /// The name a filter spec uses.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Net => "net",
            Self::World => "world",
            Self::Render => "render",
            Self::Ui => "ui",
            Self::Packs => "packs",
            Self::Audio => "audio",
            Self::Input => "input",
            Self::Core => "core",
            Self::Session => "session",
        }
    }

    /// The target prefixes the category sets. Matching follows `EnvFilter`: a target belongs
    /// to a prefix it starts with, and the longest matching prefix wins.
    pub const fn targets(self) -> &'static [&'static str] {
        match self {
            Self::Net => &[
                "protocol",
                "web_transport",
                "reqwest",
                "cinnabar::runtime::network",
            ],
            Self::World => &[
                "world",
                "client_world",
                "client_interaction",
                "chunk_pipeline",
                "far_terrain",
                "gameplay",
                "inventory",
                "meshing",
                "sim",
                "cinnabar::runtime::world",
                "cinnabar::block",
                "cinnabar::far_terrain",
                "cinnabar::interaction_authority",
                "cinnabar::item_use",
                "cinnabar::local_player",
                "cinnabar::melee",
                "cinnabar::movement",
                "cinnabar::pick_block",
                "cinnabar::survival_mining",
            ],
            Self::Render => &[
                "render",
                "actor_animation",
                "client_presentation",
                "particles",
                "wgpu",
                "naga",
                "bevy_render",
                "cinnabar::app::render_setup",
                "cinnabar::camera",
                "cinnabar::environment",
                "cinnabar::frame_pacing",
                "cinnabar::particles",
                "cinnabar::present_mode",
                "cinnabar::presentation",
                "cinnabar::primitive_shapes",
                "cinnabar::render",
                "cinnabar::runtime::visibility",
            ],
            Self::Ui => &[
                "ui",
                "bootstrap_ui",
                "client_ui",
                "client_shell",
                "json_ui",
                "mod_ui",
                "oreui",
                "cinnabar::hotbar",
                "cinnabar::hud_tools",
                "cinnabar::ui_runtime",
            ],
            Self::Packs => &[
                "assets",
                "client_extensions",
                "mod_host",
                "pack_runtime",
                "resource_pack",
                "cinnabar::asset_bindings",
                "cinnabar::modding",
                "cinnabar::server_experiences",
            ],
            Self::Audio => &[
                "client_audio",
                "rodio",
                "cpal",
                "symphonia",
                "cinnabar::audio",
                "cinnabar::named_audio",
                "cinnabar::session_audio",
            ],
            Self::Input => &[
                "semantic_input",
                "gilrs",
                "winit",
                "bevy_input",
                "cinnabar::semantic_controls",
            ],
            Self::Core => &[
                "bridge",
                "crash_monitor",
                "diagnostics",
                "launcher",
                "cinnabar::lifecycle",
            ],
            Self::Session => &[
                "client_session",
                "client_runtime",
                "client_state",
                "acceptance",
                "rich_presence",
                "discord_presence",
                "cinnabar::app::session",
                "cinnabar::runtime::telemetry",
                "cinnabar::session",
            ],
        }
    }

    /// The category a spec names.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|category| category.name() == name)
    }
}

/// A spec directive that is neither a category directive nor a valid `EnvFilter` directive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidDirective {
    pub directive: String,
    pub reason: String,
}

/// Expands one spec directive into `EnvFilter` directives.
fn expand_one(directive: &str) -> Result<Vec<String>, InvalidDirective> {
    let invalid = |reason: String| InvalidDirective {
        directive: directive.to_owned(),
        reason,
    };
    let (name, level) = match directive.split_once('=') {
        Some((name, level)) => (name, Some(level)),
        None => (directive, None),
    };
    if let Some(category) = Category::from_name(name) {
        let level = match level {
            Some(level) => level
                .parse::<LevelFilter>()
                .map_err(|error| invalid(error.to_string()))?
                .to_string()
                .to_ascii_lowercase(),
            None => "trace".to_owned(),
        };
        return Ok(category
            .targets()
            .iter()
            .map(|target| format!("{target}={level}"))
            .collect());
    }
    directive
        .parse::<Directive>()
        .map_err(|error| invalid(error.to_string()))?;
    Ok(vec![directive.to_owned()])
}

/// Expands a spec, failing on the first invalid directive.
pub(super) fn expand(spec: &str) -> Result<Vec<String>, InvalidDirective> {
    let mut expanded = Vec::new();
    for directive in split(spec) {
        expanded.extend(expand_one(directive)?);
    }
    Ok(expanded)
}

/// Expands a spec, skipping invalid directives and returning them separately.
pub(super) fn expand_lossy(spec: &str) -> (Vec<String>, Vec<InvalidDirective>) {
    let mut expanded = Vec::new();
    let mut rejected = Vec::new();
    for directive in split(spec) {
        match expand_one(directive) {
            Ok(more) => expanded.extend(more),
            Err(error) => rejected.push(error),
        }
    }
    (expanded, rejected)
}

/// The level `directives` (applied in order, later ones overriding equal targets) give events
/// of `target` outside any span: the longest directive target that prefixes it, else the
/// default directive, else off. Span and field directives are ignored.
fn target_level<'a>(target: &str, directives: impl IntoIterator<Item = &'a str>) -> LevelFilter {
    let mut best: Option<(usize, LevelFilter)> = None;
    let mut default = None;
    for directive in directives {
        if directive.contains('[') {
            continue;
        }
        let (name, level) = match directive.split_once('=') {
            Some((name, level)) => match level.parse::<LevelFilter>() {
                Ok(level) => (Some(name), level),
                Err(_) => continue,
            },
            None => match directive.parse::<LevelFilter>() {
                Ok(level) => (None, level),
                Err(_) => (Some(directive), LevelFilter::TRACE),
            },
        };
        match name {
            None => default = Some(level),
            Some(name) if target.starts_with(name) => {
                if best.is_none_or(|(length, _)| name.len() >= length) {
                    best = Some((name.len(), level));
                }
            }
            Some(_) => {}
        }
    }
    best.map(|(_, level)| level)
        .or(default)
        .unwrap_or(LevelFilter::OFF)
}

/// The most verbose level `directives` give any of the category's targets.
pub(super) fn category_level<'a>(
    category: Category,
    directives: impl IntoIterator<Item = &'a str> + Clone,
) -> LevelFilter {
    category
        .targets()
        .iter()
        .map(|target| target_level(target, directives.clone()))
        .max()
        .unwrap_or(LevelFilter::OFF)
}

/// Splits a filter string into its directives, as `EnvFilter` does.
pub(super) fn split(spec: &str) -> impl Iterator<Item = &str> + Clone {
    spec.split(',')
        .map(str::trim)
        .filter(|directive| !directive.is_empty())
}
