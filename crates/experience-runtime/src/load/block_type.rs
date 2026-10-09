//! Server WIT 0.5's block types: states, placement traits, visuals and permutations. The adapter
//! checks the same rules again before it registers a block.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};

use super::geometry::{self, Geometry};
use super::{ALL_FACES, FACES, asset_path, is_name};
use crate::host::cinnabar::experience_server::types as wit;
use crate::limits::{
    MAX_BONES, MAX_CONDITION_TESTS, MAX_FLIPBOOK_FRAMES, MAX_MATERIALS, MAX_NAME_BYTES,
    MAX_PERMUTATIONS, MAX_STATE_COMBINATIONS, MAX_STATE_VALUES,
};
use crate::protocol::{
    BlockState, BoneVisibility, Condition, Flipbook, Material, Permutation, Pixel, PixelBox,
    PlacementState, QuarterTurns, RenderMethod, StateDef, StateTest, StateValue, StateValues,
    Visual,
};

/// The largest coordinate of a box, in pixels: a block's edge.
const BLOCK_PIXELS: f32 = 16.0;
/// The most quarter turns about one axis.
const MAX_QUARTER_TURNS: u8 = 3;

/// Where a block's assets are, and what its Experience's blocks have declared so far.
pub(super) struct Assets<'a> {
    pub root: &'a Path,
    pub files: &'a BTreeMap<String, String>,
    /// The Experience's id.
    pub id: &'a str,
    /// The file of each geometry identifier its blocks use; one identifier is one file.
    pub geometries: HashMap<String, String>,
}

/// A block's states, then its placement traits; with the axes they make, placement traits
/// first, and no more than [`MAX_STATE_COMBINATIONS`] combinations of them.
pub(super) fn states(
    id: &str,
    defs: Vec<wit::StateDef>,
    placement: wit::PlacementStates,
) -> Result<(Vec<StateDef>, Vec<PlacementState>)> {
    let namespace = format!("{id}:");
    let mut states: Vec<StateDef> = Vec::with_capacity(defs.len());
    for wit::StateDef { name, values } in defs {
        ensure!(
            name.strip_prefix(&namespace).is_some_and(is_name),
            "state \"{name}\" is not {namespace}<name>, the name matching \
             ^[a-z0-9_]{{1,{MAX_NAME_BYTES}}}$"
        );
        ensure!(
            states.iter().all(|state| state.name != name),
            "state \"{name}\" is declared twice"
        );
        let values = match values {
            wit::StateValues::Bool => StateValues::Bool,
            wit::StateValues::Choices(choices) => {
                ensure!(
                    (1..=MAX_STATE_VALUES).contains(&choices.len()),
                    "state \"{name}\" has {} values; it needs 1 to {MAX_STATE_VALUES}",
                    choices.len()
                );
                for (i, choice) in choices.iter().enumerate() {
                    ensure!(
                        is_name(choice),
                        "state \"{name}\" value \"{choice}\" does not match \
                         ^[a-z0-9_]{{1,{MAX_NAME_BYTES}}}$"
                    );
                    ensure!(
                        !choices[..i].contains(choice),
                        "state \"{name}\" lists value \"{choice}\" twice"
                    );
                }
                StateValues::Choices(choices)
            }
        };
        states.push(StateDef { name, values });
    }
    let placement: Vec<PlacementState> = PlacementState::ALL
        .into_iter()
        .filter(|state| placement.contains(flag(*state)))
        .collect();
    let combinations = super::axes(&states, &placement)
        .iter()
        .try_fold(1usize, |product, axis| {
            product
                .checked_mul(value_count(&axis.values))
                .filter(|product| *product <= MAX_STATE_COMBINATIONS)
        });
    ensure!(
        combinations.is_some(),
        "its states and placement traits make more than {MAX_STATE_COMBINATIONS} combinations"
    );
    Ok((states, placement))
}

