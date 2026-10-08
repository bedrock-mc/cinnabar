//! Builds guest crates for wasm32 and assembles them into temporary server artifacts, and builds
//! the probe's callback requests. The probe selects a behavior by the interacted block's x; `p(x)`
//! is that block and `up(x)` the one above.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use experience_runtime::callback::run;
use experience_runtime::load::{EpochTicker, Loaded, engine, load};
use experience_runtime::manifest::{ASSETS_DIR, MANIFEST_FILE, SERVER_WASM};
use experience_runtime::protocol::{
    BlockPos, Call, Cell, Face, Info, Op, Outcome, Request, Scalar,
};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use wasmtime::Engine;
use wit_component::StringEncoding;
use wit_component::metadata::Bindgen;

pub const ACTOR: &str = "3f2a7c1e-8b4d-4e6a-9c5f-1d2e3f4a5b6c";
pub const COUNTER: &str = "probe:counter";
pub const AIR: &str = "minecraft:air";

const WASM_TARGET: &str = "wasm32-unknown-unknown";

/// The WIT that guests are built against.
const SERVER_WIT: &str = include_str!("../../../../experience-sdk/wit/server/server.wit");
/// The server WIT 0.1, which the runtime still accepts.
const SERVER_WIT_0_1: &str = include_str!("../../../wit/0.1/server.wit");
/// The server WIT 0.2, which the runtime still accepts.
const SERVER_WIT_0_2: &str = include_str!("../../../wit/0.2/server.wit");
/// The server WIT 0.3, which the runtime still accepts.
const SERVER_WIT_0_3: &str = include_str!("../../../wit/0.3/server.wit");

