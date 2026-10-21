//! Explicit capability grants and their permission controls, declared together.

/// A permission derived from a field of [`ModGrants`].
pub struct Capability {
    pub name: &'static str,
    pub description: &'static str,
    /// Short name shown in permission controls.
    pub label: &'static str,
    /// Whether this capability can change combat, movement or gameplay input.
    pub sensitive: bool,
    pub enabled: fn(&ModGrants) -> bool,
    /// Whether every requested value for this capability is already granted.
    pub requested_allowed: fn(&ModGrants, &ModGrants) -> bool,
    pub set: fn(&mut ModGrants, bool, Option<&ModGrants>),
}

trait GrantValue {
    /// Whether this field grants any authority.
    fn enabled(&self) -> bool;
    /// Whether this value includes all authority named by the request.
    fn covers(&self, requested: &Self) -> bool;
    /// Sets only authority explicitly named by the request for non-boolean fields.
    fn set(&mut self, enabled: bool, requested: Option<&Self>);
}
impl GrantValue for bool {
    fn enabled(&self) -> bool {
        *self
    }
    /// Treats a disabled request as covered, otherwise requires the grant.
    fn covers(&self, requested: &Self) -> bool {
        !requested || *self
    }
    fn set(&mut self, enabled: bool, _: Option<&Self>) {
        *self = enabled;
    }
}
impl GrantValue for Vec<String> {
    fn enabled(&self) -> bool {
        !self.is_empty()
    }
    /// Requires every requested command name to be present in the retained grant.
    fn covers(&self, requested: &Self) -> bool {
        requested.iter().all(|value| self.contains(value))
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
    ($(#[doc = $doc:literal] #[permission($label:literal, $sensitive:literal)] pub $field:ident: $ty:ty,)* ) => {
        /// Explicit per-instance authority; optional capabilities are denied by default.
        #[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
        #[serde(default, deny_unknown_fields)]
        pub struct ModGrants { $(#[doc = $doc] pub $field: $ty,)* }
        impl ModGrants {
            /// Every optional capability, generated from the same fields as serialization.
            pub const CAPABILITIES: &'static [Capability] = &[$(Capability {
                name: stringify!($field), description: $doc, label: $label, sensitive: $sensitive,
                enabled: |grants| GrantValue::enabled(&grants.$field),
                requested_allowed: |grants, requested| GrantValue::covers(&grants.$field, &requested.$field),
                set: |grants, enabled, requested| GrantValue::set(&mut grants.$field, enabled, requested.map(|grants| &grants.$field)),
            },)*];
        }
    };
}

grants! {
    /// Allows bounded read-only regions and resolved block properties.
    #[permission("Terrain", false)]
    pub terrain: bool,
    /// Allows read-only current-session local inventory and status effect snapshots.
    #[permission("Player status", false)]
    pub player_state: bool,
    /// Allows this instance to replace visual time and local menu branding.
    #[permission("Environment", false)]
    pub environment: bool,
    /// Allows current-frame remote player and camera pose reads with independent user consent.
    #[permission("Player poses", false)]
    pub players: bool,
    /// Allows bounded local camera rotation, rigs, and per-frame teleport aim preservation.
    #[permission("Camera rotation", true)]
    pub camera: bool,
    /// Allows one immediate air use after each slot change and air use while attacking.
    #[permission("Item use", true)]
    pub item_use: bool,
    /// Allows local key edges, reserved bindings and the retained settings panel.
    #[permission("Controls", true)]
    pub controls: bool,
    /// Allows bounded host-rendered HUD cards and a local custom crosshair.
    #[permission("HUD", false)]
    pub hud: bool,
    /// Allows bounded actor attack range and held-attack press requests.
    #[permission("Attack reach", true)]
    pub interaction: bool,
    /// Allows local physics snapshots and ordinary one-shot jump input requests.
    #[permission("Movement", true)]
    pub movement: bool,
    /// Allows the selected component's bounded companion settings file.
    #[permission("Settings file", false)]
    pub settings: bool,
    /// Allows sandboxed post passes and bounded world primitives.
    #[permission("Rendering", false)]
    pub render: bool,
    /// Allows bounded retained world geometry.
    #[permission("World drawing", false)]
    pub world_draw: bool,
    /// Allows session-authorized actor observations.
    #[permission("Entity observation", false)]
    pub entity_observe: bool,
    /// Allows drawing attached to authorized actors.
    #[permission("Entity highlights", false)]
    pub entity_highlight: bool,
    /// Allows elevated drawing and protected world observations.
    #[permission("Through walls", false)]
    pub render_through_walls: bool,
    /// Allows bounded procedural drawing materials.
    #[permission("Drawing materials", false)]
    pub render_material: bool,
    /// Allows sandboxed scene post effects.
    #[permission("Post effects", false)]
    pub render_post: bool,
    /// Lets render passes read scene depth.
    #[permission("Scene depth", false)]
    pub render_depth: bool,
    /// Allows current-frame reads of nearby non-player actors.
    #[permission("Entities", false)]
    pub entities: bool,
    /// Command names this instance may request; empty denies command requests.
    #[permission("Commands", true)]
    pub commands: Vec<String>,
    /// Allows bounded post-login packet delay through the private core endpoint.
    #[permission("Packet delay", true)]
    pub packet_delay: bool,
    /// Allows a package's overlay and view beside the container screens.
    #[permission("Screens", false)]
    pub screen: bool,
    /// Allows reading the session's items.
    #[permission("Items", false)]
    pub items: bool,
    /// Allows reading the session's recipes.
    #[permission("Recipes", false)]
    pub recipes: bool,
    /// Allows delivering a package's declared keys.
    #[permission("Keys", true)]
    pub keys: bool,
    /// Allows a package's vanilla inventory requests: creative takes and recipe fills.
    #[permission("Inventory requests", true)]
    pub inventory: bool,
    /// Allows retained full-block highlights of matching loaded blocks.
    #[permission("Block highlights", false)]
    pub block_highlights: bool,
    /// Allows retained local fullbright lighting, without altering server light data.
    #[permission("Fullbright", false)]
    pub fullbright: bool,
    /// Lets a trusted packet mod see and change every session packet, and add resource packs.
    #[permission("Session packets", true)]
    pub packets: bool,
    /// Links WASI clocks, randomness and captured output into a packet mod, never files or network.
    #[permission("WASI runtime", false)]
    pub wasi: bool,
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
            assert!(!capability.label.is_empty());
        }
    }
    #[test]
    fn gameplay_authority_is_marked_in_its_capability_definition() {
        for name in [
            "camera",
            "item_use",
            "interaction",
            "packet_delay",
            "packets",
            "controls",
            "movement",
            "commands",
            "keys",
            "inventory",
        ] {
            assert!(
                ModGrants::CAPABILITIES
                    .iter()
                    .find(|capability| capability.name == name)
                    .unwrap()
                    .sensitive,
                "{name}"
            );
        }
        assert!(
            !ModGrants::CAPABILITIES
                .iter()
                .find(|capability| capability.name == "settings")
                .unwrap()
                .sensitive
        );
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
        assert!((descriptor.requested_allowed)(&grants, &requested));
        let updated = ModGrants {
            commands: vec!["ability".into(), "say".into()],
            ..Default::default()
        };
        assert!(!(descriptor.requested_allowed)(&grants, &updated));
        (descriptor.set)(&mut grants, false, Some(&requested));
        assert!(grants.commands.is_empty());
    }
}