fn flag(state: PlacementState) -> wit::PlacementStates {
    match state {
        PlacementState::CardinalDirection => wit::PlacementStates::CARDINAL_DIRECTION,
        PlacementState::FacingDirection => wit::PlacementStates::FACING_DIRECTION,
        PlacementState::BlockFace => wit::PlacementStates::BLOCK_FACE,
        PlacementState::VerticalHalf => wit::PlacementStates::VERTICAL_HALF,
    }
}

fn value_count(values: &StateValues) -> usize {
    match values {
        StateValues::Bool => 2,
        StateValues::Choices(choices) => choices.len(),
    }
}

/// Whether `value` is one that `axis` takes.
pub(crate) fn holds(axis: &StateDef, value: &StateValue) -> bool {
    match (&axis.values, value) {
        (StateValues::Bool, StateValue::Bool(_)) => true,
        (StateValues::Choices(choices), StateValue::Choice(choice)) => choices.contains(choice),
        _ => false,
    }
}

/// A condition over `axes`, at most [`MAX_CONDITION_TESTS`] tests of states the block has
/// against values they take.
fn condition(axes: &[StateDef], when: wit::Condition) -> Result<Condition> {
    let tests: usize = when.iter().map(Vec::len).sum();
    ensure!(
        tests <= MAX_CONDITION_TESTS,
        "a condition has {tests} tests; the limit is {MAX_CONDITION_TESTS}"
    );
    when.into_iter()
        .map(|clause| {
            clause
                .into_iter()
                .map(|wit::StateTest { state, equal }| {
                    let state = BlockState::from(state);
                    let Some(axis) = axes.iter().find(|axis| axis.name == state.name) else {
                        bail!(
                            "a condition tests state \"{}\", which the block does not have",
                            state.name
                        );
                    };
                    ensure!(
                        holds(axis, &state.value),
                        "a condition tests state \"{}\" against {:?}, a value it does not take",
                        state.name,
                        state.value
                    );
                    Ok(StateTest { state, equal })
                })
                .collect()
        })
        .collect()
}

/// A visual, its geometry read for the names its parts refer to.
pub(super) struct Look {
    pub visual: Visual,
    geometry: Option<Geometry>,
}

/// Checks a block's visual over its state `axes`.
pub(super) fn visual(
    assets: &mut Assets<'_>,
    axes: &[StateDef],
    visual: wit::Visual,
) -> Result<Look> {
    let wit::Visual {
        geometry,
        materials,
        bones,
        collision,
        selection,
        rotation,
    } = visual;
    let (geometry, geometry_path) = match geometry {
        Some(path) => {
            let (read, path) = read_geometry(assets, &path)?;
            (Some(read), Some(path))
        }
        None => (None, None),
    };
    let materials = self::materials(assets, materials, geometry.as_ref())?;
    let bones = self::bones(axes, bones, geometry.as_ref())?;
    Ok(Look {
        visual: Visual {
            geometry: geometry_path,
            materials,
            bones,
            collision: collision.map(|b| pixel_box("collision", b)).transpose()?,
            selection: selection.map(|b| pixel_box("selection", b)).transpose()?,
            rotation: rotation.map(quarter_turns).transpose()?,
        },
        geometry,
    })
}

/// Checks a block's permutations against its look.
pub(super) fn permutations(
    assets: &mut Assets<'_>,
    axes: &[StateDef],
    look: &Look,
    permutations: Vec<wit::Permutation>,
) -> Result<Vec<Permutation>> {
    ensure!(
        permutations.len() <= MAX_PERMUTATIONS,
        "it declares {} permutations; the limit is {MAX_PERMUTATIONS}",
        permutations.len()
    );
    let zero = QuarterTurns { x: 0, y: 0, z: 0 };
    let mut checked: Vec<Permutation> = Vec::with_capacity(permutations.len());
    for (i, permutation) in permutations.into_iter().enumerate() {
        let permutation = self::permutation(assets, axes, look, permutation)
            .with_context(|| format!("permutation {i}"))?;
        ensure!(
            permutation.rotation != Some(zero)
                || !checked.iter().any(|earlier| {
                    earlier.rotation.is_some_and(|turns| turns != zero)
                        && conditions_overlap(axes, &earlier.when, &permutation.when)
                }),
            "permutation {i}: its zero rotation cannot replace an earlier permutation"
        );
        checked.push(permutation);
    }
    Ok(checked)
}