/// A core module for the `server` world whose `register` spins forever and whose callbacks trap.
/// Each export takes the canonical ABI's flattening of its WIT signature, and returns a pointer
/// to its result.
const LOOPING_REGISTER: &str = r#"(module
    (memory (export "memory") 1)
    (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) unreachable)
    (func (export "register") (result i32)
        (loop $spin (br $spin))
        unreachable)
    (func (export "on-place")
        (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
        unreachable)
    (func (export "on-break")
        (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
        unreachable)
    (func (export "on-interact") (param i32 i32 i32 i32 i32 i32 i32) (result i32) unreachable)
    (func (export "on-neighbor-changed") (param i32 i32 i32 i32 i32 i32 i32) (result i32)
        unreachable)
    (func (export "client-message") (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
        unreachable)
    (func (export "epoch") (param i32 i32 i32) (result i32) unreachable))"#;

/// A core module for the 0.1 `server` world, as a guest built before 0.2 would be. `register`
/// declares `probe:counter`, breakable with hardness 1, whose `*` texture is `counter.png`, and
/// `on-interact` tells the player "v0.1" through the 0.1 `tell`; the other callbacks do nothing.
/// Each callback drops its borrowed `callback` before it returns, as the canonical ABI requires.
/// Results live at fixed addresses: an `ok` without payload at 0, the tell's at 16, register's
/// at 32 pointing at the block at 64, whose texture is at 128 and strings from 256 on.
const V0_1_GUEST: &str = r#"(module
    (import "cinnabar:experience-server/world-access@0.1.0" "[method]callback.tell"
        (func $tell (param i32 i32 i32 i32 i32 i32)))
    (import "cinnabar:experience-server/world-access@0.1.0" "[resource-drop]callback"
        (func $drop (param i32)))
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 1024))
    (data (i32.const 32) "\00\00\00\00\40\00\00\00\01\00\00\00")
    (data (i32.const 64) "\00\01\00\00\0d\00\00\00\10\01\00\00\06\00\00\00")
    (data (i32.const 80) "\80\00\00\00\01\00\00\00\01\00\00\00\00\00\80\3f")
    (data (i32.const 128) "\20\01\00\00\01\00\00\00\28\01\00\00\0b\00\00\00")
    (data (i32.const 256) "probe:counter")
    (data (i32.const 272) "Legacy")
    (data (i32.const 288) "*")
    (data (i32.const 296) "counter.png")
    (data (i32.const 320) "v0.1")
    (func (export "cabi_realloc") (param i32 i32) (param $align i32) (param $size i32)
        (result i32)
        (local $at i32)
        (local.set $at (i32.and
            (i32.add (global.get $heap) (i32.sub (local.get $align) (i32.const 1)))
            (i32.sub (i32.const 0) (local.get $align))))
        (global.set $heap (i32.add (local.get $at) (local.get $size)))
        (local.get $at))
    (func (export "register") (result i32) (i32.const 32))
    (func (export "on-place")
        (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
        (call $drop (local.get 0))
        (i32.const 0))
    (func (export "on-break")
        (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
        (call $drop (local.get 0))
        (i32.const 0))
    (func (export "on-interact")
        (param $ctx i32) (param $player i32) (param $len i32) (param i32 i32 i32 i32) (result i32)
        (call $tell (local.get $ctx) (local.get $player) (local.get $len)
            (i32.const 320) (i32.const 4) (i32.const 16))
        (call $drop (local.get $ctx))
        (i32.const 0))
    (func (export "on-neighbor-changed") (param i32 i32 i32 i32 i32 i32 i32) (result i32)
        (call $drop (local.get 0))
        (i32.const 0)))"#;

/// A core module for the 0.2 `server` world, as a guest built before 0.3 would be. Its `register`
/// and block callbacks are [`V0_1_GUEST`]'s, except that `on-interact` tells "v0.2" through the
/// 0.2 `tell`, and `client-message` echoes the message through the 0.2 `send-client`: the same
/// channel, schema and scalars, to the sender.
const V0_2_GUEST: &str = r#"(module
    (import "cinnabar:experience-server/world-access@0.2.0" "[method]callback.tell"
        (func $tell (param i32 i32 i32 i32 i32 i32)))
    (import "cinnabar:experience-server/world-access@0.2.0" "[method]callback.send-client"
        (func $send (param i32 i32 i32 i32 i32 i32 i32 i32 i32)))
    (import "cinnabar:experience-server/world-access@0.2.0" "[resource-drop]callback"
        (func $drop (param i32)))
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 1024))
    (data (i32.const 32) "\00\00\00\00\40\00\00\00\01\00\00\00")
    (data (i32.const 64) "\00\01\00\00\0d\00\00\00\10\01\00\00\06\00\00\00")
    (data (i32.const 80) "\80\00\00\00\01\00\00\00\01\00\00\00\00\00\80\3f")
    (data (i32.const 128) "\20\01\00\00\01\00\00\00\28\01\00\00\0b\00\00\00")
    (data (i32.const 256) "probe:counter")
    (data (i32.const 272) "Legacy")
    (data (i32.const 288) "*")
    (data (i32.const 296) "counter.png")
    (data (i32.const 320) "v0.2")
    (func (export "cabi_realloc") (param i32 i32) (param $align i32) (param $size i32)
        (result i32)
        (local $at i32)
        (local.set $at (i32.and
            (i32.add (global.get $heap) (i32.sub (local.get $align) (i32.const 1)))
            (i32.sub (i32.const 0) (local.get $align))))
        (global.set $heap (i32.add (local.get $at) (local.get $size)))
        (local.get $at))
    (func (export "register") (result i32) (i32.const 32))
    (func (export "on-place")
        (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
        (call $drop (local.get 0))
        (i32.const 0))
    (func (export "on-break")
        (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
        (call $drop (local.get 0))
        (i32.const 0))
    (func (export "on-interact")
        (param $ctx i32) (param $player i32) (param $len i32) (param i32 i32 i32 i32) (result i32)
        (call $tell (local.get $ctx) (local.get $player) (local.get $len)
            (i32.const 320) (i32.const 4) (i32.const 16))
        (call $drop (local.get $ctx))
        (i32.const 0))
    (func (export "on-neighbor-changed") (param i32 i32 i32 i32 i32 i32 i32) (result i32)
        (call $drop (local.get 0))
        (i32.const 0))
    (func (export "client-message")
        (param $ctx i32) (param i32 i32 i32 i32 i32 i32 i32) (result i32)
        (call $send (local.get $ctx) (local.get 1) (local.get 2) (local.get 3) (local.get 4)
            (local.get 5) (local.get 6) (local.get 7) (i32.const 16))
        (call $drop (local.get $ctx))
        (i32.const 0)))"#;

/// A core module for the 0.3 `server` world, as a guest built before 0.4 would be. It is
/// [`V0_2_GUEST`] through the 0.3 imports, telling "v0.3": `client-message` echoes the message's
/// nodes, lists and records included, and `epoch` does nothing.
const V0_3_GUEST: &str = r#"(module
    (import "cinnabar:experience-server/world-access@0.3.0" "[method]callback.tell"
        (func $tell (param i32 i32 i32 i32 i32 i32)))
    (import "cinnabar:experience-server/world-access@0.3.0" "[method]callback.send-client"
        (func $send (param i32 i32 i32 i32 i32 i32 i32 i32 i32)))
    (import "cinnabar:experience-server/world-access@0.3.0" "[resource-drop]callback"
        (func $drop (param i32)))
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 1024))
    (data (i32.const 32) "\00\00\00\00\40\00\00\00\01\00\00\00")
    (data (i32.const 64) "\00\01\00\00\0d\00\00\00\10\01\00\00\06\00\00\00")
    (data (i32.const 80) "\80\00\00\00\01\00\00\00\01\00\00\00\00\00\80\3f")
    (data (i32.const 128) "\20\01\00\00\01\00\00\00\28\01\00\00\0b\00\00\00")
    (data (i32.const 256) "probe:counter")
    (data (i32.const 272) "Legacy")
    (data (i32.const 288) "*")
    (data (i32.const 296) "counter.png")
    (data (i32.const 320) "v0.3")
    (func (export "cabi_realloc") (param i32 i32) (param $align i32) (param $size i32)
        (result i32)
        (local $at i32)
        (local.set $at (i32.and
            (i32.add (global.get $heap) (i32.sub (local.get $align) (i32.const 1)))
            (i32.sub (i32.const 0) (local.get $align))))
        (global.set $heap (i32.add (local.get $at) (local.get $size)))
        (local.get $at))
    (func (export "register") (result i32) (i32.const 32))
    (func (export "on-place")
        (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
        (call $drop (local.get 0))
        (i32.const 0))
    (func (export "on-break")
        (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
        (call $drop (local.get 0))
        (i32.const 0))
    (func (export "on-interact")
        (param $ctx i32) (param $player i32) (param $len i32) (param i32 i32 i32 i32) (result i32)
        (call $tell (local.get $ctx) (local.get $player) (local.get $len)
            (i32.const 320) (i32.const 4) (i32.const 16))
        (call $drop (local.get $ctx))
        (i32.const 0))
    (func (export "on-neighbor-changed") (param i32 i32 i32 i32 i32 i32 i32) (result i32)
        (call $drop (local.get 0))
        (i32.const 0))
    (func (export "client-message")
        (param $ctx i32) (param i32 i32 i32 i32 i32 i32 i32) (result i32)
        (call $send (local.get $ctx) (local.get 1) (local.get 2) (local.get 3) (local.get 4)
            (local.get 5) (local.get 6) (local.get 7) (i32.const 16))
        (call $drop (local.get $ctx))
        (i32.const 0))
    (func (export "epoch") (param i32 i32 i32) (result i32)
        (call $drop (local.get 0))
        (i32.const 0)))"#;

/// The probe's `assets/counter.png`: a 1×1 opaque RGBA PNG.
const COUNTER_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x68, 0x68, 0x68, 0xf8,
    0x0f, 0x00, 0x05, 0x84, 0x02, 0x80, 0x53, 0x93, 0x74, 0x36, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate lives two levels below the workspace root")
}

