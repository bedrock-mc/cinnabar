//! Server-pack entities for one session: compiled at admission into their own index space
//! and layered over the vanilla catalog. A bad file skips its entity, never the session.

use std::sync::Arc;

use assets::{RuntimeEntityAssets, RuntimeEquipmentCatalog};
use resource_pack::LayeredPackView;

mod collect;
use collect::collect_files;

use super::resource_packs::{StackFingerprint, parse_pack_json};

pub(crate) use assets::SessionEntityPack;
pub(crate) use client_presentation::session_assets::SessionItems;

/// Vanilla definitions server-pack entities may reference; replaced whenever a carrier loads.
static VANILLA_REFS: std::sync::RwLock<Option<Arc<assets::VanillaEntityRefs>>> =
    std::sync::RwLock::new(None);

pub(crate) fn set_vanilla_refs(refs: assets::VanillaEntityRefs) {
    *VANILLA_REFS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(refs));
}

fn vanilla_refs() -> Option<Arc<assets::VanillaEntityRefs>> {
    VANILLA_REFS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Everything a compile reads: the pack stack and the vanilla refs it resolved against.
#[derive(Clone, PartialEq)]
struct EntityInputs {
    stack: StackFingerprint,
    vanilla: Option<Arc<assets::VanillaEntityRefs>>,
}

type CachedEntities = (
    EntityInputs,
    Option<Arc<SessionEntityPack>>,
    Option<std::collections::BTreeSet<resource_pack::PackDependency>>,
);

/// The previous compile, reused when the same inputs rejoin.
#[derive(Default)]
struct EntityCache(std::sync::Mutex<Option<CachedEntities>>);

impl EntityCache {
    /// Returns the cached pack for `inputs`, compiling without holding the lock on a miss.
    fn get_or_compile(
        &self,
        inputs: EntityInputs,
        view: &LayeredPackView,
        compile: impl FnOnce(
            &LayeredPackView,
            Option<&assets::VanillaEntityRefs>,
        ) -> Option<Arc<SessionEntityPack>>,
    ) -> Option<Arc<SessionEntityPack>> {
        {
            let cache = self.lock();
            if let Some((cached, pack, files)) = cache.as_ref()
                && *cached == inputs
                && (view.dependencies().is_none() || files.is_some())
            {
                if let (Some(dependencies), Some(files)) = (view.dependencies(), files) {
                    dependencies.extend(files.clone());
                }
                return pack.clone();
            }
        }
        let pack = compile(view, inputs.vanilla.as_deref());
        *self.lock() = Some((
            inputs,
            pack.clone(),
            view.dependencies().map(|files| files.snapshot()),
        ));
        pack
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<CachedEntities>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

static ENTITY_CACHE: EntityCache = EntityCache(std::sync::Mutex::new(None));

/// Compiles the stack's entity files; `None` when it defines no usable entity.
pub(super) fn compile_session_entities(
    fingerprint: &StackFingerprint,
    view: &LayeredPackView,
) -> Option<Arc<SessionEntityPack>> {
    let inputs = EntityInputs {
        stack: fingerprint.clone(),
        vanilla: vanilla_refs(),
    };
    ENTITY_CACHE.get_or_compile(inputs, view, compile)
}

fn compile(
    view: &LayeredPackView,
    vanilla: Option<&assets::VanillaEntityRefs>,
) -> Option<Arc<SessionEntityPack>> {
    let files = collect_files(view, vanilla);
    let compiled = match pack_compiler::compile_actor_pack(files) {
        Ok(Some(compiled)) => compiled,
        Ok(None) => return None,
        Err(error) => {
            bevy::log::warn!(%error, "server pack entities were not applied");
            return None;
        }
    };
    let skipped = compiled.skipped;
    if skipped != pack_compiler::EntityPackSkips::default() || !compiled.fallbacks.is_empty() {
        bevy::log::warn!(
            oversized = skipped.oversized,
            unparsable = skipped.unparsable,
            over_budget = skipped.over_budget,
            isolated = skipped.isolated,
            rigs_without_artwork = compiled.fallbacks.len(),
            "server pack entities are incomplete"
        );
    }
    let equipment = if compiled.equipment_bindings.is_empty() {
        None
    } else {
        match RuntimeEquipmentCatalog::from_parts(
            compiled.identity,
            compiled.equipment_bindings,
            compiled.equipment_textures,
        ) {
            Ok(catalog) => Some(Arc::new(catalog)),
            Err(error) => {
                bevy::log::warn!(%error, "server pack attachables were rejected");
                None
            }
        }
    };
    let assets = match RuntimeEntityAssets::from_compiled(compiled.entities) {
        Ok(assets) => Arc::new(assets),
        Err(error) => {
            bevy::log::warn!(%error, "server pack entity catalog was rejected");
            return None;
        }
    };
    Some(Arc::new(SessionEntityPack {
        assets,
        textures: compiled.textures.into(),
        bindings: compiled.bindings.into(),
        equipment,
    }))
}

/// Property defaults of each `entities/*.json` behavior definition the stack carries, keyed by
/// entity type. Only the winning copy of a file is read; a malformed property is skipped.
pub(super) fn pack_property_defaults(
    view: &LayeredPackView,
) -> Vec<(Arc<str>, Vec<client_world::PropertyDefault>)> {
    view.winning_files("entities/", "json")
        .into_iter()
        .filter_map(|(_, bytes)| {
            let root = parse_pack_json(&bytes)?;
            let description = &root["minecraft:entity"]["description"];
            let identifier: Arc<str> = description["identifier"].as_str()?.into();
            let defaults = description["properties"]
                .as_object()?
                .iter()
                .filter_map(|(name, definition)| property_default(name, definition))
                .collect::<Vec<_>>();
            (!defaults.is_empty()).then_some((identifier, defaults))
        })
        .collect()
}

fn property_default(
    name: &str,
    definition: &serde_json::Value,
) -> Option<client_world::PropertyDefault> {
    let mut values = None;
    let default = &definition["default"];
    let number = match definition["type"].as_str()? {
        "bool" => f32::from(u8::from(default.as_bool()?)),
        "int" | "float" => default.as_f64()? as f32,
        "enum" => {
            let names = definition["values"]
                .as_array()?
                .iter()
                .map(|value| value.as_str().map(Arc::<str>::from))
                .collect::<Option<Vec<_>>>()?;
            let wanted = default.as_str()?;
            let index = names.iter().position(|value| value.as_ref() == wanted)?;
            values = Some(names.into());
            index as f32
        }
        _ => return None,
    };
    number.is_finite().then(|| client_world::PropertyDefault {
        name: name.into(),
        values,
        default: number,
    })
}

pub(super) fn entity_identifier(bytes: &[u8]) -> Option<String> {
    parse_pack_json(bytes)?["minecraft:client_entity"]["description"]["identifier"]
        .as_str()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::entity_identifier;

    #[test]
    fn property_defaults_read_bool_number_and_enum_index() {
        use super::property_default;
        let json = |text: &str| serde_json::from_str::<serde_json::Value>(text).unwrap();
        let flag = property_default("a", &json(r#"{"type":"bool","default":true}"#)).unwrap();
        assert_eq!(flag.default, 1.0);
        let level = property_default("b", &json(r#"{"type":"int","default":4}"#)).unwrap();
        assert_eq!(level.default, 4.0);
        let variant = property_default(
            "c",
            &json(r#"{"type":"enum","values":["x","y"],"default":"y"}"#),
        )
        .unwrap();
        assert_eq!((variant.default, variant.values.unwrap().len()), (1.0, 2));
        assert!(property_default("d", &json(r#"{"type":"weird"}"#)).is_none());
    }

    #[test]
    fn review_property_defaults_skip_invalid_types_membership_and_nonfinite_values() {
        for definition in [
            serde_json::json!({"type":"bool", "default":"invalid"}),
            serde_json::json!({"type":"float", "default":1e100}),
            serde_json::json!({"type":"float", "default":"invalid"}),
            serde_json::json!({"type":"int", "default":"invalid"}),
            serde_json::json!({"type":"enum", "values":["x"], "default":"missing"}),
            serde_json::json!({"type":"enum", "values":[4,"x"], "default":"x"}),
        ] {
            assert!(
                super::property_default("test", &definition).is_none(),
                "{definition}"
            );
        }
    }

    fn empty_view() -> resource_pack::LayeredPackView {
        resource_pack::LayeredPackView::new(resource_pack::validate_handoff(
            protocol::ResourcePackHandoff::from_archives(Vec::new()),
        ))
    }

    fn inputs(vanilla: Option<assets::VanillaEntityRefs>) -> super::EntityInputs {
        super::EntityInputs {
            stack: Vec::new(),
            vanilla: vanilla.map(std::sync::Arc::new),
        }
    }

    fn refs_with_animation() -> assets::VanillaEntityRefs {
        let mut refs = assets::VanillaEntityRefs::new();
        refs.animations
            .insert("animation.test".into(), serde_json::Value::Null);
        refs
    }

    // The same stack recompiles when the vanilla refs it resolved against change.
    #[test]
    fn entity_cache_keys_on_vanilla_refs() {
        let (view, cache) = (empty_view(), super::EntityCache::default());
        let compiles = std::cell::Cell::new(0);
        for vanilla in [
            None,
            None,
            Some(assets::VanillaEntityRefs::new()),
            Some(refs_with_animation()),
        ] {
            cache.get_or_compile(inputs(vanilla), &view, |_, _| {
                compiles.set(compiles.get() + 1);
                None
            });
        }
        assert_eq!(compiles.get(), 3);
    }

    #[test]
    fn entity_cache_compiles_without_holding_its_lock() {
        let (view, cache) = (empty_view(), super::EntityCache::default());
        cache.get_or_compile(inputs(None), &view, |_, _| {
            assert!(cache.0.try_lock().is_ok());
            None
        });
    }

    #[test]
    fn later_vanilla_refs_replace_earlier_ones() {
        let later = refs_with_animation();
        super::set_vanilla_refs(assets::VanillaEntityRefs::new());
        super::set_vanilla_refs(later.clone());
        assert_eq!(super::vanilla_refs().as_deref(), Some(&later));
    }

    #[test]
    fn identifier_is_read_from_the_client_entity_description() {
        let json = br#"{"minecraft:client_entity":{"description":{"identifier":"a:b"}}}"#;
        assert_eq!(entity_identifier(json).as_deref(), Some("a:b"));
        assert_eq!(entity_identifier(b"{}"), None);
    }
}

#[cfg(test)]
mod pack_report;

#[cfg(test)]
mod equipment_report;

#[cfg(test)]
mod render_report;

#[cfg(all(test, unix))]
mod lobby_bench;

#[cfg(test)]
mod scene_report;

#[cfg(test)]
mod projectile_report;

#[cfg(test)]
mod mob_motion_report;
