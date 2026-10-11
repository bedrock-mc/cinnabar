//! Typed `particle_effect` definitions parsed from vanilla-format json.

use serde_json::Value;

use super::molang::{Interner, Program, variable_key};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    Alpha,
    Blend,
    Opaque,
    Add,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureSource {
    Path(Box<str>),
    Terrain,
    Items,
}

pub enum Direction {
    Outwards,
    Inwards,
    Vector([Program; 3]),
}

pub struct ShapeCommon {
    pub offset: [Program; 3],
    pub direction: Direction,
    pub surface_only: bool,
}

pub enum Shape {
    Point,
    Sphere {
        radius: Program,
    },
    Box {
        half: [Program; 3],
    },
    Disc {
        radius: Program,
        normal: [Program; 3],
    },
    Custom,
    EntityAabb,
}

pub struct EmitterShape {
    pub common: ShapeCommon,
    pub kind: Shape,
}

pub enum Rate {
    Instant { count: Program },
    Steady { per_second: Program, max: Program },
    Manual { max: Program },
}

pub enum Lifetime {
    Once {
        active: Program,
    },
    Looping {
        active: Program,
        sleep: Program,
    },
    Expression {
        activation: Program,
        expiration: Program,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnKind {
    Emitter,
    EmitterBound,
    Particle,
    ParticleWithVelocity,
}

pub enum EventNode {
    Spawn { effect: Box<str>, kind: SpawnKind },
    Sound(Box<str>),
    Expression(Program),
    Sequence(Vec<EventNode>),
    Randomize(Vec<(f32, EventNode)>),
}

pub enum Motion {
    None,
    Dynamic {
        acceleration: [Program; 3],
        drag: Program,
        rotation_acceleration: Program,
        rotation_drag: Program,
    },
    Parametric {
        position: Option<[Program; 3]>,
        direction: Option<[Program; 3]>,
        rotation: Option<Program>,
    },
}

pub struct CollisionEvent {
    pub event: Box<str>,
    pub min_speed: f32,
}

pub struct Collision {
    pub enabled: Program,
    pub drag: Program,
    pub restitution: Program,
    pub radius: Program,
    pub expire_on_contact: bool,
    pub events: Vec<CollisionEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    RotateXyz,
    RotateY,
    LookatXyz,
    LookatY,
    LookatDirection,
    DirectionX,
    DirectionY,
    DirectionZ,
    EmitterTransformXy,
    EmitterTransformXz,
    EmitterTransformYz,
}

pub struct Flipbook {
    pub base: [Program; 2],
    pub size: [Program; 2],
    pub step: [Program; 2],
    pub fps: Program,
    pub max_frame: Program,
    pub stretch_to_lifetime: bool,
    pub looping: bool,
}

pub struct Uv {
    pub texture_size: [f32; 2],
    pub uv: [Program; 2],
    pub size: [Program; 2],
    pub flipbook: Option<Flipbook>,
}

pub enum DirectionMode {
    DeriveFromVelocity,
    Custom([Program; 3]),
}

pub struct Billboard {
    pub size: [Program; 2],
    pub facing: Facing,
    pub direction: DirectionMode,
    pub min_speed_threshold: f32,
    pub uv: Uv,
}

pub enum Tint {
    White,
    Fixed([Program; 4]),
    Gradient {
        stops: Vec<(f32, [Program; 4])>,
        interpolant: Program,
    },
}

pub struct Spin {
    pub rotation: Program,
    pub rate: Program,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveKind {
    Linear,
    Bezier,
    BezierChain,
    CatmullRom,
}

pub struct Curve {
    pub slot: u16,
    pub kind: CurveKind,
    pub nodes: Vec<f32>,
    /// `(time, value, slope)` for bezier chains.
    pub chain: Vec<(f32, f32, f32)>,
    pub input: Program,
    pub range: Program,
}

pub struct ParticleDef {
    pub max_lifetime: Option<Program>,
    pub expiration: Option<Program>,
    pub kill_plane: Option<[f32; 4]>,
    pub initial_speed: Option<Program>,
    pub initial_velocity: Option<[Program; 3]>,
    pub spin: Option<Spin>,
    pub per_update: Option<Program>,
    pub per_render: Option<Program>,
    pub motion: Motion,
    pub collision: Option<Collision>,
    pub billboard: Billboard,
    pub tint: Tint,
    pub lit: bool,
    pub expire_if_not_in: crate::world::BlockList,
    pub expire_if_in: crate::world::BlockList,
    pub creation_events: Vec<Box<str>>,
    pub expiration_events: Vec<Box<str>>,
    /// Sorted by time in seconds.
    pub timeline: Vec<(f32, Box<str>)>,
    /// Sorted by travelled distance in blocks; each fires once.
    pub travel_events: Vec<(f32, Box<str>)>,
    /// `(interval, event)`; each fires every interval blocks travelled.
    pub looping_travel_events: Vec<(f32, Box<str>)>,
}

pub struct EmitterDef {
    pub creation: Option<Program>,
    pub per_update: Option<Program>,
    pub local_position: bool,
    pub local_velocity: bool,
    pub rate: Rate,
    pub lifetime: Lifetime,
    pub shape: EmitterShape,
}

pub struct EffectDef {
    pub identifier: Box<str>,
    pub material: Material,
    pub texture: TextureSource,
    pub interner: Interner,
    pub curves: Vec<Curve>,
    pub events: Vec<(Box<str>, EventNode)>,
    pub emitter: EmitterDef,
    pub particle: ParticleDef,
}

impl EffectDef {
    #[must_use]
    pub fn event(&self, name: &str) -> Option<&EventNode> {
        self.events
            .iter()
            .find(|(key, _)| key.as_ref() == name)
            .map(|(_, node)| node)
    }
}

/// Parses one `particle_effect` document; `None` when it lacks the required structure.
#[must_use]
pub fn parse_effect(bytes: &[u8]) -> Option<EffectDef> {
    let cleaned = assets::strip_json_comments(bytes);
    let root: Value = serde_json::from_slice(&cleaned).ok()?;
    let effect = root.get("particle_effect")?;
    let description = effect.get("description")?;
    let identifier = description.get("identifier")?.as_str()?;
    let render = description.get("basic_render_parameters")?;
    let material = match render.get("material").and_then(Value::as_str) {
        Some(name) if name.contains("blend") => Material::Blend,
        Some(name) if name.contains("opaque") => Material::Opaque,
        Some(name) if name.contains("add") => Material::Add,
        _ => Material::Alpha,
    };
    let texture = match render.get("texture").and_then(Value::as_str)? {
        "atlas.terrain" => TextureSource::Terrain,
        "atlas.items" => TextureSource::Items,
        path => TextureSource::Path(path.into()),
    };
    let mut interner = Interner::with_builtins();
    let components = effect.get("components")?.as_object()?;
    let component = |name: &str| components.get(name);
    let it = &mut interner;

    let curves: Vec<Curve> = effect
        .get("curves")
        .and_then(Value::as_object)
        .map(|curves| {
            curves
                .iter()
                .filter_map(|(name, value)| parse_curve(name, value, it))
                .collect()
        })
        .unwrap_or_default();
    let events: Vec<(Box<str>, EventNode)> = effect
        .get("events")
        .and_then(Value::as_object)
        .map(|events| {
            events
                .iter()
                .filter_map(|(name, value)| Some((name.as_str().into(), parse_event(value, it)?)))
                .collect()
        })
        .unwrap_or_default();

    let emitter = EmitterDef {
        creation: component("minecraft:emitter_initialization")
            .and_then(|c| c.get("creation_expression"))
            .and_then(|v| program(v, it)),
        per_update: component("minecraft:emitter_initialization")
            .and_then(|c| c.get("per_update_expression"))
            .and_then(|v| program(v, it)),
        local_position: local_flag(component("minecraft:emitter_local_space"), "position"),
        local_velocity: local_flag(component("minecraft:emitter_local_space"), "velocity"),
        rate: parse_rate(components, it),
        lifetime: parse_lifetime(components, it),
        shape: parse_shape(components, it),
    };
    let particle = parse_particle(components, it)?;
    Some(EffectDef {
        identifier: identifier.into(),
        material,
        texture,
        interner,
        curves,
        events,
        emitter,
        particle,
    })
}

fn local_flag(component: Option<&Value>, key: &str) -> bool {
    component
        .and_then(|c| c.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// A number, boolean, Molang string, or `{ "expression": ... }` object.
fn program(value: &Value, it: &mut Interner) -> Option<Program> {
    match value {
        Value::Number(number) => Some(Program::constant(number.as_f64()? as f32)),
        Value::Bool(flag) => Some(Program::constant(f32::from(*flag))),
        Value::String(text) => Program::parse(text, it),
        Value::Object(map) => program(map.get("expression")?, it),
        _ => None,
    }
}

fn program_or(value: Option<&Value>, default: f32, it: &mut Interner) -> Program {
    value
        .and_then(|value| program(value, it))
        .unwrap_or_else(|| Program::constant(default))
}

fn vector<const N: usize>(
    value: Option<&Value>,
    defaults: [f32; N],
    it: &mut Interner,
) -> [Program; N] {
    let items = value.and_then(Value::as_array);
    let programs: Vec<Program> = (0..N)
        .map(|index| {
            items
                .and_then(|items| items.get(index))
                .and_then(|item| program(item, it))
                .unwrap_or_else(|| Program::constant(defaults[index]))
        })
        .collect();
    programs.try_into().unwrap_or_else(|_| unreachable!())
}

fn number(value: Option<&Value>, default: f32) -> f32 {
    value.and_then(Value::as_f64).map_or(default, |v| v as f32)
}

fn parse_rate(components: &serde_json::Map<String, Value>, it: &mut Interner) -> Rate {
    if let Some(c) = components.get("minecraft:emitter_rate_steady") {
        return Rate::Steady {
            per_second: program_or(c.get("spawn_rate"), 1.0, it),
            max: program_or(c.get("max_particles"), 50.0, it),
        };
    }
    if let Some(c) = components.get("minecraft:emitter_rate_manual") {
        return Rate::Manual {
            max: program_or(c.get("max_particles"), 50.0, it),
        };
    }
    let count = components
        .get("minecraft:emitter_rate_instant")
        .and_then(|c| c.get("num_particles"));
    Rate::Instant {
        count: program_or(count, 10.0, it),
    }
}

fn parse_lifetime(components: &serde_json::Map<String, Value>, it: &mut Interner) -> Lifetime {
    if let Some(c) = components.get("minecraft:emitter_lifetime_looping") {
        return Lifetime::Looping {
            active: program_or(c.get("active_time"), 10.0, it),
            sleep: program_or(c.get("sleep_time"), 0.0, it),
        };
    }
    if let Some(c) = components.get("minecraft:emitter_lifetime_expression") {
        return Lifetime::Expression {
            activation: program_or(c.get("activation_expression"), 1.0, it),
            expiration: program_or(c.get("expiration_expression"), 0.0, it),
        };
    }
    let active = components
        .get("minecraft:emitter_lifetime_once")
        .and_then(|c| c.get("active_time"));
    Lifetime::Once {
        active: program_or(active, 10.0, it),
    }
}

fn parse_direction(value: Option<&Value>, it: &mut Interner) -> Direction {
    match value {
        Some(Value::String(text)) if text == "inwards" => Direction::Inwards,
        Some(Value::Array(_)) => Direction::Vector(vector(value, [0.0; 3], it)),
        _ => Direction::Outwards,
    }
}

fn parse_shape(components: &serde_json::Map<String, Value>, it: &mut Interner) -> EmitterShape {
    let common = |c: &Value, it: &mut Interner| ShapeCommon {
        offset: vector(c.get("offset"), [0.0; 3], it),
        direction: parse_direction(c.get("direction"), it),
        surface_only: c
            .get("surface_only")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };
    if let Some(c) = components.get("minecraft:emitter_shape_sphere") {
        return EmitterShape {
            common: common(c, it),
            kind: Shape::Sphere {
                radius: program_or(c.get("radius"), 1.0, it),
            },
        };
    }
    if let Some(c) = components.get("minecraft:emitter_shape_box") {
        return EmitterShape {
            common: common(c, it),
            kind: Shape::Box {
                half: vector(c.get("half_dimensions"), [1.0; 3], it),
            },
        };
    }
    if let Some(c) = components.get("minecraft:emitter_shape_disc") {
        let normal = match c.get("plane_normal") {
            Some(Value::String(axis)) => {
                let axes: [f32; 3] = match axis.as_str() {
                    "x" => [1.0, 0.0, 0.0],
                    "z" => [0.0, 0.0, 1.0],
                    _ => [0.0, 1.0, 0.0],
                };
                axes.map(Program::constant)
            }
            other => vector(other, [0.0, 1.0, 0.0], it),
        };
        return EmitterShape {
            common: common(c, it),
            kind: Shape::Disc {
                radius: program_or(c.get("radius"), 1.0, it),
                normal,
            },
        };
    }
    if let Some(c) = components.get("minecraft:emitter_shape_custom") {
        return EmitterShape {
            common: common(c, it),
            kind: Shape::Custom,
        };
    }
    if let Some(c) = components.get("minecraft:emitter_shape_entity_aabb") {
        return EmitterShape {
            common: common(c, it),
            kind: Shape::EntityAabb,
        };
    }
    let empty = Value::Null;
    let c = components
        .get("minecraft:emitter_shape_point")
        .unwrap_or(&empty);
    EmitterShape {
        common: common(c, it),
        kind: Shape::Point,
    }
}

fn parse_collision(c: &Value, it: &mut Interner) -> Collision {
    let events = c
        .get("events")
        .and_then(Value::as_array)
        .map(|events| {
            events
                .iter()
                .filter_map(|event| {
                    Some(CollisionEvent {
                        event: event.get("event")?.as_str()?.into(),
                        min_speed: number(event.get("min_speed"), 0.0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Collision {
        enabled: program_or(c.get("enabled"), 1.0, it),
        drag: program_or(c.get("collision_drag"), 0.0, it),
        restitution: program_or(c.get("coefficient_of_restitution"), 0.0, it),
        radius: program_or(c.get("collision_radius"), 0.1, it),
        expire_on_contact: c
            .get("expire_on_contact")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        events,
    }
}

fn parse_facing(name: Option<&str>) -> Facing {
    match name {
        Some("rotate_xyz") => Facing::RotateXyz,
        Some("rotate_y") => Facing::RotateY,
        Some("lookat_y") => Facing::LookatY,
        Some("lookat_direction") => Facing::LookatDirection,
        Some("direction_x") => Facing::DirectionX,
        Some("direction_y") => Facing::DirectionY,
        Some("direction_z") => Facing::DirectionZ,
        Some("emitter_transform_xy") => Facing::EmitterTransformXy,
        Some("emitter_transform_xz") => Facing::EmitterTransformXz,
        Some("emitter_transform_yz") => Facing::EmitterTransformYz,
        _ => Facing::LookatXyz,
    }
}

fn parse_billboard(c: &Value, it: &mut Interner) -> Billboard {
    let uv = c.get("uv");
    let field = |key: &str| uv.and_then(|uv| uv.get(key));
    let flipbook = field("flipbook").map(|f| Flipbook {
        base: vector(f.get("base_UV"), [0.0; 2], it),
        size: vector(f.get("size_UV"), [0.0; 2], it),
        step: vector(f.get("step_UV"), [0.0; 2], it),
        fps: program_or(f.get("frames_per_second"), 0.0, it),
        max_frame: program_or(f.get("max_frame"), 1.0, it),
        stretch_to_lifetime: f
            .get("stretch_to_lifetime")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        looping: f.get("loop").and_then(Value::as_bool).unwrap_or(false),
    });
    let direction = c.get("direction");
    let mode = match direction
        .and_then(|d| d.get("mode"))
        .and_then(Value::as_str)
    {
        Some("custom") => DirectionMode::Custom(vector(
            direction.and_then(|d| d.get("custom_direction")),
            [0.0; 3],
            it,
        )),
        _ => DirectionMode::DeriveFromVelocity,
    };
    Billboard {
        size: vector(c.get("size"), [0.0; 2], it),
        facing: parse_facing(c.get("facing_camera_mode").and_then(Value::as_str)),
        direction: mode,
        min_speed_threshold: number(direction.and_then(|d| d.get("min_speed_threshold")), 0.0),
        uv: Uv {
            texture_size: [
                number(field("texture_width"), 1.0).max(1.0),
                number(field("texture_height"), 1.0).max(1.0),
            ],
            uv: vector(field("uv"), [0.0; 2], it),
            size: vector(field("uv_size"), [1.0; 2], it),
            flipbook,
        },
    }
}

fn parse_color(value: &Value, it: &mut Interner) -> Option<[Program; 4]> {
    match value {
        Value::String(hex) if hex.starts_with('#') => {
            let digits = &hex[1..];
            let component = |at: usize| {
                u8::from_str_radix(digits.get(at..at + 2)?, 16)
                    .ok()
                    .map(|v| f32::from(v) / 255.0)
            };
            let (offset, a) = match digits.len() {
                6 => (0, 1.0),
                8 => (2, component(0)?),
                _ => return None,
            };
            let (r, g, b) = (
                component(offset)?,
                component(offset + 2)?,
                component(offset + 4)?,
            );
            Some([r, g, b, a].map(Program::constant))
        }
        Value::Array(items) if items.len() >= 3 => {
            Some(vector(Some(value), [1.0, 1.0, 1.0, 1.0], it))
        }
        _ => None,
    }
}

fn parse_tint(c: &Value, it: &mut Interner) -> Tint {
    let Some(color) = c.get("color") else {
        return Tint::White;
    };
    if let Some(fixed) = parse_color(color, it) {
        return Tint::Fixed(fixed);
    }
    let Some(gradient) = color.get("gradient") else {
        return Tint::White;
    };
    let mut stops: Vec<(f32, [Program; 4])> = match gradient {
        Value::Object(map) => map
            .iter()
            .filter_map(|(key, value)| Some((key.parse().ok()?, parse_color(value, it)?)))
            .collect(),
        Value::Array(items) => {
            let count = items.len().max(2) - 1;
            items
                .iter()
                .enumerate()
                .filter_map(|(index, value)| {
                    Some((index as f32 / count as f32, parse_color(value, it)?))
                })
                .collect()
        }
        _ => return Tint::White,
    };
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    if stops.is_empty() {
        return Tint::White;
    }
    Tint::Gradient {
        interpolant: program_or(color.get("interpolant"), 0.0, it),
        stops,
    }
}

fn string_list(value: Option<&Value>) -> Vec<Box<str>> {
    match value {
        Some(Value::String(text)) => vec![text.as_str().into()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(Into::into))
            .collect(),
        _ => Vec::new(),
    }
}

/// `[{ "distance": d, "events": [...] }]` flattened to sorted `(distance, event)` pairs.
fn parse_travel(value: Option<&Value>) -> Vec<(f32, Box<str>)> {
    let mut out: Vec<(f32, Box<str>)> = value
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .flat_map(|entry| {
                    let distance = number(entry.get("distance"), 0.0);
                    let events = entry.get("events").or_else(|| entry.get("event"));
                    string_list(events)
                        .into_iter()
                        .map(move |event| (distance, event))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// Preserves absent vectors and rejects malformed authored three-component expressions.
fn optional_vector(value: Option<&Value>, it: &mut Interner) -> Option<Option<[Program; 3]>> {
    match value {
        Some(Value::Array(values)) if values.len() == 3 => Some(Some([
            program(&values[0], it)?,
            program(&values[1], it)?,
            program(&values[2], it)?,
        ])),
        None => Some(None),
        _ => None,
    }
}

fn parse_particle(
    components: &serde_json::Map<String, Value>,
    it: &mut Interner,
) -> Option<ParticleDef> {
    let get = |name: &str| components.get(name);
    let lifetime = get("minecraft:particle_lifetime_expression")?;
    let billboard = parse_billboard(get("minecraft:particle_appearance_billboard")?, it);
    let (initial_speed, initial_velocity) = match get("minecraft:particle_initial_speed") {
        Some(Value::Array(_)) => (
            None,
            Some(vector(
                get("minecraft:particle_initial_speed"),
                [0.0; 3],
                it,
            )),
        ),
        Some(value) => (program(value, it), None),
        None => (None, None),
    };
    let motion = if let Some(c) = get("minecraft:particle_motion_dynamic") {
        Motion::Dynamic {
            acceleration: vector(c.get("linear_acceleration"), [0.0; 3], it),
            drag: program_or(c.get("linear_drag_coefficient"), 0.0, it),
            rotation_acceleration: program_or(c.get("rotation_acceleration"), 0.0, it),
            rotation_drag: program_or(c.get("rotation_drag_coefficient"), 0.0, it),
        }
    } else if let Some(c) = get("minecraft:particle_motion_parametric") {
        Motion::Parametric {
            position: optional_vector(c.get("relative_position"), it)?,
            direction: optional_vector(c.get("direction"), it)?,
            rotation: match c.get("rotation") {
                Some(value) => Some(program(value, it)?),
                None => None,
            },
        }
    } else {
        Motion::None
    };
    let (travel_events, looping_travel_events) = match get("minecraft:particle_lifetime_events") {
        Some(c) => (
            parse_travel(c.get("travel_distance_events")),
            parse_travel(c.get("looping_travel_distance_events")),
        ),
        None => (Vec::new(), Vec::new()),
    };
    let (creation_events, expiration_events, timeline) =
        match get("minecraft:particle_lifetime_events") {
            Some(c) => {
                let mut timeline: Vec<(f32, Box<str>)> = c
                    .get("timeline")
                    .and_then(Value::as_object)
                    .map(|map| {
                        map.iter()
                            .filter_map(|(time, events)| {
                                let time: f32 = time.parse().ok()?;
                                Some(
                                    string_list(Some(events))
                                        .into_iter()
                                        .map(move |event| (time, event)),
                                )
                            })
                            .flatten()
                            .collect()
                    })
                    .unwrap_or_default();
                timeline.sort_by(|a, b| a.0.total_cmp(&b.0));
                (
                    string_list(c.get("creation_event")),
                    string_list(c.get("expiration_event")),
                    timeline,
                )
            }
            None => (Vec::new(), Vec::new(), Vec::new()),
        };
    Some(ParticleDef {
        max_lifetime: match lifetime.get("max_lifetime") {
            Some(value) => Some(program(value, it)?),
            None => None,
        },
        expiration: match lifetime.get("expiration_expression") {
            Some(value) => Some(program(value, it)?),
            None => None,
        },
        kill_plane: get("minecraft:particle_kill_plane")
            .and_then(Value::as_array)
            .filter(|plane| plane.len() >= 4)
            .map(|plane| std::array::from_fn(|i| number(plane.get(i), 0.0))),
        initial_speed,
        initial_velocity,
        spin: get("minecraft:particle_initial_spin").map(|c| Spin {
            rotation: program_or(c.get("rotation"), 0.0, it),
            rate: program_or(c.get("rotation_rate"), 0.0, it),
        }),
        per_update: get("minecraft:particle_initialization")
            .and_then(|c| c.get("per_update_expression"))
            .and_then(|v| program(v, it)),
        per_render: get("minecraft:particle_initialization")
            .and_then(|c| c.get("per_render_expression"))
            .and_then(|v| program(v, it)),
        motion,
        collision: get("minecraft:particle_motion_collision").map(|c| parse_collision(c, it)),
        billboard,
        tint: get("minecraft:particle_appearance_tinting")
            .map_or(Tint::White, |c| parse_tint(c, it)),
        lit: get("minecraft:particle_appearance_lighting").is_some(),
        expire_if_not_in: crate::world::BlockList::new(string_list(get(
            "minecraft:particle_expire_if_not_in_blocks",
        ))),
        expire_if_in: crate::world::BlockList::new(string_list(get(
            "minecraft:particle_expire_if_in_blocks",
        ))),
        creation_events,
        expiration_events,
        timeline,
        travel_events,
        looping_travel_events,
    })
}

fn parse_curve(name: &str, value: &Value, it: &mut Interner) -> Option<Curve> {
    let slot = it.intern(&variable_key(name)?)?;
    let kind = match value.get("type")?.as_str()? {
        "linear" => CurveKind::Linear,
        "bezier" => CurveKind::Bezier,
        "bezier_chain" => CurveKind::BezierChain,
        "catmull_rom" => CurveKind::CatmullRom,
        _ => return None,
    };
    let mut nodes = Vec::new();
    let mut chain = Vec::new();
    match value.get("nodes")? {
        Value::Array(items) => {
            nodes = items
                .iter()
                .filter_map(Value::as_f64)
                .map(|v| v as f32)
                .collect()
        }
        Value::Object(map) => {
            for (time, node) in map {
                chain.push((
                    time.parse::<f32>().ok()?,
                    number(node.get("value"), 0.0),
                    number(node.get("slope"), 0.0),
                ));
            }
            chain.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        _ => return None,
    }
    Some(Curve {
        slot,
        kind,
        nodes,
        chain,
        input: program_or(value.get("input"), 0.0, it),
        range: program_or(value.get("horizontal_range"), 1.0, it),
    })
}

fn parse_event(value: &Value, it: &mut Interner) -> Option<EventNode> {
    let mut nodes = Vec::new();
    if let Some(effect) = value.get("particle_effect") {
        let kind = match effect.get("type").and_then(Value::as_str) {
            Some("emitter_bound") => SpawnKind::EmitterBound,
            Some("particle") => SpawnKind::Particle,
            Some("particle_with_velocity") => SpawnKind::ParticleWithVelocity,
            _ => SpawnKind::Emitter,
        };
        nodes.push(EventNode::Spawn {
            effect: effect.get("effect")?.as_str()?.into(),
            kind,
        });
    }
    if let Some(name) = value
        .get("sound_effect")
        .and_then(|sound| sound.get("event_name"))
        .and_then(Value::as_str)
    {
        nodes.push(EventNode::Sound(name.into()));
    }
    if let Some(expression) = value.get("expression").and_then(|v| program(v, it)) {
        nodes.push(EventNode::Expression(expression));
    }
    if let Some(items) = value.get("sequence").and_then(Value::as_array) {
        nodes.push(EventNode::Sequence(
            items
                .iter()
                .filter_map(|item| parse_event(item, it))
                .collect(),
        ));
    }
    if let Some(items) = value.get("randomize").and_then(Value::as_array) {
        nodes.push(EventNode::Randomize(
            items
                .iter()
                .filter_map(|item| {
                    Some((
                        number(item.get("weight"), 1.0).max(0.0),
                        parse_event(item, it)?,
                    ))
                })
                .collect(),
        ));
    }
    match nodes.len() {
        0 => None,
        1 => nodes.pop(),
        _ => Some(EventNode::Sequence(nodes)),
    }
}

#[cfg(test)]
#[path = "def/color_tests.rs"]
mod color_tests;

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "format_version": "1.26.10",
      "particle_effect": {
        "description": {"identifier": "minecraft:t", "basic_render_parameters": {"material": "particles_blend", "texture": "textures/particle/particles"}},
        "curves": {"variable.shrink": {"type": "linear", "nodes": [1, 0], "input": "v.particle_age", "horizontal_range": "v.particle_lifetime"}},
        "events": {"boom": {"particle_effect": {"effect": "minecraft:x", "type": "emitter"}}},
        "components": {
          "minecraft:emitter_rate_steady": {"spawn_rate": 5, "max_particles": 20},
          "minecraft:emitter_shape_sphere": {"radius": 2, "direction": "outwards"},
          "minecraft:particle_lifetime_expression": {"max_lifetime": "1.5f"},
          "minecraft:particle_appearance_billboard": {"size": [0.1, {"expression": "v.shrink", "version": 12}], "facing_camera_mode": "rotate_xyz", "uv": {"texture_width": 128, "texture_height": 128, "uv": [8, 8], "uv_size": [8, 8]}},
          "minecraft:particle_appearance_tinting": {"color": {"interpolant": "v.particle_age", "gradient": {"0.0": [1,0,0,1], "1.0": [0, 1, 0, 1]}}}
        }
      }
    }"#;

    #[test]
    fn parses_material_texture_rate_and_curves() {
        let effect = parse_effect(SAMPLE.as_bytes()).unwrap();
        assert_eq!(effect.material, Material::Blend);
        assert_eq!(
            effect.texture,
            TextureSource::Path("textures/particle/particles".into())
        );
        assert!(matches!(effect.emitter.rate, Rate::Steady { .. }));
        assert_eq!(effect.curves.len(), 1);
        assert!(effect.event("boom").is_some());
        assert_eq!(effect.particle.billboard.facing, Facing::RotateXyz);
    }

    #[test]
    fn travel_distance_events_flatten_and_sort() {
        let json = SAMPLE.replace(
            "\"minecraft:particle_appearance_billboard\"",
            "\"minecraft:particle_lifetime_events\": {\"travel_distance_events\": [{\"distance\": 2, \"events\": [\"b\"]}, {\"distance\": 1, \"events\": [\"a\"]}], \"looping_travel_distance_events\": [{\"distance\": 0.5, \"events\": \"c\"}]},\n          \"minecraft:particle_appearance_billboard\"",
        );
        let effect = parse_effect(json.as_bytes()).unwrap();
        let travel: Vec<_> = effect
            .particle
            .travel_events
            .iter()
            .map(|e| &*e.1)
            .collect();
        assert_eq!(travel, ["a", "b"]);
        assert_eq!(effect.particle.looping_travel_events.len(), 1);
    }

    #[test]
    fn rejects_documents_without_lifetime_or_billboard() {
        let bad = br#"{"particle_effect":{"description":{"identifier":"a","basic_render_parameters":{"texture":"t"}},"components":{}}}"#;
        assert!(parse_effect(bad).is_none());
    }
}

#[cfg(test)]
mod lifecycle_admission_tests {
    use super::*;

    #[test]
    fn malformed_authored_lifecycle_fields_are_rejected() {
        for field in [
            serde_json::json!({"minecraft:particle_lifetime_expression": {"expiration_expression": {}}}),
            serde_json::json!({"minecraft:particle_lifetime_expression": {"max_lifetime": {}}}),
            serde_json::json!({"minecraft:particle_motion_parametric": {"direction": [0, 1]}}),
            serde_json::json!({"minecraft:particle_motion_parametric": {"direction": [0, {}, 1]}}),
        ] {
            let mut components = serde_json::json!({
                "minecraft:particle_lifetime_expression": {"max_lifetime": 1},
                "minecraft:particle_appearance_billboard": {"size": [0.1, 0.1]}
            });
            components
                .as_object_mut()
                .unwrap()
                .extend(field.as_object().unwrap().clone());
            let effect = serde_json::json!({"particle_effect": {
                "description": {"identifier": "fixture:admission", "basic_render_parameters": {"material": "particles_alpha", "texture": "fixture"}},
                "components": components
            }});
            assert!(parse_effect(&serde_json::to_vec(&effect).unwrap()).is_none());
        }
    }
}
