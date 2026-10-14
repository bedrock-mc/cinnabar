//! Explicit capability grants and their permission controls, declared together.

/// A permission derived from a field of [`ModGrants`].
pub struct Capability {
    pub name: &'static str,
    pub description: &'static str,
    pub enabled: fn(&ModGrants) -> bool,
    pub set: fn(&mut ModGrants, bool, Option<&ModGrants>),
}

trait GrantValue {
    /// Whether this field grants any authority.
    fn enabled(&self) -> bool;
    /// Sets only authority explicitly named by the request for non-boolean fields.
    fn set(&mut self, enabled: bool, requested: Option<&Self>);
}
impl GrantValue for bool {
    fn enabled(&self) -> bool {
        *self
    }
    fn set(&mut self, enabled: bool, _: Option<&Self>) {
        *self = enabled;
    }
}
impl GrantValue for Vec<String> {
    fn enabled(&self) -> bool {
        !self.is_empty()
    }
    fn set(&mut self, enabled: bool, requested: Option<&Self>) {
        *self = if enabled {
            requested.cloned().unwrap_or_default()
        } else {
            Vec::new()
        };
    }
}

macro_rules! grants {
    ($(#[doc = $doc:literal] pub $field:ident: $ty:ty,)* ) => {
        /// Explicit per-instance authority; optional capabilities are denied by default.
        #[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
        #[serde(default, deny_unknown_fields)]
        pub struct ModGrants { $(#[doc = $doc] pub $field: $ty,)* }
        impl ModGrants {
            /// Every optional capability, generated from the same fields as serialization.
            pub const CAPABILITIES: &'static [Capability] = &[$(Capability {
                name: stringify!($field), description: $doc,
                enabled: |grants| GrantValue::enabled(&grants.$field),
                set: |grants, enabled, requested| GrantValue::set(&mut grants.$field, enabled, requested.map(|grants| &grants.$field)),
            },)*];
        }
    };
}

grants! {

    /// Allows read-only current-session local inventory and status effect snapshots.
    pub player_state: bool,
    /// Allows this instance to replace visual time and local menu branding.
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descriptors_cover_every_serialized_grant_and_default_off() {
        let grants = ModGrants::default();
        let json = serde_json::to_value(&grants).unwrap();
        let fields = json.as_object().unwrap();
        assert_eq!(fields.len(), ModGrants::CAPABILITIES.len());
        for capability in ModGrants::CAPABILITIES {
            assert!(fields.contains_key(capability.name));
            assert!(!(capability.enabled)(&grants));
        }
    }
    #[test]
    fn command_toggle_grants_only_declared_names() {
        let descriptor = ModGrants::CAPABILITIES
            .iter()
            .find(|capability| capability.name == "commands")
            .unwrap();
        let mut grants = ModGrants::default();
        (descriptor.set)(&mut grants, true, None);
        assert!(grants.commands.is_empty());
        let requested = ModGrants {
            commands: vec!["ability".into()],
            ..Default::default()
        };
        (descriptor.set)(&mut grants, true, Some(&requested));
        assert_eq!(grants.commands, ["ability"]);
        (descriptor.set)(&mut grants, false, Some(&requested));
        assert!(grants.commands.is_empty());
    }
}
