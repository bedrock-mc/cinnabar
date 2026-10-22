//! Boats and rafts, which vanilla draws with a native model rather than a render controller.
use super::painting::{encode_document, unused_source_path};
use super::*;
use serde_json::json;

/// Prefix of the generated controllers; each boat entity gets its own.
const CONTROLLER: &str = "controller.render.cinnabar";
/// Texture keys in actor-variant order; `query.variant` indexes this list.
const WOODS: [&str; 11] = [
    "oak", "spruce", "birch", "jungle", "acacia", "darkoak", "mangrove", "bamboo", "cherry",
    "paleoak", "poplar",
];
/// The bamboo variant draws the raft model.
const RAFT_VARIANT: usize = 7;

/// One textured box, in model-part pixels relative to its part.
struct Cuboid {
    origin: [f32; 3],
    size: [f32; 3],
    inflate: f32,
}

/// A native model part: Y-down pixels about the actor's render origin, rotations in degrees.
struct Part {
    name: &'static str,
    uv: [u16; 2],
    position: [f32; 3],
    rotation: [f32; 3],
    cuboids: &'static [Cuboid],
}

const fn cuboid(origin: [f32; 3], size: [f32; 3]) -> Cuboid {
    Cuboid {
        origin,
        size,
        inflate: 0.0,
    }
}

const SIDE: &[Cuboid] = &[cuboid([-14.0, -7.0, -1.0], [28.0, 6.0, 2.0])];
const HULL: [Part; 5] = [
    Part {
        name: "bottom",
        uv: [0, 0],
        position: [0.0, 3.0, 1.0],
        rotation: [90.0, 0.0, 0.0],
        cuboids: &[cuboid([-14.0, -9.0, -3.0], [28.0, 16.0, 3.0])],
    },
    Part {
        name: "back",
        uv: [0, 19],
        position: [-15.0, 4.0, 4.0],
        rotation: [0.0, 270.0, 0.0],
        cuboids: &[cuboid([-13.0, -7.0, -1.0], [18.0, 6.0, 2.0])],
    },
    Part {
        name: "front",
        uv: [0, 27],
        position: [15.0, 4.0, 0.0],
        rotation: [0.0, 90.0, 0.0],
        cuboids: &[cuboid([-8.0, -7.0, -1.0], [16.0, 6.0, 2.0])],
    },
    Part {
        name: "right",
        uv: [0, 35],
        position: [0.0, 4.0, -9.0],
        rotation: [0.0, 180.0, 0.0],
        cuboids: SIDE,
    },
    Part {
        name: "left",
        uv: [0, 43],
        position: [0.0, 4.0, 9.0],
        rotation: [0.0, 0.0, 0.0],
        cuboids: SIDE,
    },
];
const RAFT: [Part; 2] = [
    Part {
        name: "bottom_above",
        uv: [0, 0],
        position: [0.0, -2.0, 1.0],
        rotation: [90.0, 0.0, 0.0],
        cuboids: &[cuboid([-14.0, -11.0, -4.0], [28.0, 20.0, 4.0])],
    },
    Part {
        name: "bottom_below",
        uv: [0, 0],
        position: [0.0, 2.0, 3.0],
        rotation: [90.0, 0.0, 0.0],
        cuboids: &[cuboid([-14.0, -11.0, -4.0], [28.0, 16.0, 4.0])],
    },
];
const PADDLE_LEFT: &[Cuboid] = &[
    cuboid([-1.0, 0.0, -5.0], [2.0, 2.0, 18.0]),
    cuboid([-0.999, -3.0, 8.0], [1.0, 6.0, 7.0]),
];
const PADDLE_RIGHT: &[Cuboid] = &[
    cuboid([-1.0, 0.0, -5.0], [2.0, 2.0, 18.0]),
    cuboid([0.001, -3.0, 8.0], [1.0, 6.0, 7.0]),
];

/// Paddles share their placement across hulls; only their texture origins differ.
fn paddles(left_uv: [u16; 2], right_uv: [u16; 2]) -> [Part; 2] {
    [
        Part {
            name: "paddle_left",
            uv: left_uv,
            position: [3.0, -5.0, 9.0],
            rotation: [0.0, 0.0, 11.25],
            cuboids: PADDLE_LEFT,
        },
        Part {
            name: "paddle_right",
            uv: right_uv,
            position: [3.0, -5.0, -9.0],
            rotation: [0.0, 180.0, 11.25],
            cuboids: PADDLE_RIGHT,
        },
    ]
}