/// Whether two validated conditions can both hold, intersecting each clause's allowed state values.
fn conditions_overlap(axes: &[StateDef], left: &Condition, right: &Condition) -> bool {
    left.iter().any(|left| {
        right.iter().any(|right| {
            let mut allowed = HashMap::<usize, u32>::new();
            for test in left.iter().chain(right) {
                let (index, axis) = axes
                    .iter()
                    .enumerate()
                    .find(|(_, axis)| axis.name == test.state.name)
                    .expect("validated state");
                let value = match (&axis.values, &test.state.value) {
                    (StateValues::Bool, StateValue::Bool(value)) => usize::from(*value),
                    (StateValues::Choices(values), StateValue::Choice(value)) => values
                        .iter()
                        .position(|candidate| candidate == value)
                        .expect("validated value"),
                    _ => unreachable!("validated state value"),
                };
                let mask = allowed
                    .entry(index)
                    .or_insert((1 << value_count(&axis.values)) - 1);
                if test.equal {
                    *mask &= 1 << value;
                } else {
                    *mask &= !(1 << value);
                }
                if *mask == 0 {
                    return false;
                }
            }
            true
        })
    })
}

fn permutation(
    assets: &mut Assets<'_>,
    axes: &[StateDef],
    look: &Look,
    permutation: wit::Permutation,
) -> Result<Permutation> {
    let wit::Permutation {
        when,
        geometry,
        materials,
        bones,
        collision,
        selection,
        rotation,
    } = permutation;
    ensure!(!when.is_empty(), "its condition never holds");
    ensure!(
        geometry.is_some()
            || materials.is_some()
            || bones.is_some()
            || collision.is_some()
            || selection.is_some()
            || rotation.is_some(),
        "it sets nothing"
    );
    let when = condition(axes, when)?;
    let (read, geometry) = match geometry {
        Some(path) => {
            let (read, path) = read_geometry(assets, &path)?;
            (Some(read), Some(path))
        }
        None => (None, None),
    };
    let shape = read.as_ref().or(look.geometry.as_ref());
    let materials = match materials {
        Some(materials) => Some(self::materials(assets, materials, shape)?),
        None => {
            // The block's materials must still cover a geometry of the permutation's own.
            covered(&look.visual.materials, shape)?;
            None
        }
    };
    let bones = bones
        .map(|bones| self::bones(axes, bones, shape))
        .transpose()?;
    let rotation = rotation.map(quarter_turns).transpose()?;
    // A zero rotation reads as none, so it cannot undo the block's own.
    let zero = QuarterTurns { x: 0, y: 0, z: 0 };
    ensure!(
        rotation != Some(zero) || look.visual.rotation.is_none_or(|turns| turns == zero),
        "its zero rotation cannot replace the visual's rotation"
    );
    Ok(Permutation {
        when,
        geometry,
        materials,
        bones,
        collision: collision.map(|b| pixel_box("collision", b)).transpose()?,
        selection: selection.map(|b| pixel_box("selection", b)).transpose()?,
        rotation,
    })
}

/// Reads the indexed geometry at `path`; one identifier must always be the same file.
fn read_geometry(assets: &mut Assets<'_>, path: &str) -> Result<(Geometry, String)> {
    let absolute = asset_path(assets.root, assets.files, path)?;
    let geometry = geometry::read(Path::new(&absolute), assets.id)
        .with_context(|| format!("geometry \"{path}\""))?;
    let file = assets
        .geometries
        .entry(geometry.identifier.clone())
        .or_insert_with(|| absolute.clone());
    ensure!(
        *file == absolute,
        "geometry \"{}\" is in two files",
        geometry.identifier
    );
    Ok((geometry, absolute))
}

