//! Original redistributable catalog fixtures, excluded from production builds.
use cosmetics::{Catalog, Item};
use std::sync::OnceLock;

mod cloth_mesh;

/// Admits a small original fabric mesh shared by cloth and perspective regression tests.
pub fn cloth() -> Item {
    static ITEM: OnceLock<Item> = OnceLock::new();
    *ITEM.get_or_init(|| cloth_mesh::admit(2, 2))
}

/// Admits a finely subdivided synthetic fabric for jump, landing and drape regressions.
pub fn dense_cloth() -> Item {
    static ITEM: OnceLock<Item> = OnceLock::new();
    *ITEM.get_or_init(|| cloth_mesh::admit(12, 24))
}

/// Admits the original bunny-ear catalog item through the production asset validator.
pub fn bunny() -> Item {
    static ITEM: OnceLock<Item> = OnceLock::new();
    *ITEM.get_or_init(|| {
        let catalog: Catalog = serde_json::from_str(include_str!("fixture/catalog.json")).unwrap();
        super::admit(
            catalog.items[0].clone(),
            include_bytes!("bunny_ears.geo.json"),
            include_bytes!("bunny_ears.png"),
            include_bytes!("fixture/animation.json"),
        )
        .unwrap()
    })
}

/// Admits the animated bunny fixture under a caller-owned ID to isolate its rig caches.
pub fn named_bunny(id: &str) -> Item {
    let mut descriptor = bunny().descriptor().clone();
    descriptor.id = id.into();
    super::admit(
        descriptor,
        include_bytes!("bunny_ears.geo.json"),
        include_bytes!("bunny_ears.png"),
        include_bytes!("fixture/animation.json"),
    )
    .unwrap()
}

/// Admits a paid synthetic item without granting session ownership.
pub fn locked_bunny() -> Item {
    static ITEM: OnceLock<Item> = OnceLock::new();
    *ITEM.get_or_init(|| {
        let mut descriptor = bunny().descriptor().clone();
        descriptor.id = "fixture:locked_bunny".into();
        descriptor.display_name = "Locked bunny ears".into();
        descriptor.free = false;
        super::admit(
            descriptor,
            include_bytes!("bunny_ears.geo.json"),
            include_bytes!("bunny_ears.png"),
            include_bytes!("fixture/animation.json"),
        )
        .unwrap()
    })
}

/// Admits a second slot using the same original surface with no movement animation.
pub fn rigid() -> Item {
    static ITEM: OnceLock<Item> = OnceLock::new();
    *ITEM.get_or_init(|| {
        let mut item = bunny().descriptor().clone();
        item.id = "fixture:rigid".into();
        item.slot = "back".into();
        item.display_name = "Rigid fixture".into();
        item.assets.animation = None;
        super::admit(
            item,
            include_bytes!("bunny_ears.geo.json"),
            include_bytes!("bunny_ears.png"),
            &[],
        )
        .unwrap()
    })
}

/// Admits the existing original crown through the same catalog path as downloaded items.
pub fn crown() -> Item {
    static ITEM: OnceLock<Item> = OnceLock::new();
    *ITEM.get_or_init(|| {
        let catalog: Catalog = serde_json::from_str(include_str!("fixture/catalog.json")).unwrap();
        let item = catalog
            .items
            .into_iter()
            .find(|item| item.id == "cinnabar:crown")
            .unwrap();
        super::admit(
            item,
            include_bytes!("crown.geo.json"),
            include_bytes!("crown.png"),
            b"{}\n",
        )
        .unwrap()
    })
}

/// Admits a large gallery of independently named items using the original ear surface.
pub fn gallery() -> &'static [Item] {
    static ITEMS: OnceLock<Vec<Item>> = OnceLock::new();
    ITEMS.get_or_init(|| {
        (0..40)
            .map(|index| named_bunny(&format!("fixture:gallery-{index}")))
            .collect()
    })
}

/// Admits original rigid devil horns through the downloaded-item validation path.
pub fn devil_horns() -> Item {
    static ITEM: OnceLock<Item> = OnceLock::new();
    *ITEM.get_or_init(|| {
        let catalog: Catalog = serde_json::from_str(include_str!("fixture/catalog.json")).unwrap();
        let item = catalog
            .items
            .into_iter()
            .find(|item| item.id == "cinnabar:devil_horns")
            .unwrap();
        super::admit(
            item,
            include_bytes!("devil_horns.geo.json"),
            include_bytes!("devil_horns.png"),
            b"{}\n",
        )
        .unwrap()
    })
}