/// The chest sits this many pixels lower on a raft's deck than on a hull.
const RAFT_CHEST_DROP: f32 = 5.0;

const CHEST_LID: &[Cuboid] = &[Cuboid {
    origin: [0.0, -5.0, -12.0],
    size: [12.0, 5.0, 12.0],
    inflate: 0.01,
}];
const CHEST_LOCK: &[Cuboid] = &[cuboid([-1.0, -2.0, -15.0], [2.0, 4.0, 1.0])];
const CHEST_BODY: &[Cuboid] = &[cuboid([0.0, 0.0, 0.0], [12.0, 8.0, 12.0])];

/// Lid, lock and body, lowered onto a raft deck when `raft` is set.
fn chest(raft: bool) -> [Part; 3] {
    let drop = if raft { RAFT_CHEST_DROP } else { 0.0 };
    [
        Part {
            name: "chest_lid",
            uv: [0, 59],
            position: [-14.0, -4.0 - drop, -6.0],
            rotation: [0.0, 270.0, 0.0],
            cuboids: CHEST_LID,
        },
        Part {
            name: "chest_lock",
            uv: [0, 59],
            position: [-16.0, -4.0 - drop, 0.0],
            rotation: [0.0, 270.0, 0.0],
            cuboids: CHEST_LOCK,
        },
        Part {
            name: "chest_bottom",
            uv: [0, 76],
            position: [-2.0, -5.0 - drop, -6.0],
            rotation: [0.0, 270.0, 0.0],
            cuboids: CHEST_BODY,
        },
    ]
}

/// The parts of one native model; chest variants use a 128-pixel-square texture.
fn model(chest_boat: bool, raft: bool) -> (Vec<Part>, [u16; 2]) {
    let mut parts: Vec<Part> = if raft {
        RAFT.into_iter().collect()
    } else {
        HULL.into_iter().collect()
    };
    parts.extend(if raft {
        paddles([0, 24], [40, 24])
    } else {
        paddles([62, 0], [62, 20])
    });
    if chest_boat {
        parts.extend(chest(raft));
    }
    (parts, [128, if chest_boat { 128 } else { 64 }])
}

/// Converts native parts to a Y-up geometry whose origin is the actor's feet. The native
/// model's origin is the actor's render position, `height` pixels above its feet.
fn geometry(identifier: &str, parts: &[Part], texture: [u16; 2], height: f32) -> Value {
    let bones: Vec<Value> = parts
        .iter()
        .map(|part| {
            let [x, y, z] = part.position;
            let cubes: Vec<Value> = part
                .cuboids
                .iter()
                .map(|cuboid| {
                    let [ox, oy, oz] = cuboid.origin;
                    let [_, height_px, _] = cuboid.size;
                    json!({
                        "origin": [x + ox, height - (y + oy + height_px), z + oz],
                        "size": cuboid.size,
                        "uv": part.uv,
                        "inflate": cuboid.inflate,
                    })
                })
                .collect();
            json!({
                "name": part.name,
                "pivot": [x, height - y, z],
                "rotation": part.rotation,
                "cubes": cubes,
            })
        })
        .collect();
    json!({
        "description": {
            "identifier": identifier,
            "texture_width": texture[0],
            "texture_height": texture[1],
            "visible_bounds_width": 3,
            "visible_bounds_height": 2,
            "visible_bounds_offset": [0, 0.5, 0],
        },
        "bones": bones,
    })
}

