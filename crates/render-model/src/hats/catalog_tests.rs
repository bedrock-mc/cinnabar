//! Every shipped catalog export admits through production validation with its declared metadata.
use super::*;
use cosmetics::Catalog;

/// Runtime assets for one catalog item, keyed by its namespaced identifier.
struct Shipped {
    id: &'static str,
    geometry: &'static [u8],
    texture: &'static [u8],
    animation: &'static [u8],
    emitters: usize,
}

macro_rules! shipped {
    ($name:literal, $emitters:expr) => {
        Shipped {
            id: concat!("cinnabar:", $name),
            geometry: include_bytes!(concat!($name, ".geo.json")),
            texture: include_bytes!(concat!($name, ".png")),
            animation: include_bytes!(concat!($name, ".animation.json")),
            emitters: $emitters,
        }
    };
}

/// The second catalog wave, with the number of particle emitters each one declares.
fn wave() -> [Shipped; 10] {
    [
        shipped!("fox_tail", 0),
        shipped!("pixel_shades", 0),
        shipped!("bee_buddy", 1),
        shipped!("morpho_wings", 0),
        shipped!("knit_scarf", 0),
        shipped!("kitsune_mask", 0),
        shipped!("studio_headphones", 2),
        shipped!("halo", 2),
        shipped!("ember_jetpack", 4),
        shipped!("amethyst_orbit", 7),
    ]
}

#[test]
fn every_new_catalog_item_admits_with_its_digests_slot_glow_and_emitters() {
    let catalog: Catalog = serde_json::from_str(include_str!("fixture/catalog.json")).unwrap();
    for shipped in wave() {
        let descriptor = catalog
            .items
            .iter()
            .find(|item| item.id == shipped.id)
            .unwrap_or_else(|| panic!("{} is missing from the fixture catalog", shipped.id))
            .clone();
        let slot = descriptor.slot.clone();
        let emissive = descriptor.render_flags.emissive;
        let item = admit(
            descriptor,
            shipped.geometry,
            shipped.texture,
            shipped.animation,
        )
        .unwrap_or_else(|| panic!("{} must admit with the catalog digests", shipped.id));
        assert!(cosmetics::known_slot(&slot), "{}", shipped.id);
        let model = rig(item);
        assert_eq!(model.emissive(), emissive, "{}", shipped.id);
        assert_eq!(model.emitters().len(), shipped.emitters, "{}", shipped.id);
        assert!(!model.rest_vertices.is_empty(), "{}", shipped.id);
        assert!(
            thumbnail::render(item, 64, 64).is_some_and(|image| image
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .any(|texel| texel[3] > 0)),
            "{} needs visible gallery artwork",
            shipped.id
        );
    }
}

#[test]
fn every_slot_has_a_catalog_item_and_glow_is_limited_to_the_halo_and_aura() {
    let catalog: Catalog = serde_json::from_str(include_str!("fixture/catalog.json")).unwrap();
    for slot in cosmetics::known_slots() {
        assert!(
            catalog.items.iter().any(|item| item.slot == *slot),
            "no catalog item fills {slot}"
        );
    }
    let glowing: Vec<_> = catalog
        .items
        .iter()
        .filter(|item| item.render_flags.emissive)
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(glowing, ["cinnabar:halo", "cinnabar:amethyst_orbit"]);
}

#[test]
fn spinning_items_turn_smoothly_through_their_wrap_keys() {
    let catalog: Catalog = serde_json::from_str(include_str!("fixture/catalog.json")).unwrap();
    let attachment = RenderBoneTransform {
        rotation: Quat::IDENTITY.to_array(),
        translation_scale: [0.0, 1.5, 0.0, 1.0],
        axis_scale: crate::UNIT_AXIS_SCALE,
    };
    for shipped in wave() {
        let descriptor = catalog
            .items
            .iter()
            .find(|item| item.id == shipped.id)
            .unwrap()
            .clone();
        let item = admit(
            descriptor,
            shipped.geometry,
            shipped.texture,
            shipped.animation,
        )
        .unwrap();
        let model = rig(item);
        let Some(length) = model.playback.as_ref().map(|clip| clip.length) else {
            continue;
        };
        let step = 0.000_5;
        let mut previous = model.pose(attachment, Motion::default());
        for index in 1..=(length / step) as usize {
            let seconds = index as f32 * step;
            let pose = model.pose(
                attachment,
                Motion {
                    seconds,
                    ..Default::default()
                },
            );
            for (bone, (before, after)) in previous.iter().zip(pose.iter()).enumerate() {
                let turn = Quat::from_array(before.rotation)
                    .angle_between(Quat::from_array(after.rotation));
                assert!(
                    turn < 3f32.to_radians(),
                    "{} bone {bone} turns {}° at {seconds}s",
                    shipped.id,
                    turn.to_degrees()
                );
            }
            previous = pose;
        }
    }
}
