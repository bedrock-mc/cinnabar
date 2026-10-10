//! Neutral actor artwork for a server pack's entities, compiled in memory.

use super::*;
use crate::entity::{EntityPackSkips, compile_entity_pack};

/// A pack's entity catalog with the artwork of its eligible rigs. Indices are
/// local to `entities`, which is its own index space beside the vanilla catalog.
#[derive(Debug)]
pub struct ActorPackCompilation {
    pub entities: CompiledEntityAssets,
    pub textures: Vec<ActorTexture>,
    pub bindings: Vec<ActorArtworkBinding>,
    pub skipped: EntityPackSkips,
    /// Rigs left without artwork, with the reason each was rejected.
    pub fallbacks: Vec<ActorFallback>,
    /// Attachable bindings of the pack's held and worn items, and their decoded rasters.
    pub equipment_bindings: Vec<assets::EquipmentBinding>,
    pub equipment_textures: Vec<assets::EquipmentTexture>,
    /// Digest of the pack sources, stable for identical packs.
    pub identity: [u8; 32],
}

/// Compiles `(pack-relative path, bytes)` files; `Ok(None)` when the pack has no
/// usable entity source. Bad files are skipped and counted in `skipped`.
pub fn compile_actor_pack(
    files: Vec<(Box<str>, Vec<u8>)>,
) -> Result<Option<ActorPackCompilation>, AssetError> {
    compile_actor_pack_unless(files, &|| false).expect("an uncancellable compile completes")
}

/// Why a cancellable compile ended early.
enum Stop {
    Cancelled,
    Failed(AssetError),
}

impl From<AssetError> for Stop {
    fn from(error: AssetError) -> Self {
        Self::Failed(error)
    }
}

/// [`compile_actor_pack`], abandoned between its stages once `cancelled` holds; `None` then.
pub fn compile_actor_pack_unless(
    files: Vec<(Box<str>, Vec<u8>)>,
    cancelled: &dyn Fn() -> bool,
) -> Option<Result<Option<ActorPackCompilation>, AssetError>> {
    match compile_stages(files, cancelled) {
        Ok(compiled) => Some(Ok(compiled)),
        Err(Stop::Failed(error)) => Some(Err(error)),
        Err(Stop::Cancelled) => None,
    }
}