/// Gives client boats that name no render controller the native hull, raft and paddles.
/// Authored geometry under the entity's default name replaces both native models.
pub(super) fn complete_bindings(
    root: &Path,
    sources: &mut Vec<EntityAssetSource>,
    payloads: &mut SourcePayloads,
    symbols: &mut BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    geometries: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
    remaining_budget: Option<(usize, usize)>,
) -> Result<usize, AssetError> {
    let mut documents = Vec::new();
    let mut replaced = Vec::new();
    for (identifier, chest_boat) in [
        (assets::facts::VanillaEntity::Boat.identifier(), false),
        (assets::facts::VanillaEntity::ChestBoat.identifier(), true),
    ] {
        let mut authored = symbols
            .keys()
            .filter(|(kind, name, _)| *kind == EntityAssetKind::Entity && &**name == identifier);
        let (Some(key), None) = (authored.next(), authored.next()) else {
            continue;
        };
        let key = key.clone();
        let Some(bytes) = payloads.get(&key.2) else {
            continue;
        };
        let mut value = parse_unique_json(Path::new(key.2.as_ref()), bytes)?;
        let Some(description) = value
            .get_mut("minecraft:client_entity")
            .and_then(|entity| entity.get_mut("description"))
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
        if description.contains_key("render_controllers") {
            continue;
        }
        let Some(default) = description
            .get("geometry")
            .and_then(|geometry| geometry.get("default"))
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        let textures = description.get("textures").and_then(Value::as_object);
        let woods: Vec<String> = WOODS
            .iter()
            .map(|&wood| {
                let key = if textures.is_some_and(|textures| textures.contains_key(wood)) {
                    wood
                } else {
                    "default"
                };
                format!("Texture.{key}")
            })
            .collect();
        let height = assets::facts::entity::EntityFacts::pinned()
            .records
            .iter()
            .find(|record| record.identifier.as_ref() == identifier)
            .map_or(0.0, |record| record.fact.network_y_offset * 16.0);
        let authored_geometry = geometries.keys().any(|(name, _)| **name == *default);
        let raft = if authored_geometry {
            default.clone()
        } else {
            let raft = format!(
                "geometry.cinnabar.{}raft",
                if chest_boat { "chest_" } else { "" }
            );
            let (hull, size) = model(chest_boat, false);
            let (deck, _) = model(chest_boat, true);
            documents.push(encode_document(
                unused_source_path(
                    format!(
                        "models/entity/_cinnabar/{}.json",
                        identifier.trim_start_matches("minecraft:")
                    ),
                    payloads,
                ),
                json!({
                    "format_version": "1.12.0",
                    "minecraft:geometry": [
                        geometry(&default, &hull, size, height),
                        geometry(&raft, &deck, size, height),
                    ],
                }),
            )?);
            raft
        };
        description.insert(
            "geometry".into(),
            json!({ "default": default, "raft": raft }),
        );
        description
            .entry("materials")
            .or_insert_with(|| json!({ "default": "entity" }));
        let controller = format!(
            "{CONTROLLER}.{}",
            identifier.trim_start_matches("minecraft:")
        );
        description.insert("render_controllers".into(), json!([controller]));
        documents.push(encode_document(
            unused_source_path(
                format!(
                    "entity/_cinnabar/{}.json",
                    identifier.trim_start_matches("minecraft:")
                ),
                payloads,
            ),
            value,
        )?);
        replaced.push((key, controller, woods));
    }
    if replaced.is_empty() {
        return Ok(0);
    }
    // Each entity's controller names only that entity's own texture aliases.
    let controllers: serde_json::Map<String, Value> = replaced
        .iter()
        .map(|(_, controller, woods)| {
            (
                controller.clone(),
                json!({
                    "arrays": { "textures": { "Array.woods": woods } },
                    "geometry": format!(
                        "query.variant == {RAFT_VARIANT} ? Geometry.raft : Geometry.default"
                    ),
                    "materials": [{ "*": "Material.default" }],
                    "textures": ["Array.woods[query.variant]"],
                }),
            )
        })
        .collect();
    documents.push(encode_document(
        unused_source_path("render_controllers/_cinnabar/boats.json".into(), payloads),
        json!({ "format_version": "1.8.0", "render_controllers": controllers }),
    )?);
    let bytes: usize = documents.iter().map(|(_, bytes)| bytes.len()).sum();
    if remaining_budget.is_some_and(|(count, budget)| documents.len() > count || bytes > budget)
        || (remaining_budget.is_some()
            && symbols.len() + documents.len() + 2 > assets::MAX_ENTITY_ASSET_SYMBOLS)
    {
        return Ok(0);
    }
    for (key, ..) in &replaced {
        symbols.remove(key);
    }
    for (path, bytes) in documents {
        parse_source(&path, &root.join(&path), &bytes, symbols, geometries)?;
        sources.push(EntityAssetSource {
            path: path.clone().into(),
            source_bytes: bytes.len() as u32,
            source_sha256: Sha256::digest(&bytes).into(),
        });
        payloads.insert(path.into(), bytes.into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
