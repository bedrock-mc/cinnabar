//! Server-pack entities for one session: compiled at admission into their own index space
//! and layered over the vanilla catalog. A bad file skips its entity, never the session.

use std::{path::PathBuf, sync::Arc};

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

static VANILLA_PACK_DIR: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

/// The installed vanilla layer supplies rasters omitted by server packs.
pub(crate) fn set_vanilla_pack_dir(path: PathBuf) {
    *VANILLA_PACK_DIR
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path);
}

fn vanilla_pack_dir() -> Option<PathBuf> {
    VANILLA_PACK_DIR
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Everything a compile reads: the pack stack and the vanilla refs it resolved against.
#[derive(Clone, PartialEq)]
struct EntityInputs {
    stack: StackFingerprint,
    vanilla: Option<Arc<assets::VanillaEntityRefs>>,
    vanilla_pack_dir: Option<PathBuf>,
}

type CachedEntities = (
    EntityInputs,
    Option<Arc<SessionEntityPack>>,
    Option<std::collections::BTreeSet<resource_pack::PackDependency>>,
);

/// A compiled pack and, when asked for, the entity blob it was built from.
type Compiled = (Option<Arc<SessionEntityPack>>, Option<Box<[u8]>>);

/// The previous compile, reused when the same inputs rejoin.
#[derive(Default)]
struct EntityCache(std::sync::Mutex<Option<CachedEntities>>);

impl EntityCache {
    /// Returns the cached pack for `inputs`, then a disk hit, compiling without holding the lock.
    fn get_or_compile(
        &self,
        inputs: EntityInputs,
        view: &LayeredPackView,
        disk: Option<&client_session::compile_cache::CompileCache>,
        compile: impl FnOnce(
            &LayeredPackView,
            Option<&assets::VanillaEntityRefs>,
            Option<&std::path::Path>,
            bool,
        ) -> Compiled,
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
        // Only a tracked compile knows its reads, which a reload diff of a disk hit needs.
        let disk = disk.filter(|_| view.dependencies().is_some());
        let key = disk.map(|_| disk_key(&inputs));
        let stored = disk.zip(key.as_ref()).and_then(|(disk, key)| {
            let entry = disk.load(key)?;
            let decoded = decode_entry(&entry);
            if decoded.is_none() {
                bevy::log::warn!("discarding an undecodable compiled entity pack");
            }
            decoded
        });
        let pack = if let Some((pack, files)) = stored {
            if let Some(dependencies) = view.dependencies() {
                dependencies.extend(files);
            }
            pack
        } else {
            let (pack, blob) = compile(
                view,
                inputs.vanilla.as_deref(),
                inputs.vanilla_pack_dir.as_deref(),
                disk.is_some(),
            );
            if let (Some(disk), Some(key), Some(files)) = (disk, &key, view.dependencies())
                && let Some(entry) =
                    encode_entry(&files.snapshot(), pack.as_deref(), blob.as_deref())
            {
                disk.store(key, &entry);
            }
            pack
        };
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

/// Names the stack, the vanilla refs it resolves against, and the build.
fn disk_key(inputs: &EntityInputs) -> [u8; 32] {
    use client_session::compile_cache::{cache_key, part};
    use sha2::Digest;
    let mut key = cache_key("entities");
    for (id, version, subpack, content) in &inputs.stack {
        for bytes in [
            id.as_bytes(),
            version.as_bytes(),
            subpack.as_bytes(),
            content,
        ] {
            part(&mut key, bytes);
        }
    }
    part(&mut key, &vanilla_digest(inputs.vanilla.as_ref()));
    if let Some(path) = &inputs.vanilla_pack_dir {
        part(&mut key, path.as_os_str().as_encoded_bytes());
    }
    key.finalize().into()
}

/// Digest of the refs' canonical JSON, computed once per installed refs.
fn vanilla_digest(refs: Option<&Arc<assets::VanillaEntityRefs>>) -> [u8; 32] {
    use sha2::Digest;
    type Memo = Option<(Arc<assets::VanillaEntityRefs>, [u8; 32])>;
    static MEMO: std::sync::Mutex<Memo> = std::sync::Mutex::new(None);
    let Some(refs) = refs else {
        return [0; 32];
    };
    let mut memo = MEMO
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((memoized, digest)) = memo.as_ref()
        && Arc::ptr_eq(memoized, refs)
    {
        return *digest;
    }
    let digest = sha2::Sha256::digest(refs.to_json()).into();
    *memo = Some((Arc::clone(refs), digest));
    digest
}

/// Reads first, then a presence flag and the pack; `None` when the stack defines no entity.
fn encode_entry(
    files: &std::collections::BTreeSet<resource_pack::PackDependency>,
    pack: Option<&SessionEntityPack>,
    blob: Option<&[u8]>,
) -> Option<Vec<u8>> {
    let mut entry = Vec::new();
    resource_pack::PackDependency::encode_set(files, &mut entry);
    match pack {
        None => entry.push(0),
        Some(pack) => {
            entry.push(1);
            match pack.encode(blob?) {
                Ok(bytes) => entry.extend_from_slice(&bytes),
                Err(error) => {
                    bevy::log::debug!(%error, "compiled entity pack is not cacheable");
                    return None;
                }
            }
        }
    }
    Some(entry)
}

fn decode_entry(
    entry: &[u8],
) -> Option<(
    Option<Arc<SessionEntityPack>>,
    std::collections::BTreeSet<resource_pack::PackDependency>,
)> {
    let (files, rest) = resource_pack::PackDependency::decode_set(entry)?;
    let pack = match rest.split_first()? {
        (0, []) => None,
        (1, bytes) => Some(Arc::new(SessionEntityPack::decode(bytes).ok()?)),
        _ => return None,
    };
    Some((pack, files))
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
        vanilla_pack_dir: vanilla_pack_dir(),
    };
    ENTITY_CACHE.get_or_compile(
        inputs,
        view,
        super::resource_packs::compile_cache(),
        compile_encoded,
    )
}

#[cfg(all(test, feature = "reports"))]
fn compile(
    view: &LayeredPackView,
    vanilla: Option<&assets::VanillaEntityRefs>,
) -> Option<Arc<SessionEntityPack>> {
    compile_encoded(view, vanilla, vanilla_pack_dir().as_deref(), false).0
}

/// `encode` also returns the entity blob, which only a disk-cached compile needs.
fn compile_encoded(
    view: &LayeredPackView,
    vanilla: Option<&assets::VanillaEntityRefs>,
    vanilla_pack_dir: Option<&std::path::Path>,
    encode: bool,
) -> Compiled {
    let files = collect_files(view, vanilla, vanilla_pack_dir);
    let compiled = match pack_compiler::compile_actor_pack(files) {
        Ok(Some(compiled)) => compiled,
        Ok(None) => return (None, None),
        Err(error) => {
            bevy::log::warn!(%error, "server pack entities were not applied");
            return (None, None);
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
    let built = if encode {
        RuntimeEntityAssets::from_compiled_encoded(compiled.entities)
    } else {
        RuntimeEntityAssets::from_compiled(compiled.entities).map(|assets| (assets, None))
    };
    let (assets, blob) = match built {
        Ok((assets, blob)) => (Arc::new(assets), blob),
        Err(error) => {
            bevy::log::warn!(%error, "server pack entity catalog was rejected");
            return (None, None);
        }
    };
    let pack = Arc::new(SessionEntityPack {
        assets,
        textures: compiled.textures.into(),
        bindings: compiled.bindings.into(),
        equipment,
    });
    (Some(pack), blob)
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
            vanilla_pack_dir: None,
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
            cache.get_or_compile(inputs(vanilla), &view, None, |_, _, _, _| {
                compiles.set(compiles.get() + 1);
                (None, None)
            });
        }
        assert_eq!(compiles.get(), 3);
    }

    #[test]
    fn entity_cache_recompiles_when_the_installed_vanilla_layer_changes() {
        let (view, cache) = (empty_view(), super::EntityCache::default());
        let compiles = std::cell::Cell::new(0);
        for directory in ["first", "first", "second"] {
            let mut input = inputs(None);
            input.vanilla_pack_dir = Some(directory.into());
            cache.get_or_compile(input, &view, None, |_, _, _, _| {
                compiles.set(compiles.get() + 1);
                (None, None)
            });
        }
        assert_eq!(compiles.get(), 2);
    }

    #[test]
    fn entity_cache_compiles_without_holding_its_lock() {
        let (view, cache) = (empty_view(), super::EntityCache::default());
        cache.get_or_compile(inputs(None), &view, None, |_, _, _, _| {
            assert!(cache.0.try_lock().is_ok());
            (None, None)
        });
    }

    #[test]
    fn later_vanilla_refs_replace_earlier_ones() {
        let later = refs_with_animation();
        super::set_vanilla_refs(assets::VanillaEntityRefs::new());
        super::set_vanilla_refs(later.clone());
        assert_eq!(super::vanilla_refs().as_deref(), Some(&later));
    }

    const FIXTURE_MATERIAL_PATH: &str = "materials/fixture.material";

    fn entity_stack() -> std::sync::Arc<resource_pack::ValidatedPackStack> {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(1, 1, image::Rgba([37, 59, 83, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        super::super::pack_reload_tests::stack(&[
            ("entity/fixture.json", br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"fixture:actor","geometry":{"default":"geometry.fixture"},"materials":{"default":"fixture_alpha"},"textures":{"default":"textures/entity/fixture"},"render_controllers":["controller.render.fixture"]}}}"#),
            (FIXTURE_MATERIAL_PATH, br#"{"materials":{"version":"1.0.0","fixture_alpha:entity_alphatest":{}}}"#),
            ("models/entity/fixture.json", br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.fixture","texture_width":1,"texture_height":1},"bones":[{"name":"root","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]}]}"#),
            ("render_controllers/fixture.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.fixture":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#),
            ("textures/entity/fixture.png", png.get_ref()),
        ])
    }

    // A relaunch (a fresh in-memory cache) reads the pack and its reads back from disk; a
    // damaged entry recompiles.
    #[test]
    fn a_disk_hit_skips_compilation_and_a_corrupt_entry_recompiles() {
        let dir = tempfile::tempdir().unwrap();
        let disk = client_session::compile_cache::CompileCache::new(dir.path().into(), 1 << 30);
        let stack = entity_stack();
        let compiles = std::cell::Cell::new(0);
        let launch = || {
            let view = resource_pack::LayeredPackView::tracked(stack.clone());
            let pack = super::EntityCache::default().get_or_compile(
                inputs(None),
                &view,
                Some(&disk),
                |view, vanilla, vanilla_pack_dir, encode| {
                    compiles.set(compiles.get() + 1);
                    super::compile_encoded(view, vanilla, vanilla_pack_dir, encode)
                },
            );
            let pack = pack.expect("the fixture defines an entity");
            let reads = view.dependencies().unwrap().snapshot();
            // Catalog encoding excludes metadata about whether a carrier was decoded.
            (
                pack.assets.encode().expect("the catalog is encodable"),
                pack.textures.clone(),
                pack.bindings.clone(),
                reads,
            )
        };
        let compiled = launch();
        assert!(
            compiled
                .3
                .contains(&resource_pack::PackDependency::Directory(
                    "materials/".to_owned()
                ))
        );
        assert!(compiled.3.iter().any(|dependency| matches!(
            dependency,
            resource_pack::PackDependency::File { path, .. } if path == FIXTURE_MATERIAL_PATH
        )));
        assert!(
            launch() == compiled,
            "a disk hit preserves the catalog, artwork and tracked inputs"
        );
        assert_eq!(compiles.get(), 1, "the second launch was a disk hit");
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            let mut bytes = std::fs::read(&path).unwrap();
            let middle = bytes.len() / 2;
            bytes[middle] ^= 0xff;
            std::fs::write(path, bytes).unwrap();
        }
        assert!(
            launch() == compiled,
            "recompilation preserves the catalog, artwork and tracked inputs"
        );
        assert_eq!(compiles.get(), 2, "a corrupt entry is a miss");
    }

    #[test]
    fn identifier_is_read_from_the_client_entity_description() {
        let json = br#"{"minecraft:client_entity":{"description":{"identifier":"a:b"}}}"#;
        assert_eq!(entity_identifier(json).as_deref(), Some("a:b"));
        assert_eq!(entity_identifier(b"{}"), None);
    }
}

#[cfg(all(test, feature = "reports"))]
mod pack_report;

#[cfg(all(test, feature = "reports"))]
mod equipment_report;

#[cfg(all(test, feature = "reports"))]
mod render_report;

#[cfg(all(test, unix, feature = "reports"))]
mod lobby_bench;

#[cfg(all(test, feature = "reports"))]
mod scene_report;

#[cfg(all(test, feature = "reports"))]
mod projectile_report;

#[cfg(all(test, feature = "reports"))]
mod mob_motion_report;
