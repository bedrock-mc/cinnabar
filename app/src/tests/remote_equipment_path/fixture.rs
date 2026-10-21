//! Generated artwork and models for a remote equipment session.

use super::*;
use std::io::{Cursor, Write};

pub(super) type Files = Vec<(Box<str>, Vec<u8>)>;
pub(super) const SPRITE: &str = "fixture:sprite";
pub(super) const SPRITE_NETWORK_ID: i16 = 100;
pub(super) const SPRITE_COLOR: [u8; 4] = [255, 0, 255, 255];
pub(super) const CAPE_COLOR: [u8; 4] = [37, 73, 109, 255];
pub(super) const SKIN_COLOR: [u8; 4] = [91, 137, 181, 255];

/// A fixture piece owns its item, wearable slot, pose bone and original artwork.
pub(super) struct Piece {
    pub name: &'static str,
    pub slot: Option<&'static str>,
    pub bone: &'static str,
    pub color: [u8; 4],
}

pub(super) const PIECES: [Piece; 5] = [
    Piece {
        name: "helmet",
        slot: Some("slot.armor.head"),
        bone: "head",
        color: [255, 0, 0, 255],
    },
    Piece {
        name: "chestplate",
        slot: Some("slot.armor.chest"),
        bone: "body",
        color: [0, 255, 0, 255],
    },
    Piece {
        name: "leggings",
        slot: Some("slot.armor.legs"),
        bone: "rightLeg",
        color: [0, 0, 255, 255],
    },
    Piece {
        name: "boots",
        slot: Some("slot.armor.feet"),
        bone: "leftLeg",
        color: [255, 255, 0, 255],
    },
    Piece {
        name: "attachable",
        slot: None,
        bone: "leftItem",
        color: [0, 255, 255, 255],
    },
];

impl Piece {
    /// Uses the fixture index as the server's compact network item identity.
    pub fn network_id(&self) -> i16 {
        i16::try_from(
            PIECES
                .iter()
                .position(|piece| piece.name == self.name)
                .unwrap()
                + 1,
        )
        .unwrap()
    }

    /// Uses admitted iron-armour identities with original models and artwork.
    pub fn identifier(&self) -> String {
        use assets::item::armour::ItemArmourSlot;
        let slot = match self.slot {
            Some("slot.armor.head") => ItemArmourSlot::Head,
            Some("slot.armor.chest") => ItemArmourSlot::Chest,
            Some("slot.armor.legs") => ItemArmourSlot::Legs,
            Some("slot.armor.feet") => ItemArmourSlot::Feet,
            Some(slot) => panic!("unknown fixture wearable slot: {slot}"),
            None => return format!("fixture:{}", self.name),
        };
        assets::facts::ItemFacts::builtin()
            .unwrap()
            .payload()
            .records
            .iter()
            .find(|record| {
                record.fact.armour_slot == Some(slot)
                    && record.fact.tier == Some(assets::facts::ItemTier::Iron)
            })
            .expect("pinned armour facts supply each fixture slot")
            .identifier
            .to_string()
    }

    /// Reloads rotate the fixture palette while keeping each piece distinguishable.
    pub fn color(&self, switched: bool) -> [u8; 4] {
        if switched {
            PIECES[self.network_id() as usize % PIECES.len()].color
        } else {
            self.color
        }
    }
}

/// Encodes an original solid image; no installed pack is read.
fn png(color: [u8; 4], side: u32) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(side, side, image::Rgba(color))
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

/// Writes one JSON fixture document into its pack-relative path.
fn json(files: &mut Files, path: impl Into<Box<str>>, value: serde_json::Value) {
    files.push((path.into(), serde_json::to_vec(&value).unwrap()));
}

/// A complete original body with named attachment bones, plus the cape rig at startup.
pub(super) fn player_files(startup: bool) -> Files {
    let mut files = Vec::new();
    let bones = [
        ("body", None, [-4, 12, -2], [8, 12, 4]),
        ("head", Some("body"), [-4, 24, -4], [8, 8, 8]),
        ("rightArm", Some("body"), [-8, 12, -2], [4, 12, 4]),
        ("leftArm", Some("body"), [4, 12, -2], [4, 12, 4]),
        ("rightLeg", Some("body"), [-4, 0, -2], [4, 12, 4]),
        ("leftLeg", Some("body"), [0, 0, -2], [4, 12, 4]),
        ("rightItem", Some("rightArm"), [-6, 12, 0], [0, 0, 0]),
        ("leftItem", Some("leftArm"), [6, 12, 0], [0, 0, 0]),
    ]
    .into_iter()
    .map(|(name, parent, origin, size)| {
        let mut bone = serde_json::json!({"name": name, "pivot": [0, 0, 0]});
        if let Some(parent) = parent {
            bone["parent"] = parent.into();
        }
        if size != [0; 3] {
            bone["cubes"] = serde_json::json!([{"origin": origin, "size": size, "uv": [0, 0]}]);
        }
        bone
    })
    .collect::<Vec<_>>();
    let mut models = vec![serde_json::json!({
        "description": {"identifier": "geometry.fixture.player", "texture_width": 64, "texture_height": 64},
        "bones": bones
    })];
    if startup {
        models.push(serde_json::json!({
            "description": {"identifier": assets::CAPE_GEOMETRY_IDENTIFIER, "texture_width": 64, "texture_height": 32},
            "bones": [{"name": "body", "pivot": [0, 24, 0]},
                {"name": "cape", "parent": "body", "pivot": [0, 24, 3],
                "cubes": [{"origin": [-5, 8, 3], "size": [10, 16, 1], "uv": [0, 0]}]}]
        }));
    }
    json(
        &mut files,
        "models/entity/player.json",
        serde_json::json!({"format_version": "1.12.0", "minecraft:geometry": models}),
    );
    json(
        &mut files,
        "entity/player.json",
        serde_json::json!({
            "format_version": "1.10.0", "minecraft:client_entity": {"description": {
                "identifier": assets::facts::VanillaEntity::Player.identifier(),
                "materials": {"default": "entity_alphatest"}, "textures": {"default": "textures/entity/fixture_player"},
                "geometry": {"default": "geometry.fixture.player"}, "render_controllers": ["controller.render.fixture"]
            }}
        }),
    );
    json(
        &mut files,
        "render_controllers/fixture.json",
        serde_json::json!({
            "format_version": "1.8.0", "render_controllers": {"controller.render.fixture": {
                "geometry": "Geometry.default", "materials": [{"*": "Material.default"}], "textures": ["Texture.default"]
            }}
        }),
    );
    files.push((
        "textures/entity/fixture_player.png".into(),
        png([173, 127, 83, 255], 64),
    ));
    files
}

