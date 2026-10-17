//! Original redistributable catalog fixtures, excluded from production builds.
use cosmetics::{Catalog, Item};
use std::sync::OnceLock;

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
            .map(|index| {
                let mut item = bunny().descriptor().clone();
                item.id = format!("fixture:gallery-{index}");
                super::admit(
                    item,
                    include_bytes!("bunny_ears.geo.json"),
                    include_bytes!("bunny_ears.png"),
                    include_bytes!("fixture/animation.json"),
                )
                .unwrap()
            })
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
