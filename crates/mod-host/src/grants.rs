//! Explicit capability grants for one component instance.

/// Explicit per-instance authority; optional capabilities are denied by default.
/// Field names are the registration and set-file grant names.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModGrants {
    /// Allows read-only current-session local inventory and status effect snapshots.
    pub player_state: bool,
    /// Allows this instance to replace visual time only.
    pub environment: bool,
    /// Allows current-frame remote player and camera pose reads.
    pub players: bool,
    /// Allows bounded local camera rotation, rigs, and per-frame teleport aim preservation.
    pub camera: bool,
    /// Allows one immediate air use after each slot change and air use while attacking.
    pub item_use: bool,
    /// Allows local key edges, reserved bindings and the retained settings panel.
    pub controls: bool,
    /// Allows bounded host-rendered HUD cards and a local custom crosshair.
    pub hud: bool,
    /// Allows bounded actor attack range and held-attack press requests.
    pub interaction: bool,
    /// Allows local physics snapshots and ordinary one-shot jump input requests.
    pub movement: bool,
    /// Allows the selected component's bounded companion settings file.
    pub settings: bool,
    /// Allows sandboxed post passes and bounded world primitives.
    pub render: bool,
    /// Lets render passes read scene depth.
    pub render_depth: bool,
    /// Allows current-frame reads of nearby non-player actors.
    pub entities: bool,
    /// Command names this instance may request; empty denies command requests.
    pub commands: Vec<String>,
    /// Allows bounded post-login packet delay through the private core endpoint.
    pub packet_delay: bool,
    /// Allows a package's overlay and view beside the container screens.
    pub screen: bool,
    /// Allows reading the session's items.
    pub items: bool,
    /// Allows reading the session's recipes.
    pub recipes: bool,
    /// Allows delivering a package's declared keys.
    pub keys: bool,
    /// Allows retained full-block highlights of matching loaded blocks.
    pub block_highlights: bool,
    /// Allows retained local fullbright lighting, without altering server light data.
    pub fullbright: bool,
}