/// The Cargo target directory of this test binary, `<target>/<profile>/deps/<test>`.
fn target_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("test binary path");
    exe.ancestors()
        .nth(3)
        .expect("test binary lives in <target>/<profile>/deps")
        .to_owned()
}

/// `name` keyed by this worktree. Cargo judges freshness by modification time alone and its
/// dep-info paths are relative to the workspace, so two worktrees building into one target would
/// silently reuse each other's guests; keying the guests' target directory by worktree keeps
/// each worktree's guests its own.
fn worktree_dir(name: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    env!("CARGO_MANIFEST_DIR").hash(&mut hasher);
    format!("{name}-{:016x}", hasher.finish())
}

/// Builds `package` for wasm32 and reads its `.wasm` while holding a cross-process lock. Cargo
/// may replace its output on another build, so tests cache bytes instead of a mutable path.
/// The nested target directory avoids the lock held by the running `cargo test` and stays
/// separate from the Go adapter's guest builds, which do not take this fixture lock.
fn build_guest(package: &str) -> Vec<u8> {
    let target = target_dir().join(worktree_dir("experience-runtime-guests"));
    fs::create_dir_all(&target).expect("create guest target directory");
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(target.join("fixture.lock"))
        .expect("open guest fixture lock");
    lock.lock().expect("lock guest fixture build and read");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .current_dir(workspace_root())
        .args(["build", "--locked", "--target", WASM_TARGET, "-p", package])
        .arg("--target-dir")
        .arg(&target)
        .output()
        .unwrap_or_else(|e| panic!("running cargo for {package}: {e}"));
    assert!(
        output.status.success(),
        "building {package} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let file = format!("{}.wasm", package.replace('-', "_"));
    fs::read(target.join(WASM_TARGET).join("debug").join(file)).expect("read built guest")
}

/// The probe guest's core module, built once per test binary.
pub fn probe_wasm() -> &'static [u8] {
    static WASM: LazyLock<Vec<u8>> = LazyLock::new(|| build_guest("experience-probe"));
    &WASM
}

