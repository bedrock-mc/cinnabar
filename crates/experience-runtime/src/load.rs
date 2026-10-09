//! Loading a server artifact: verify it, compile it against the `server` world, run `register`
//! and validate the blocks it declares.

mod block_type;
mod geometry;
mod items;

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail, ensure};
use sha2::{Digest, Sha256};
use wasmtime::component::Component;
use wasmtime::{Config, Engine};
use wit_component::ComponentEncoder;

use crate::hex;
use crate::host::cinnabar::experience_server::types as wit;
use crate::host::{Api, HostState, Pre};
use crate::limits::{
    EPOCH_PERIOD, MAX_BLOCK_NAME_BYTES, MAX_BLOCKS, MAX_COMPONENT_BYTES, MAX_DISPLAY_NAME_BYTES,
    MAX_NAME_BYTES, MAX_WASM_STACK_BYTES, REGISTER_DEADLINE, REGISTER_FUEL,
};
use crate::manifest::{ASSETS_DIR, Manifest, SERVER_WASM, read_manifest, resolve};
use crate::protocol::{self, PlacementState, StateDef, StateValues};

pub(crate) use block_type::holds;

/// The texture slot, or material instance, of every face that a block does not bind on its own.
const ALL_FACES: &str = "*";
/// The face texture slots. A block binds each slot at most once, and binds [`ALL_FACES`] unless
/// it binds all of these.
const FACES: [&str; 6] = ["up", "down", "north", "south", "east", "west"];

/// What an Experience's callbacks may name: its blocks with their state axes ([`axes`] of their
/// states), its items with the most one stack of each holds, and the server's items, which the
/// adapter lists when it loads the Experience.
#[derive(Debug, Clone, Default)]
pub(crate) struct Catalog {
    pub blocks: HashMap<String, Vec<StateDef>>,
    pub items: HashMap<String, u8>,
    pub server: HashMap<String, HashMap<u16, protocol::ServerItem>>,
}

/// A verified artifact: its manifest, its validated blocks, and the component pre-linked against
/// the `server` world of its `api`, ready for a fresh instance per callback.
pub struct Loaded {
    pub manifest: Manifest,
    /// Asset paths are absolute.
    pub blocks: Vec<protocol::BlockDef>,
    /// Icon paths are absolute.
    pub items: Vec<protocol::ItemDef>,
    /// What callbacks may name, shared by every callback.
    pub(crate) catalog: Arc<Catalog>,
    /// Whether the world of its `api` takes its player's focus in client messages and epochs.
    pub focus: bool,
    pub(crate) pre: Pre,
}