/// Worn models have different cube counts, so wrong geometry also fails the draw oracle.
pub(super) fn equipment_files(switched: bool, models: bool) -> Files {
    let mut files = Vec::new();
    for (index, piece) in PIECES.iter().enumerate() {
        let texture = format!("textures/equipment/{}", piece.name);
        files.push((
            format!("{texture}.png").into(),
            png(piece.color(switched), 16),
        ));
        if !models {
            continue;
        }
        let geometry = format!("geometry.fixture.{}", piece.name);
        json(
            &mut files,
            format!("attachables/{}.json", piece.name),
            serde_json::json!({
                "format_version": "1.10.0", "minecraft:attachable": {"description": {
                    "identifier": piece.identifier(), "materials": {"default": "entity_alphatest"},
                    "textures": {"default": texture}, "geometry": {"default": geometry},
                    "render_controllers": ["controller.render.fixture"]
                }}
            }),
        );
        let cubes = (0..index + 1 + usize::from(switched))
            .map(|cube| {
                serde_json::json!({
                    "origin": [cube as i32 * 3, 0, 0], "size": [2, 2, 2], "uv": [0, 0]
                })
            })
            .collect::<Vec<_>>();
        json(
            &mut files,
            format!("models/entity/{}.json", piece.name),
            serde_json::json!({
                "format_version": "1.12.0", "minecraft:geometry": [{
                    "description": {"identifier": geometry, "texture_width": 16, "texture_height": 16},
                    "bones": [{"name": piece.bone, "pivot": [0, 0, 0], "cubes": cubes}]
                }]
            }),
        );
    }
    files
}

/// Admits an archive before compiling through the production session pack collector.
pub(super) fn session_pack(
    files: &Files,
    equipment: &assets::RuntimeEquipmentCatalog,
) -> Arc<assets::SessionEntityPack> {
    let id = uuid::Uuid::from_u128(10);
    let manifest =
        serde_json::json!({"format_version": 2, "header": {"uuid": id, "version": [1, 0, 0]},
        "modules": [{"type": "resources"}]})
        .to_string();
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in std::iter::once(("manifest.json", manifest.as_bytes())).chain(
        files
            .iter()
            .map(|(path, bytes)| (path.as_ref(), bytes.as_slice())),
    ) {
        archive
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(bytes).unwrap();
    }
    let stack =
        resource_pack::validate_handoff(protocol::ResourcePackHandoff::from_archives(vec![
            protocol::ResourcePackArchive::unencrypted(
                id,
                "1.0.0".into(),
                String::new(),
                archive.finish().unwrap().into_inner(),
            ),
        ]));
    pack_runtime::entity_pack::compile(
        &resource_pack::LayeredPackView::new(stack),
        None,
        None,
        Some(equipment),
    )
    .expect("original fixture pack compiles")
}

/// A real skin and cape with distinguishable bytes; a crowd may vary just the skin identity.
pub(super) fn skin(seed: u64) -> render_api::StandardSkin {
    let side = render_api::CLASSIC_SKIN_SIDE;
    let color = if seed == REMOTE {
        SKIN_COLOR
    } else {
        [seed as u8, 43, 67, 255]
    };
    let mut rgba8 = color.repeat(side * side);
    let end = rgba8.len();
    rgba8[end - 8..].copy_from_slice(&seed.to_le_bytes());
    let (width, height) = render_api::CAPE_DIMENSIONS[0];
    render_api::StandardSkin {
        width: side as u32,
        height: side as u32,
        rgba8: rgba8.into(),
        geometry: None,
        cape: Some(render_api::CapeImage {
            width,
            height,
            rgba8: CAPE_COLOR.repeat((width * height) as usize).into(),
        }),
    }
}