/// The client `hello-mod` guest's core module, built once per test binary.
pub fn hello_wasm() -> &'static [u8] {
    static WASM: LazyLock<Vec<u8>> = LazyLock::new(|| build_guest("hello-mod"));
    &WASM
}

/// A fresh probe artifact: the probe's `experience.toml` with real hashes, `server.wasm` and
/// `assets/counter.png`. It is deleted when the returned guard drops.
pub fn probe_dir() -> TempDir {
    probe_dir_with(|_| {})
}

/// A fresh probe artifact after `edit` ran on it. `edit` calls [`rehash`] when the index should
/// follow its changes.
pub fn probe_dir_with(edit: impl FnOnce(&Path)) -> TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    let probe = workspace_root().join("examples/experiences/probe");
    fs::copy(probe.join(MANIFEST_FILE), root.join(MANIFEST_FILE)).expect("copy manifest");
    fs::write(root.join(SERVER_WASM), probe_wasm()).expect("write server.wasm");
    fs::create_dir(root.join(ASSETS_DIR)).expect("create assets/");
    fs::write(root.join(ASSETS_DIR).join("counter.png"), COUNTER_PNG).expect("write counter.png");
    rehash(root);
    edit(root);
    dir
}

/// Rewrites `[files]` with the SHA-256 of every file except the manifest.
pub fn rehash(dir: &Path) {
    let files: toml::Table = relative_files(dir)
        .into_iter()
        .filter(|path| path != MANIFEST_FILE)
        .map(|path| {
            let hash = format!("{:x}", Sha256::digest(fs::read(dir.join(&path)).unwrap()));
            (path, toml::Value::String(hash))
        })
        .collect();
    edit_manifest(dir, |manifest| {
        manifest.insert("files".to_owned(), toml::Value::Table(files));
    });
}

/// Applies `edit` to the parsed `experience.toml` and writes it back.
pub fn edit_manifest(dir: &Path, edit: impl FnOnce(&mut toml::Table)) {
    let path = dir.join(MANIFEST_FILE);
    let mut manifest: toml::Table = fs::read_to_string(&path)
        .expect("read manifest")
        .parse()
        .expect("parse manifest");
    edit(&mut manifest);
    fs::write(&path, toml::to_string(&manifest).unwrap()).expect("write manifest");
}