/// Admits the original animated back attachment through downloaded-asset validation.
pub fn dragon_wings() -> Item {
    static ITEM: OnceLock<Item> = OnceLock::new();
    *ITEM.get_or_init(|| {
        let catalog: Catalog = serde_json::from_str(include_str!("fixture/catalog.json")).unwrap();
        super::admit(
            catalog
                .items
                .into_iter()
                .find(|item| item.id == "cinnabar:dragon_wings")
                .unwrap(),
            include_bytes!("dragon_wings.geo.json"),
            include_bytes!("dragon_wings.png"),
            include_bytes!("dragon_wings.animation.json"),
        )
        .unwrap()
    })
}

/// Two original geometry revisions sharing one item ID and unchanged texture bytes.
pub fn wing_revisions() -> [Item; 2] {
    static ITEMS: OnceLock<[Item; 2]> = OnceLock::new();
    *ITEMS.get_or_init(|| {
        let mut descriptor = dragon_wings().descriptor().clone();
        descriptor.id = "fixture:preview-revision-wings".into();
        let texture = include_bytes!("dragon_wings.png");
        let animation = include_bytes!("dragon_wings.animation.json");
        let bytes = include_bytes!("dragon_wings.geo.json");
        let first = super::admit(descriptor.clone(), bytes, texture, animation).unwrap();
        let mut geometry: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        for bone in geometry["minecraft:geometry"][0]["bones"]
            .as_array_mut()
            .unwrap()
        {
            if let Some(positions) = bone
                .get_mut("poly_mesh")
                .and_then(|mesh| mesh.get_mut("positions"))
                .and_then(serde_json::Value::as_array_mut)
            {
                for position in positions {
                    position[1] = serde_json::json!(position[1].as_f64().unwrap() + 4.0);
                }
            }
        }
        let bytes = serde_json::to_vec(&geometry).unwrap();
        descriptor.assets.geometry = animation_asset(&bytes);
        let second = super::admit(descriptor, &bytes, texture, animation).unwrap();
        [first, second]
    })
}

/// Admits the original shoulder parrot in its independent pet slot.
pub fn shoulder_parrot() -> Item {
    static ITEM: OnceLock<Item> = OnceLock::new();
    *ITEM.get_or_init(|| {
        let catalog: Catalog = serde_json::from_str(include_str!("fixture/catalog.json")).unwrap();
        super::admit(
            catalog
                .items
                .into_iter()
                .find(|item| item.id == "cinnabar:shoulder_parrot")
                .unwrap(),
            include_bytes!("shoulder_parrot.geo.json"),
            include_bytes!("shoulder_parrot.png"),
            include_bytes!("shoulder_parrot.animation.json"),
        )
        .unwrap()
    })
}

/// A small original looping particle effect for cosmetic emitter fixtures.
pub const EMITTER_EFFECT: &str = r##"{"format_version":"1.10.0","particle_effect":{"description":{"identifier":"fixture:spark","basic_render_parameters":{"material":"particles_alpha","texture":"textures/particle/particles"}},"components":{"minecraft:emitter_rate_steady":{"spawn_rate":4,"max_particles":8},"minecraft:emitter_lifetime_looping":{"active_time":1},"minecraft:particle_lifetime_expression":{"max_lifetime":1},"minecraft:particle_appearance_billboard":{"size":[0.05,0.05],"facing_camera_mode":"lookat_xyz"}}}}"##;

/// Builds a catalog animation asset whose digest matches `bytes`.
pub fn animation_asset(bytes: &[u8]) -> cosmetics::Asset {
    use sha2::Digest;
    cosmetics::Asset {
        url: "fixture".into(),
        sha256: format!("{:x}", sha2::Sha256::digest(bytes)),
        bytes: bytes.len(),
    }
}

/// Admits an emissive crown with one particle emitter ten pixels above its head pivot.
pub fn glowing_crown() -> Item {
    static ITEM: OnceLock<Item> = OnceLock::new();
    *ITEM.get_or_init(|| {
        let animation = format!(
            r#"{{"particles":[{{"bone":"head","offset":[0,10,0],"effect":{EMITTER_EFFECT}}}]}}"#
        );
        let mut item = crown().descriptor().clone();
        item.id = "fixture:glowing_crown".into();
        item.slot = "aura".into();
        item.render_flags.emissive = true;
        item.assets.animation = Some(animation_asset(animation.as_bytes()));
        super::admit(
            item,
            include_bytes!("crown.geo.json"),
            include_bytes!("crown.png"),
            animation.as_bytes(),
        )
        .unwrap()
    })
}