/// Checks materials for `geometry`, the full cube when none: each instance `*`, a face, or one
/// the geometry names, once, and together covering every instance it draws with.
fn materials(
    assets: &Assets<'_>,
    materials: Vec<wit::Material>,
    geometry: Option<&Geometry>,
) -> Result<Vec<Material>> {
    ensure!(
        (1..=MAX_MATERIALS).contains(&materials.len()),
        "it lists {} materials; it needs 1 to {MAX_MATERIALS}",
        materials.len()
    );
    let mut checked: Vec<Material> = Vec::with_capacity(materials.len());
    for wit::Material {
        instance,
        path,
        render_method,
        flipbook,
    } in materials
    {
        ensure!(
            instance == ALL_FACES
                || FACES.contains(&instance.as_str())
                || geometry.is_some_and(|geometry| geometry.instances.contains(&instance)),
            "material instance \"{instance}\" is neither \"{ALL_FACES}\", a face, nor one its \
             geometry draws with"
        );
        ensure!(
            checked.iter().all(|material| material.instance != instance),
            "material instance \"{instance}\" is listed twice"
        );
        let path = asset_path(assets.root, assets.files, &path)?;
        let flipbook = flipbook
            .map(|flipbook| self::flipbook(Path::new(&path), flipbook))
            .transpose()
            .with_context(|| format!("material instance \"{instance}\""))?;
        checked.push(Material {
            instance,
            path,
            render_method: match render_method {
                wit::RenderMethod::Opaque => RenderMethod::Opaque,
                wit::RenderMethod::AlphaTest => RenderMethod::AlphaTest,
                wit::RenderMethod::Blend => RenderMethod::Blend,
                wit::RenderMethod::DoubleSided => RenderMethod::DoubleSided,
            },
            flipbook,
        });
    }
    covered(&checked, geometry)?;
    Ok(checked)
}

/// Whether `materials` give every instance that `geometry`, or the full cube, draws with a
/// texture: by name, or through `*`.
fn covered(materials: &[Material], geometry: Option<&Geometry>) -> Result<()> {
    let listed = |instance: &str| {
        materials
            .iter()
            .any(|material| material.instance == instance)
    };
    if listed(ALL_FACES) {
        return Ok(());
    }
    let drawn: Vec<&str> = match geometry {
        Some(geometry) => geometry.instances.iter().map(String::as_str).collect(),
        None => FACES.to_vec(),
    };
    if let Some(instance) = drawn.into_iter().find(|instance| !listed(instance)) {
        bail!(
            "material instance \"{instance}\" is drawn but has no material, and there is no \
             \"{ALL_FACES}\""
        );
    }
    Ok(())
}

/// Checks bone visibilities: at most [`MAX_BONES`], each of a bone of `geometry`, once.
fn bones(
    axes: &[StateDef],
    bones: Vec<wit::BoneVisibility>,
    geometry: Option<&Geometry>,
) -> Result<Vec<BoneVisibility>> {
    ensure!(
        bones.len() <= MAX_BONES,
        "it sets the visibility of {} bones; the limit is {MAX_BONES}",
        bones.len()
    );
    let mut checked: Vec<BoneVisibility> = Vec::with_capacity(bones.len());
    for wit::BoneVisibility { bone, visible } in bones {
        let Some(geometry) = geometry else {
            bail!("bone \"{bone}\" has a visibility, but the full cube has no bones");
        };
        ensure!(
            geometry.bones.contains(&bone),
            "geometry \"{}\" has no bone \"{bone}\"",
            geometry.identifier
        );
        ensure!(
            checked.iter().all(|checked| checked.bone != bone),
            "bone \"{bone}\" has two visibilities"
        );
        let visible = condition(axes, visible).with_context(|| format!("bone \"{bone}\""))?;
        checked.push(BoneVisibility { bone, visible });
    }
    Ok(checked)
}