/// Advances the engine's epoch once per elapsed [`EPOCH_PERIOD`]; dropping it stops and joins
/// the thread.
pub struct EpochTicker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl EpochTicker {
    fn start(engine: Engine) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("epoch-ticker".to_owned())
            .spawn(move || {
                let start = Instant::now();
                let mut ticks = 0;
                while !stopped.load(Ordering::Relaxed) {
                    thread::sleep(EPOCH_PERIOD);
                    // Sleeps overshoot, so the epoch catches up with wall time instead of
                    // counting wake-ups.
                    let due = start.elapsed().as_nanos() / EPOCH_PERIOD.as_nanos();
                    for _ in ticks..due {
                        engine.increment_epoch();
                    }
                    ticks = due;
                }
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for EpochTicker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The engine every Experience runs on: component model, fuel, epoch interruption and the wasm
/// stack limit, plus the ticker that drives its epoch. Traps carry no wasm backtrace: no answer
/// shows one, and its size grows with the guest's stack and its names, so an error is just its
/// context chain down to the root cause.
pub fn engine() -> Result<(Engine, EpochTicker)> {
    let mut config = Config::new();
    config
        .wasm_component_model(true)
        .wasm_backtrace(false)
        .consume_fuel(true)
        .epoch_interruption(true)
        .max_wasm_stack(MAX_WASM_STACK_BYTES);
    let engine = Engine::new(&config)?;
    let ticker = EpochTicker::start(engine.clone()).context("starting the epoch ticker")?;
    Ok((engine, ticker))
}

/// Loads the artifact in `dir`. Errors name the directory and the cause.
pub fn load(engine: &Engine, dir: &Path) -> Result<Loaded> {
    load_dir(engine, dir).with_context(|| format!("loading experience {}", dir.display()))
}

fn load_dir(engine: &Engine, dir: &Path) -> Result<Loaded> {
    let manifest = read_manifest(dir)?;
    let api = Api::of(&manifest.api).expect("read_manifest admits only implemented apis");
    let module = read_module(dir, &manifest)?;
    let pre = link(engine, &module, api)
        .with_context(|| format!("{SERVER_WASM} is not a {} server component", api.package()))?;
    let registration = register(engine, &pre, &manifest.id)?;
    let (blocks, items) = validate_registration(dir, &manifest, registration)?;
    let catalog = Catalog {
        blocks: blocks
            .iter()
            .map(|block| (block.id.clone(), axes(&block.states, &block.placement)))
            .collect(),
        items: items
            .iter()
            .map(|item| (item.id.clone(), item.max_stack))
            .collect(),
        server: HashMap::new(),
    };
    Ok(Loaded {
        manifest,
        blocks,
        items,
        catalog: Arc::new(catalog),
        focus: api.focus(),
        pre,
    })
}

/// Reads `server.wasm`, refusing more than [`MAX_COMPONENT_BYTES`], and checks that the bytes it
/// will compile are the indexed ones.
fn read_module(dir: &Path, manifest: &Manifest) -> Result<Vec<u8>> {
    let mut module = Vec::new();
    File::open(dir.join(SERVER_WASM))
        .and_then(|file| {
            file.take(MAX_COMPONENT_BYTES as u64 + 1)
                .read_to_end(&mut module)
        })
        .with_context(|| format!("reading {SERVER_WASM}"))?;
    ensure!(
        module.len() <= MAX_COMPONENT_BYTES,
        "{SERVER_WASM} exceeds {MAX_COMPONENT_BYTES} bytes"
    );
    let indexed = manifest
        .files
        .get(SERVER_WASM)
        .with_context(|| format!("{SERVER_WASM} is not indexed"))?;
    ensure!(
        hex::encode(&Sha256::digest(&module)) == *indexed,
        "{SERVER_WASM} changed after its hash was verified"
    );
    Ok(module)
}

/// Turns the core module into a component and links it against exactly the imports of `api`'s
/// `server` world; a client world, a WASI import, another version's world or a missing export
/// fails here.
fn link(engine: &Engine, module: &[u8], api: Api) -> Result<Pre> {
    let component = ComponentEncoder::default()
        .module(module)?
        .validate(true)
        .encode()?;
    Pre::link(engine, &Component::new(engine, &component)?, api)
}

/// Runs `register` once on a fresh instance under the register fuel and deadline.
fn register(engine: &Engine, pre: &Pre, id: &str) -> Result<wit::Registration> {
    let mut store = HostState::store(engine, id, REGISTER_FUEL, REGISTER_DEADLINE)?;
    match pre.register(&mut store)? {
        Ok(registration) => Ok(registration),
        Err(wit::GuestError::Rejected(reason) | wit::GuestError::Failed(reason)) => {
            bail!("register failed: {reason}")
        }
    }
}

/// A block's state axes as callbacks and the adapter order them: its placement traits' states,
/// then its own states.
pub(crate) fn axes(states: &[StateDef], placement: &[PlacementState]) -> Vec<StateDef> {
    placement
        .iter()
        .map(|placement| {
            let (name, values) = placement.state();
            StateDef {
                name: name.to_owned(),
                values: StateValues::Choices(
                    values.iter().map(|&value| value.to_owned()).collect(),
                ),
            }
        })
        .chain(states.iter().cloned())
        .collect()
}

impl Loaded {
    /// The loaded artifact whose callbacks may make stacks of the server's `items` besides its
    /// own, as the adapter lists them at load.
    pub fn with_server_items(mut self, items: Vec<protocol::ServerItem>) -> Result<Self> {
        let mut catalog = (*self.catalog).clone();
        catalog.server = items::server_items(items)?;
        self.catalog = Arc::new(catalog);
        Ok(self)
    }
}

/// Checks what `register` declared, its blocks and then its items; asset paths become absolute
/// under the artifact directory.
fn validate_registration(
    dir: &Path,
    manifest: &Manifest,
    registration: wit::Registration,
) -> Result<(Vec<protocol::BlockDef>, Vec<protocol::ItemDef>)> {
    let blocks = validate_blocks(dir, manifest, registration.blocks)?;
    let root = std::path::absolute(dir).context("resolving the artifact directory")?;
    let items = items::validate_items(
        &root,
        &manifest.files,
        &manifest.id,
        &blocks,
        registration.items,
    )?;
    Ok((blocks, items))
}

/// Checks every declared block; asset paths become absolute under the artifact directory.
fn validate_blocks(
    dir: &Path,
    manifest: &Manifest,
    types: Vec<wit::BlockType>,
) -> Result<Vec<protocol::BlockDef>> {
    ensure!(
        types.len() <= MAX_BLOCKS,
        "register declared {} blocks; the limit is {MAX_BLOCKS}",
        types.len()
    );
    let root = std::path::absolute(dir).context("resolving the artifact directory")?;
    let namespace = format!("{}:", manifest.id);
    let mut assets = block_type::Assets {
        root: &root,
        files: &manifest.files,
        id: &manifest.id,
        geometries: HashMap::new(),
    };
    let mut blocks: Vec<protocol::BlockDef> = Vec::with_capacity(types.len());
    for block in types {
        let id = &block.def.id;
        let Some(name) = id.strip_prefix(&namespace) else {
            bail!("block \"{id}\" is outside namespace \"{namespace}\"");
        };
        ensure!(
            is_block_name(name),
            "block \"{id}\" has an invalid name: the part after \"{namespace}\" must match \
             ^[a-z0-9_]{{1,{MAX_BLOCK_NAME_BYTES}}}$"
        );
        ensure!(
            blocks.iter().all(|block| block.id != *id),
            "block \"{id}\" is declared twice"
        );
        let context = format!("block \"{id}\"");
        blocks.push(validate_block(&mut assets, block).context(context)?);
    }
    Ok(blocks)
}

/// `^[a-z0-9_]{1,MAX_BLOCK_NAME_BYTES}$`, the part of a block id after `<id>:`.
fn is_block_name(name: &str) -> bool {
    (1..=MAX_BLOCK_NAME_BYTES).contains(&name.len()) && is_lower_snake(name)
}

/// `^[a-z0-9_]{1,MAX_NAME_BYTES}$`: a state's name after `<id>:`, a string state value, a
/// material instance that a geometry names, or a geometry's name.
fn is_name(name: &str) -> bool {
    (1..=MAX_NAME_BYTES).contains(&name.len()) && is_lower_snake(name)
}

fn is_lower_snake(name: &str) -> bool {
    name.bytes()
        .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

/// Checks one block type: its display name and mining, its states and placement traits, and
/// either textures on the full cube or a visual with its permutations.
fn validate_block(
    assets: &mut block_type::Assets<'_>,
    block: wit::BlockType,
) -> Result<protocol::BlockDef> {
    let wit::BlockType {
        def,
        states,
        placement,
        visual,
        permutations,
        network,
    } = block;
    let wit::BlockDef {
        id,
        display_name,
        textures,
        mining,
    } = def;
    check_display_name(&display_name)?;
    let mining = match mining {
        wit::Mining::Unbreakable => protocol::Mining::Unbreakable {},
        wit::Mining::Breakable(hardness) => {
            ensure!(
                hardness.is_finite() && hardness >= 0.0,
                "hardness {hardness} is not a finite number ≥ 0"
            );
            protocol::Mining::Breakable { hardness }
        }
    };
    let (states, placement) = block_type::states(assets.id, states, placement)?;
    let axes = axes(&states, &placement);
    let (textures, visual, permutations) = match visual {
        Some(visual) => {
            ensure!(
                textures.is_empty(),
                "it binds textures and declares a visual, whose materials replace them"
            );
            let look = block_type::visual(assets, &axes, visual)?;
            let permutations = block_type::permutations(assets, &axes, &look, permutations)?;
            (Vec::new(), Some(look.visual), permutations)
        }
        None => {
            ensure!(
                permutations.is_empty(),
                "it declares permutations but no visual for them to change"
            );
            let textures = validate_textures(assets.root, assets.files, textures)?;
            (textures, None, Vec::new())
        }
    };
    Ok(protocol::BlockDef {
        id,
        display_name,
        textures,
        mining,
        states,
        placement,
        visual,
        permutations,
        network,
    })
}

/// Checks a block's or an item's display name: 1 to [`MAX_DISPLAY_NAME_BYTES`] bytes without a
/// control character.
fn check_display_name(display_name: &str) -> Result<()> {
    ensure!(
        (1..=MAX_DISPLAY_NAME_BYTES).contains(&display_name.len()),
        "display name has {} bytes; it needs 1 to {MAX_DISPLAY_NAME_BYTES}",
        display_name.len()
    );
    ensure!(
        !display_name.chars().any(char::is_control),
        "display name {display_name:?} has a control character"
    );
    Ok(())
}

/// Checks a full cube's texture bindings: each slot once, covering every face.
fn validate_textures(
    root: &Path,
    files: &BTreeMap<String, String>,
    bindings: Vec<wit::TextureBinding>,
) -> Result<Vec<protocol::Texture>> {
    let mut textures: Vec<protocol::Texture> = Vec::new();
    for wit::TextureBinding { slot, path } in bindings {
        ensure!(
            slot == ALL_FACES || FACES.contains(&slot.as_str()),
            "texture slot \"{slot}\" is neither \"{ALL_FACES}\" nor one of {FACES:?}"
        );
        ensure!(
            textures.iter().all(|texture| texture.slot != slot),
            "texture slot \"{slot}\" is bound twice"
        );
        let path = asset_path(root, files, &path).context("texture")?;
        textures.push(protocol::Texture { slot, path });
    }
    let bound = |slot: &str| textures.iter().any(|texture| texture.slot == slot);
    ensure!(
        bound(ALL_FACES) || FACES.into_iter().all(bound),
        "textures bind neither \"{ALL_FACES}\" nor all of {FACES:?}"
    );
    Ok(textures)
}

/// The absolute path of `path`, a file under `assets/` that the manifest indexes.
fn asset_path(root: &Path, files: &BTreeMap<String, String>, path: &str) -> Result<String> {
    // Index keys are already confined to the artifact, so a listed key is safe to join.
    let indexed = format!("{ASSETS_DIR}/{path}");
    ensure!(
        files.contains_key(&indexed),
        "\"{path}\" is not an indexed file: [files] has no \"{indexed}\""
    );
    resolve(root, &indexed)
        .into_os_string()
        .into_string()
        .map_err(|path| anyhow!("{} is not a UTF-8 path", path.display()))
}

#[cfg(test)]
mod tests;