/// Admits the original crown with a looping particle sampling its own first sprite region.
pub fn sprite_crown() -> Item {
    static ITEM: OnceLock<Item> = OnceLock::new();
    *ITEM.get_or_init(|| {
        let mut effect: serde_json::Value = serde_json::from_str(EMITTER_EFFECT).unwrap();
        effect["particle_effect"]["description"]["basic_render_parameters"]["texture"] = super::COSMETIC_PARTICLE_TEXTURE.into();
        let animation = serde_json::to_vec(&serde_json::json!({"particles": [{"bone": "head", "offset": [0, 10, 0], "effect": effect, "sprites": [0, 0]}]})).unwrap();
        let mut item = glowing_crown().descriptor().clone();
        item.id = "fixture:sprite_crown".into();
        item.assets.animation = Some(animation_asset(&animation));
        super::admit(item, include_bytes!("crown.geo.json"), include_bytes!("crown.png"), &animation).unwrap()
    })
}

/// Admits an original moving spark fountain for readable paper-doll captures.
pub fn sparkling_crown() -> Item {
    static ITEM: OnceLock<Item> = OnceLock::new();
    *ITEM.get_or_init(|| {
        let mut effect: serde_json::Value = serde_json::from_str(EMITTER_EFFECT).unwrap();
        let components = &mut effect["particle_effect"]["components"];
        components["minecraft:emitter_rate_steady"] = serde_json::json!({"spawn_rate": 8, "max_particles": 16});
        components["minecraft:emitter_shape_point"] = serde_json::json!({"offset": ["math.random(-0.35, 0.35)", 0, "math.random(-0.35, 0.35)"]});
        components["minecraft:particle_initial_speed"] = serde_json::json!([0, 0.25, 0]);
        components["minecraft:particle_motion_dynamic"] = serde_json::json!({});
        components["minecraft:particle_appearance_billboard"]["size"] = serde_json::json!([0.025, 0.025]);
        let animation = serde_json::to_vec(&serde_json::json!({"particles": [{"bone": "head", "offset": [0, 10, 0], "effect": effect}]})).unwrap();
        let mut item = glowing_crown().descriptor().clone();
        item.id = "fixture:sparkling_crown".into();
        item.assets.animation = Some(animation_asset(&animation));
        super::admit(item, include_bytes!("crown.geo.json"), include_bytes!("crown.png"), &animation).unwrap()
    })
}

/// Admits a synthetic ground controller using existing fixture geometry and four simple clips.
pub fn ground_companion() -> Item {
    static ITEM: OnceLock<Item> = OnceLock::new();
    *ITEM.get_or_init(|| {
        let mut descriptor = crown().descriptor().clone();
        descriptor.id = "fixture:independent_companion".into();
        descriptor.slot = "pet".into();
        let animations: serde_json::Map<_,_> = ["idle", "sit", "trot", "gallop"].into_iter().map(|name| {
            (name.into(),serde_json::json!({"loop":true,"animation_length":1.0,"bones":{"head":{"rotation":{"0":[0,0,0],"1":[0,0,0]}}}}))
        }).collect();
        let animation = serde_json::to_vec(&serde_json::json!({"animations":animations,"companion":{"idle":"idle","sit":"sit","trot":"trot","gallop":"gallop","trot_stride":1.6,"gallop_stride":2.6}})).unwrap();
        descriptor.assets.animation = Some(animation_asset(&animation));
        super::admit(descriptor, include_bytes!("crown.geo.json"), include_bytes!("crown.png"), &animation).unwrap()
    })
}

/// Admits a synthetic aura with one instant landing effect for contact-routing regressions.
pub fn ground_effect() -> Item {
    static ITEM: OnceLock<Item> = OnceLock::new();
    *ITEM.get_or_init(|| {
        let mut descriptor = crown().descriptor().clone();
        descriptor.id = "fixture:ground_contact".into();
        descriptor.slot = "aura".into();
        let mut effect: serde_json::Value = serde_json::from_str(EMITTER_EFFECT).unwrap();
        let parts = effect["particle_effect"]["components"]
            .as_object_mut()
            .unwrap();
        parts.remove("minecraft:emitter_rate_steady");
        parts.remove("minecraft:emitter_lifetime_looping");
        parts.insert(
            "minecraft:emitter_rate_instant".into(),
            serde_json::json!({"num_particles": 1}),
        );
        parts.insert(
            "minecraft:emitter_lifetime_once".into(),
            serde_json::json!({"active_time": 0.1}),
        );
        let animation = serde_json::to_vec(&serde_json::json!({"particles": [
            {"bone": "head", "offset": [0, 0, 0], "trigger": "landing", "effect": effect}
        ]}))
        .unwrap();
        descriptor.assets.animation = Some(animation_asset(&animation));
        super::admit(
            descriptor,
            include_bytes!("crown.geo.json"),
            include_bytes!("crown.png"),
            &animation,
        )
        .unwrap()
    })
}