/// A probe artifact whose `server.wasm` is [`LOOPING_REGISTER`] with the `server` world
/// embedded, the way wit-bindgen embeds it in a guest.
pub fn looping_register_dir() -> TempDir {
    wat_dir(LOOPING_REGISTER, SERVER_WIT)
}

/// A probe artifact whose `server.wasm` is [`V0_1_GUEST`] with the 0.1 `server` world embedded,
/// and whose manifest has `api = "0.1"`.
pub fn v0_1_dir() -> TempDir {
    wat_dir(V0_1_GUEST, SERVER_WIT_0_1)
}

/// A probe artifact whose `server.wasm` is [`V0_2_GUEST`] with the 0.2 `server` world embedded,
/// and whose manifest has `api = "0.2"`.
pub fn v0_2_dir() -> TempDir {
    wat_dir(V0_2_GUEST, SERVER_WIT_0_2)
}

/// A probe artifact whose `server.wasm` is [`V0_3_GUEST`] with the 0.3 `server` world embedded,
/// and whose manifest has `api = "0.3"`.
pub fn v0_3_dir() -> TempDir {
    wat_dir(V0_3_GUEST, SERVER_WIT_0_3)
}

/// The manifest `api` of a server WIT: its package's `major.minor`.
fn api_of(wit: &str) -> &str {
    let package = wit
        .lines()
        .find_map(|line| line.strip_prefix("package ")?.strip_suffix(';'))
        .expect("server.wit declares its package");
    let (_, version) = package.rsplit_once('@').expect("the package is versioned");
    version.rsplit_once('.').map_or(version, |(api, _)| api)
}

/// The manifest `api` of the current server WIT, which the probe targets.
pub fn current_api() -> &'static str {
    api_of(SERVER_WIT)
}

/// A probe artifact whose `server.wasm` is the module `wat` with the `server` world of `wit`
/// embedded, and whose manifest has that world's `api`.
fn wat_dir(wat: &str, wit: &str) -> TempDir {
    let api = api_of(wit);
    let mut module = wat::parse_str(wat).unwrap();
    // wit-component hands out a `Resolve` only inside a `Bindgen`.
    let mut resolve = Bindgen::default().resolve;
    let package = resolve.push_source("server.wit", wit).unwrap();
    let world = resolve.select_world(&[package], Some("server")).unwrap();
    wit_component::embed_component_metadata(&mut module, &resolve, world, StringEncoding::UTF8)
        .unwrap();
    probe_dir_with(|dir| {
        fs::write(dir.join(SERVER_WASM), module).unwrap();
        edit_manifest(dir, |manifest| {
            manifest.insert("api".to_owned(), api.into());
        });
        rehash(dir);
    })
}

/// Every file below `dir`, as a `/`-separated path relative to it.
fn relative_files(dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        if entry.file_type().unwrap().is_dir() {
            let nested = relative_files(&entry.path());
            files.extend(nested.into_iter().map(|path| format!("{name}/{path}")));
        } else {
            files.push(name);
        }
    }
    files
}

/// The probe, loaded once per test binary.
pub struct Probe {
    pub engine: Engine,
    pub loaded: Loaded,
    _ticker: EpochTicker,
}

pub fn probe() -> &'static Probe {
    static PROBE: LazyLock<Probe> = LazyLock::new(|| {
        let (engine, ticker) = engine().unwrap();
        // The artifact is only read while loading.
        let dir = probe_dir();
        let loaded = load(&engine, dir.path()).unwrap();
        Probe {
            engine,
            loaded,
            _ticker: ticker,
        }
    });
    &PROBE
}

/// Runs `request` on the probe.
pub fn outcome(request: &Request) -> Outcome {
    let probe = probe();
    run(&probe.engine, &probe.loaded, request)
}

