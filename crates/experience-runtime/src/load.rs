//! Loading a server artifact: verify it, compile it against the `server` world, run `register`
//! and validate the blocks it declares.

use std::collections::BTreeMap;
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
    MAX_WASM_STACK_BYTES, REGISTER_DEADLINE, REGISTER_FUEL,
};
use crate::manifest::{ASSETS_DIR, Manifest, SERVER_WASM, read_manifest, resolve};
use crate::protocol;

/// The texture slot for every face that a block does not bind on its own.
const ALL_FACES: &str = "*";
/// The face texture slots. A block binds each slot at most once, and binds [`ALL_FACES`] unless
/// it binds all of these.
const FACES: [&str; 6] = ["up", "down", "north", "south", "east", "west"];

/// A verified artifact: its manifest, its validated blocks, and the component pre-linked against
/// the `server` world of its `api`, ready for a fresh instance per callback.
pub struct Loaded {
    pub manifest: Manifest,
    /// Texture paths are absolute.
    pub blocks: Vec<protocol::BlockDef>,
    /// The ids of `blocks`, shared by every callback.
    pub(crate) block_ids: Arc<[String]>,
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
    let defs = register(engine, &pre, &manifest.id)?;
    let blocks = validate_blocks(dir, &manifest, defs)?;
    let block_ids = blocks.iter().map(|block| block.id.clone()).collect();
    Ok(Loaded {
        manifest,
        blocks,
        block_ids,
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
fn register(engine: &Engine, pre: &Pre, id: &str) -> Result<Vec<wit::BlockDef>> {
    let mut store = HostState::store(engine, id, REGISTER_FUEL, REGISTER_DEADLINE)?;
    match pre.register(&mut store)? {
        Ok(defs) => Ok(defs),
        Err(wit::GuestError::Rejected(reason) | wit::GuestError::Failed(reason)) => {
            bail!("register failed: {reason}")
        }
    }
}

/// Checks every declared block; texture paths become absolute under the artifact directory.
fn validate_blocks(
    dir: &Path,
    manifest: &Manifest,
    defs: Vec<wit::BlockDef>,
) -> Result<Vec<protocol::BlockDef>> {
    ensure!(
        defs.len() <= MAX_BLOCKS,
        "register declared {} blocks; the limit is {MAX_BLOCKS}",
        defs.len()
    );
    let root = std::path::absolute(dir).context("resolving the artifact directory")?;
    let namespace = format!("{}:", manifest.id);
    let mut blocks: Vec<protocol::BlockDef> = Vec::with_capacity(defs.len());
    for def in defs {
        let wit::BlockDef {
            id,
            display_name,
            textures,
            mining,
        } = def;
        let Some(name) = id.strip_prefix(&namespace) else {
            bail!("block \"{id}\" is outside namespace \"{namespace}\"");
        };
        ensure!(
            is_block_name(name),
            "block \"{id}\" has an invalid name: the part after \"{namespace}\" must match \
             ^[a-z0-9_]{{1,{MAX_BLOCK_NAME_BYTES}}}$"
        );
        ensure!(
            blocks.iter().all(|block| block.id != id),
            "block \"{id}\" is declared twice"
        );
        let (textures, mining) =
            validate_block(&root, &manifest.files, &display_name, textures, mining)
                .with_context(|| format!("block \"{id}\""))?;
        blocks.push(protocol::BlockDef {
            id,
            display_name,
            textures,
            mining,
        });
    }
    Ok(blocks)
}

/// `^[a-z0-9_]{1,MAX_BLOCK_NAME_BYTES}$`, the part of a block id after `<id>:`.
fn is_block_name(name: &str) -> bool {
    (1..=MAX_BLOCK_NAME_BYTES).contains(&name.len())
        && name
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

/// Checks one block's display name, texture bindings and mining.
fn validate_block(
    root: &Path,
    files: &BTreeMap<String, String>,
    display_name: &str,
    bindings: Vec<wit::TextureBinding>,
    mining: wit::Mining,
) -> Result<(Vec<protocol::Texture>, protocol::Mining)> {
    ensure!(
        (1..=MAX_DISPLAY_NAME_BYTES).contains(&display_name.len()),
        "display name has {} bytes; it needs 1 to {MAX_DISPLAY_NAME_BYTES}",
        display_name.len()
    );
    ensure!(
        !display_name.chars().any(char::is_control),
        "display name {display_name:?} has a control character"
    );
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
        // Index keys are already confined to the artifact, so a listed key is safe to join.
        let indexed = format!("{ASSETS_DIR}/{path}");
        ensure!(
            files.contains_key(&indexed),
            "texture \"{path}\" is not an indexed file: [files] has no \"{indexed}\""
        );
        let path = resolve(root, &indexed)
            .into_os_string()
            .into_string()
            .map_err(|path| anyhow!("{} is not a UTF-8 path", path.display()))?;
        textures.push(protocol::Texture { slot, path });
    }
    let bound = |slot: &str| textures.iter().any(|texture| texture.slot == slot);
    ensure!(
        bound(ALL_FACES) || FACES.into_iter().all(bound),
        "textures bind neither \"{ALL_FACES}\" nor all of {FACES:?}"
    );
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
    Ok((textures, mining))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use wasmtime::Trap;
    use wasmtime::component::{Component, Linker};

    use super::{engine, validate_blocks, wit};
    use crate::host::{Api, HostState};
    use crate::limits::{MAX_BLOCK_NAME_BYTES, MAX_BLOCKS, MAX_DISPLAY_NAME_BYTES};
    use crate::manifest::{ASSETS_DIR, DATA_SCHEMA, Manifest, SERVER_WASM};

    /// The ticker keeps the epoch on wall time, so a store with fuel to spare still stops at its
    /// deadline, and not long before or after it.
    #[test]
    fn epoch_deadline_stops_a_store_with_fuel_left() {
        const DEADLINE: Duration = Duration::from_millis(200);
        // Seconds of spinning: a stalled epoch ends in a fuel trap instead of a hang.
        const FUEL: u64 = 10_000_000_000;
        let (engine, _ticker) = engine().unwrap();
        let spin = r#"(component
            (core module $m (func (export "spin") (loop (br 0))))
            (core instance $i (instantiate $m))
            (func (export "spin") (canon lift (core func $i "spin"))))"#;
        let component = Component::new(&engine, spin).unwrap();
        let mut store = HostState::store(&engine, "spin", FUEL, DEADLINE).unwrap();
        let instance = Linker::new(&engine)
            .instantiate(&mut store, &component)
            .unwrap();
        let spin = instance
            .get_typed_func::<(), ()>(&mut store, "spin")
            .unwrap();
        let start = Instant::now();
        let error = spin.call(&mut store, ()).unwrap_err();
        let elapsed = start.elapsed();
        assert_eq!(
            error.downcast_ref::<Trap>(),
            Some(&Trap::Interrupt),
            "{error:#}"
        );
        assert!(
            elapsed >= DEADLINE / 2 && elapsed < DEADLINE * 5,
            "stopped after {elapsed:?}"
        );
    }

    fn binding(slot: &str) -> wit::TextureBinding {
        wit::TextureBinding {
            slot: slot.to_owned(),
            path: "counter.png".to_owned(),
        }
    }

    /// A valid block: `probe:<name>` with the indexed texture bound to `*`.
    fn block(name: &str) -> wit::BlockDef {
        wit::BlockDef {
            id: format!("probe:{name}"),
            display_name: "Probe Counter".to_owned(),
            textures: vec![binding("*")],
            mining: wit::Mining::Breakable(1.0),
        }
    }

    /// `count` valid blocks with distinct names.
    fn blocks(count: usize) -> Vec<wit::BlockDef> {
        (0..count).map(|i| block(&format!("b{i}"))).collect()
    }

    /// The valid block `probe:counter` after `edit`.
    fn counter(edit: impl FnOnce(&mut wit::BlockDef)) -> Vec<wit::BlockDef> {
        let mut def = block("counter");
        edit(&mut def);
        vec![def]
    }

    /// Each row changes a valid declaration in one way. `None` means it is still accepted;
    /// otherwise the refusal must contain the cause and, for a single block, name that block.
    #[test]
    fn block_rules_accept_limits_and_refuse_violations() {
        let faces = || ["up", "down", "north", "south", "east", "west"].map(binding);
        let cases: Vec<(&str, Vec<wit::BlockDef>, Option<&str>)> = vec![
            ("the baseline", counter(|_| {}), None),
            ("MAX_BLOCKS blocks", blocks(MAX_BLOCKS), None),
            (
                "MAX_BLOCKS + 1 blocks",
                blocks(MAX_BLOCKS + 1),
                Some("the limit is"),
            ),
            (
                "a duplicate id",
                vec![block("counter"), block("counter")],
                Some("declared twice"),
            ),
            (
                "a foreign namespace",
                counter(|def| def.id = "other:counter".to_owned()),
                Some("outside namespace"),
            ),
            ("a name of digits and _", vec![block("cell_64k")], None),
            (
                "a name of MAX_BLOCK_NAME_BYTES",
                vec![block(&"a".repeat(MAX_BLOCK_NAME_BYTES))],
                None,
            ),
            (
                "a name of MAX_BLOCK_NAME_BYTES + 1",
                vec![block(&"a".repeat(MAX_BLOCK_NAME_BYTES + 1))],
                Some("invalid name"),
            ),
            ("an empty name", vec![block("")], Some("invalid name")),
            ("a name with /", vec![block("a/b")], Some("invalid name")),
            ("the name ..", vec![block("..")], Some("invalid name")),
            (
                "an uppercase name",
                vec![block("Counter")],
                Some("invalid name"),
            ),
            (
                "an empty display name",
                counter(|def| def.display_name.clear()),
                Some("display name has"),
            ),
            (
                "a display name of MAX_DISPLAY_NAME_BYTES",
                counter(|def| def.display_name = "a".repeat(MAX_DISPLAY_NAME_BYTES)),
                None,
            ),
            (
                "a display name of MAX_DISPLAY_NAME_BYTES + 1",
                counter(|def| def.display_name = "a".repeat(MAX_DISPLAY_NAME_BYTES + 1)),
                Some("display name has"),
            ),
            (
                "a newline in the display name",
                counter(|def| def.display_name = "Probe\nCounter".to_owned()),
                Some("control character"),
            ),
            (
                "the slot top",
                counter(|def| def.textures.push(binding("top"))),
                Some("slot \"top\""),
            ),
            (
                "a repeated slot",
                counter(|def| def.textures.push(binding("*"))),
                Some("bound twice"),
            ),
            (
                "an unindexed texture",
                counter(|def| def.textures[0].path = "missing.png".to_owned()),
                Some("not an indexed file"),
            ),
            (
                "a texture outside assets/",
                counter(|def| def.textures[0].path = "../server.wasm".to_owned()),
                Some("not an indexed file"),
            ),
            (
                "no textures",
                counter(|def| def.textures.clear()),
                Some("bind neither"),
            ),
            (
                "five faces without *",
                counter(|def| def.textures = faces().into_iter().take(5).collect()),
                Some("bind neither"),
            ),
            (
                "six faces without *",
                counter(|def| def.textures = faces().into()),
                None,
            ),
            (
                "* and a face",
                counter(|def| def.textures.push(binding("up"))),
                None,
            ),
            (
                "hardness 0",
                counter(|def| def.mining = wit::Mining::Breakable(0.0)),
                None,
            ),
            (
                "unbreakable",
                counter(|def| def.mining = wit::Mining::Unbreakable),
                None,
            ),
            (
                "hardness NaN",
                counter(|def| def.mining = wit::Mining::Breakable(f32::NAN)),
                Some("not a finite number"),
            ),
            (
                "hardness inf",
                counter(|def| def.mining = wit::Mining::Breakable(f32::INFINITY)),
                Some("not a finite number"),
            ),
            (
                "hardness -1",
                counter(|def| def.mining = wit::Mining::Breakable(-1.0)),
                Some("not a finite number"),
            ),
        ];
        let dir = tempfile::tempdir().unwrap();
        // `validate_blocks` consults only the index's paths, not the files or their hashes.
        let manifest = Manifest {
            id: "probe".to_owned(),
            version: "0.1.0".to_owned(),
            api: Api::V0_2.api().to_owned(),
            data_schema: DATA_SCHEMA,
            files: [format!("{ASSETS_DIR}/counter.png"), SERVER_WASM.to_owned()]
                .into_iter()
                .map(|path| (path, "0".repeat(64)))
                .collect(),
        };
        let mut failures = Vec::new();
        for (case, defs, refusal) in cases {
            let culprit = match defs.as_slice() {
                [def] => Some(format!("block \"{}\"", def.id)),
                _ => None,
            };
            let outcome = validate_blocks(dir.path(), &manifest, defs);
            match (outcome.map_err(|error| format!("{error:#}")), refusal) {
                (Ok(_), None) => {}
                (Ok(_), Some(cause)) => failures.push(format!("{case}: accepted, not {cause:?}")),
                (Err(error), None) => failures.push(format!("{case}: refused: {error}")),
                (Err(error), Some(cause)) => {
                    let named = culprit.is_none_or(|culprit| error.contains(&culprit));
                    if !named || !error.contains(cause) {
                        failures.push(format!("{case}: {error:?} lacks the block or {cause:?}"));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }
}