/// Checks a flipbook of the strip at `path`: a whole number of square frames, of which it shows
/// only ones there are.
fn flipbook(path: &Path, flipbook: wit::Flipbook) -> Result<Flipbook> {
    let wit::Flipbook {
        ticks_per_frame,
        frames,
        blend_frames,
    } = flipbook;
    ensure!(
        ticks_per_frame >= 1,
        "its flipbook shows a frame for 0 ticks"
    );
    ensure!(
        frames.len() <= MAX_FLIPBOOK_FRAMES,
        "its flipbook lists {} frames; the limit is {MAX_FLIPBOOK_FRAMES}",
        frames.len()
    );
    let (width, height) = png_size(path)?;
    ensure!(
        width > 0 && height % width == 0,
        "its flipbook strip is {width}×{height} pixels, not whole square frames"
    );
    let count = height / width;
    if let Some(frame) = frames.iter().find(|frame| **frame >= count) {
        bail!("its flipbook shows frame {frame} of a strip of {count}");
    }
    Ok(Flipbook {
        ticks_per_frame,
        frames,
        blend_frames,
    })
}

/// The width and height in a PNG's header.
fn png_size(path: &Path) -> Result<(u32, u32)> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    let mut header = [0; 24];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .with_context(|| format!("reading the header of {}", path.display()))?;
    ensure!(
        header.starts_with(SIGNATURE) && &header[12..16] == b"IHDR",
        "{} is not a PNG",
        path.display()
    );
    let word = |at: usize| u32::from_be_bytes(header[at..at + 4].try_into().expect("4 bytes"));
    Ok((word(16), word(20)))
}

/// A box within the block: finite, at least 0 and at most 16 pixels, and not empty on any axis.
fn pixel_box(what: &str, wit::PixelBox { min, max }: wit::PixelBox) -> Result<PixelBox> {
    let (min, max) = (pixel(min), pixel(max));
    for (axis, low, high) in [
        ("x", min.x, max.x),
        ("y", min.y, max.y),
        ("z", min.z, max.z),
    ] {
        ensure!(
            low.is_finite() && high.is_finite() && 0.0 <= low && low < high && high <= BLOCK_PIXELS,
            "its {what} box spans {low} to {high} on {axis}; it needs 0 ≤ min < max ≤ \
             {BLOCK_PIXELS}"
        );
    }
    Ok(PixelBox { min, max })
}

fn pixel(wit::Pixel { x, y, z }: wit::Pixel) -> Pixel {
    Pixel { x, y, z }
}

fn quarter_turns(wit::QuarterTurns { x, y, z }: wit::QuarterTurns) -> Result<QuarterTurns> {
    ensure!(
        [x, y, z].iter().all(|turns| *turns <= MAX_QUARTER_TURNS),
        "its rotation turns ({x}, {y}, {z}); each axis takes 0 to {MAX_QUARTER_TURNS}"
    );
    Ok(QuarterTurns { x, y, z })
}

impl From<wit::StateValue> for StateValue {
    fn from(value: wit::StateValue) -> Self {
        match value {
            wit::StateValue::Bool(value) => Self::Bool(value),
            wit::StateValue::Choice(value) => Self::Choice(value),
        }
    }
}

impl From<StateValue> for wit::StateValue {
    fn from(value: StateValue) -> Self {
        match value {
            StateValue::Bool(value) => Self::Bool(value),
            StateValue::Choice(value) => Self::Choice(value),
        }
    }
}

impl From<wit::BlockState> for BlockState {
    fn from(wit::BlockState { name, value }: wit::BlockState) -> Self {
        Self {
            name,
            value: value.into(),
        }
    }
}

impl From<BlockState> for wit::BlockState {
    fn from(BlockState { name, value }: BlockState) -> Self {
        Self {
            name,
            value: value.into(),
        }
    }
}