pub fn p(x: i32) -> BlockPos {
    BlockPos { x, y: 64, z: 0 }
}

pub fn up(x: i32) -> BlockPos {
    BlockPos { x, y: 65, z: 0 }
}

/// A loaded cell; `data` is hex.
pub fn cell(pos: BlockPos, id: &str, owned: bool, data: Option<&str>) -> Cell {
    Cell {
        pos,
        loaded: true,
        id: id.to_owned(),
        owned,
        data: data.map(str::to_owned),
    }
}

/// A callback from the actor for `call` at `anchor`, with a 7-cell snapshot: the anchor is an
/// owned probe:counter without data and its six neighbors are loaded air. The world height and the
/// data budget leave room.
pub fn callback(anchor: BlockPos, call: Call) -> Request {
    let BlockPos { x, y, z } = anchor;
    let neighbors = [
        (x + 1, y, z),
        (x - 1, y, z),
        (x, y + 1, z),
        (x, y - 1, z),
        (x, y, z + 1),
        (x, y, z - 1),
    ];
    let mut snapshot = vec![cell(anchor, COUNTER, true, None)];
    snapshot.extend(
        neighbors
            .into_iter()
            .map(|(x, y, z)| cell(BlockPos { x, y, z }, AIR, false, None)),
    );
    Request::Callback {
        seq: 1,
        info: Info {
            world_id: "world".to_owned(),
            dimension_id: "overworld".to_owned(),
            tick: 1,
            event_sequence: 1,
        },
        actor: Some(ACTOR.to_owned()),
        world_min_y: -64,
        world_max_y: 319,
        data_budget: 1 << 20,
        snapshot,
        call,
    }
}

/// The actor's interaction with `p(x)`.
pub fn interact(x: i32) -> Request {
    callback(
        p(x),
        Call::Interact {
            player: ACTOR.to_owned(),
            pos: p(x),
            face: Face::Up,
        },
    )
}

/// A message that the actor's client part sent: a callback without a snapshot.
pub fn client_message(channel: &str, schema: u16, payload: Vec<Scalar>) -> Request {
    let call = Call::ClientMessage {
        player: ACTOR.to_owned(),
        channel: channel.to_owned(),
        schema,
        payload,
        focus: None,
    };
    let mut request = callback(p(0), call);
    if let Request::Callback { snapshot, .. } = &mut request {
        snapshot.clear();
    }
    request
}

/// The actor's client part moved to a new world epoch: a callback without a snapshot.
pub fn epoch() -> Request {
    let call = Call::Epoch {
        player: ACTOR.to_owned(),
        focus: None,
    };
    let mut request = callback(p(0), call);
    if let Request::Callback { snapshot, .. } = &mut request {
        snapshot.clear();
    }
    request
}

/// `request`, a client message or an epoch, with `anchor` as its player's focus: its snapshot is
/// [`callback`]'s for `anchor`.
pub fn focused(request: Request, anchor: BlockPos) -> Request {
    let Request::Callback { mut call, .. } = request else {
        unreachable!("a callback request");
    };
    match &mut call {
        Call::ClientMessage { focus, .. } | Call::Epoch { focus, .. } => *focus = Some(anchor),
        other => unreachable!("{other:?} has no focus"),
    }
    callback(anchor, call)
}

/// The probe's item list of `count` entries: one list of records, each an index and a name.
pub fn items(count: i64) -> Vec<Scalar> {
    let item = |i: i64| Scalar::Record(vec![Scalar::Integer(i), Scalar::Text(format!("item {i}"))]);
    vec![Scalar::List((0..count).map(item).collect())]
}

/// A tell to the actor.
pub fn tell(text: &str) -> Op {
    Op::Tell {
        player: ACTOR.to_owned(),
        text: text.to_owned(),
    }
}

/// A client message to the actor.
pub fn send(channel: &str, schema: u16, payload: Vec<Scalar>) -> Op {
    Op::SendClient {
        player: ACTOR.to_owned(),
        channel: channel.to_owned(),
        schema,
        payload,
    }
}