/// The stages of [`compile_actor_pack`], checking `cancelled` before each one after the first.
fn compile_stages(
    files: Vec<(Box<str>, Vec<u8>)>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<ActorPackCompilation>, Stop> {
    let check = || {
        if cancelled() {
            Err(Stop::Cancelled)
        } else {
            Ok(())
        }
    };
    let Some(pack) = compile_entity_pack(files)? else {
        return Ok(None);
    };
    check()?;
    let runtime = assets::RuntimeEntityAssets::from_compiled(pack.assets.clone())?;
    check()?;
    let mut read = |index: u32| -> Result<Vec<u8>, AssetError> {
        let source = &pack.assets.sources[index as usize];
        pack.payloads
            .get(source.path.as_ref())
            .map(|bytes| bytes.to_vec())
            .ok_or_else(|| invalid("pack entity source payload is absent"))
    };
    let build = build_artwork(&pack.assets, &runtime, &mut read, true)?;
    check()?;
    let equipment_textures = crate::entity::compile_equipment_textures_for_assets_with(
        &pack.assets,
        &pack.equipment_bindings,
        &mut |source| {
            pack.payloads
                .get(source.path.as_ref())
                .map(|bytes| bytes.to_vec())
                .ok_or_else(|| invalid("pack equipment raster is absent"))
        },
    )?;
    let mut identity = Sha256::new();
    for source in pack.assets.sources.iter() {
        identity.update(source.path.as_bytes());
        identity.update(source.source_sha256);
    }
    Ok(Some(ActorPackCompilation {
        equipment_bindings: pack.equipment_bindings.to_vec(),
        equipment_textures,
        identity: identity.finalize().into(),
        entities: pack.assets,
        textures: build.textures,
        bindings: build.bindings,
        skipped: pack.skipped,
        fallbacks: build.fallbacks,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pack_without_entity_sources_compiles_to_nothing() {
        assert!(
            compile_actor_pack(vec![("textures/blocks/a.png".into(), vec![1])])
                .unwrap()
                .is_none()
        );
    }

    // A tall flipbook (as display packs animate with `uv_anim`) keeps its artwork whole.
    #[test]
    fn a_tall_flipbook_texture_keeps_its_artwork() {
        let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"test:logo","materials":{"default":"entity_alphatest"},"textures":{"default":"textures/entity/logo"},"geometry":{"default":"geometry.logo"},"render_controllers":["controller.render.logo"]}}}"#;
        let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.logo","texture_width":8,"texture_height":8},"bones":[{"name":"root","pivot":[0,0,0],"cubes":[{"origin":[0,0,0],"size":[8,8,0],"uv":[0,0]}]}]}]}"#;
        let controller = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.logo":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#;
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(8, 1024, image::Rgba([1, 2, 3, 255]))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let compiled = compile_actor_pack(vec![
            ("entity/logo.json".into(), entity.to_vec()),
            ("models/entity/logo.geo.json".into(), geometry.to_vec()),
            ("render_controllers/logo.json".into(), controller.to_vec()),
            ("textures/entity/logo.png".into(), png),
        ])
        .unwrap()
        .expect("entity compiles");
        assert_eq!(compiled.bindings.len(), 1, "{:?}", compiled.fallbacks);
        assert_eq!(compiled.textures[0].height, 1024);
    }

    // A cancelled compile stops at its next stage instead of decoding artwork; uncancelled, the
    // same files compile as the plain entry point compiles them.
    #[test]
    fn a_cancelled_compile_stops_between_stages() {
        let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"test:cube","materials":{"default":"entity_alphatest"},"textures":{"default":"textures/entity/cube"},"geometry":{"default":"geometry.cube"},"render_controllers":["controller.render.cube"]}}}"#;
        let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.cube","texture_width":1,"texture_height":1},"bones":[{"name":"root","pivot":[0,0,0],"cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]}]}"#;
        let controller = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.cube":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#;
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 255]))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let files = || {
            vec![
                ("entity/cube.json".into(), entity.to_vec()),
                ("models/entity/cube.geo.json".into(), geometry.to_vec()),
                ("render_controllers/cube.json".into(), controller.to_vec()),
                ("textures/entity/cube.png".into(), png.clone()),
            ]
        };
        let checks = std::cell::Cell::new(0);
        let stopped = compile_actor_pack_unless(files(), &|| {
            checks.set(checks.get() + 1);
            true
        });
        assert!(stopped.is_none());
        assert_eq!(checks.get(), 1);
        let completed = compile_actor_pack_unless(files(), &|| false)
            .unwrap()
            .unwrap()
            .unwrap();
        let plain = compile_actor_pack(files()).unwrap().unwrap();
        assert_eq!(completed.identity, plain.identity);
        assert_eq!(completed.bindings.len(), plain.bindings.len());
    }

    // A weighted object binding two animations (as camel's controller does) compiles both.
    #[test]
    fn a_weighted_object_with_several_animations_compiles_every_binding() {
        let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"test:camel","materials":{"default":"entity"},"textures":{"default":"textures/entity/camel"},"geometry":{"default":"geometry.camel"},"animations":{"walk":"animation.camel.walk","baby_walk":"animation.camel.baby_walk","move":"controller.animation.camel.move"},"scripts":{"animate":["move"]},"render_controllers":["controller.render.camel"]}}}"#;
        let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.camel","texture_width":16,"texture_height":16},"bones":[{"name":"body","pivot":[0,0,0],"cubes":[{"origin":[0,0,0],"size":[4,4,4],"uv":[0,0]}]}]}]}"#;
        let animations = br#"{"format_version":"1.8.0","animations":{"animation.camel.walk":{"loop":true,"bones":{"body":{"rotation":[10,0,0]}}},"animation.camel.baby_walk":{"loop":true,"bones":{"body":{"rotation":[20,0,0]}}}}}"#;
        let controllers = br#"{"format_version":"1.10.0","animation_controllers":{"controller.animation.camel.move":{"initial_state":"default","states":{"default":{"animations":[{"walk":"!query.is_baby","baby_walk":"query.is_baby"}]}}}}}"#;
        let render = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.camel":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#;
        let compiled = compile_actor_pack(vec![
            ("entity/camel.json".into(), entity.to_vec()),
            ("models/entity/camel.geo.json".into(), geometry.to_vec()),
            ("animations/camel.json".into(), animations.to_vec()),
            (
                "animation_controllers/camel.json".into(),
                controllers.to_vec(),
            ),
            ("render_controllers/camel.json".into(), render.to_vec()),
        ])
        .unwrap()
        .expect("entity compiles");
        assert_eq!(compiled.entities.controllers.len(), 1);
        assert_eq!(compiled.entities.controller_animations.len(), 2);
    }

    // A pack shipping only attachables (custom armor) still yields its equipment bindings.
    #[test]
    fn an_attachable_only_pack_compiles_its_equipment() {
        let attachable = br#"{"format_version":"1.10.0","minecraft:attachable":{"description":{"identifier":"test:crown","materials":{"default":"armor"},"textures":{"default":"textures/models/crown"},"geometry":{"default":"geometry.test.crown"},"render_controllers":["controller.render.armor"]}}}"#;
        let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.test.crown","texture_width":16,"texture_height":16},"bones":[{"name":"head","pivot":[0,24,0]}]}]}"#;
        let compiled = compile_actor_pack(vec![
            ("attachables/crown.json".into(), attachable.to_vec()),
            ("models/entity/crown.geo.json".into(), geometry.to_vec()),
        ])
        .unwrap()
        .expect("attachables compile");
        assert_eq!(compiled.equipment_bindings.len(), 1);
    }
}
