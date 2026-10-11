fn empty_view() -> resource_pack::LayeredPackView {
    resource_pack::LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(Vec::new()),
    ))
}

fn inputs(vanilla: Option<assets::VanillaEntityRefs>) -> super::EntityInputs {
    super::EntityInputs {
        stack: Vec::new(),
        vanilla: vanilla.map(std::sync::Arc::new),
        vanilla_pack_dir: None,
    }
}

fn refs_with_animation() -> assets::VanillaEntityRefs {
    let mut refs = assets::VanillaEntityRefs::new();
    refs.animations
        .insert("animation.test".into(), serde_json::Value::Null);
    refs
}

// The same stack recompiles when the vanilla refs it resolved against change.
#[test]
fn entity_cache_keys_on_vanilla_refs() {
    let (view, cache) = (empty_view(), super::EntityCache::default());
    let compiles = std::cell::Cell::new(0);
    for vanilla in [
        None,
        None,
        Some(assets::VanillaEntityRefs::new()),
        Some(refs_with_animation()),
    ] {
        cache.get_or_compile(inputs(vanilla), &view, None, &|| false, |_, _, _, _, _| {
            compiles.set(compiles.get() + 1);
            Some((None, None))
        });
    }
    assert_eq!(compiles.get(), 3);
}

#[test]
fn entity_cache_recompiles_when_the_installed_vanilla_layer_changes() {
    let (view, cache) = (empty_view(), super::EntityCache::default());
    let compiles = std::cell::Cell::new(0);
    for directory in ["first", "first", "second"] {
        let mut input = inputs(None);
        input.vanilla_pack_dir = Some(directory.into());
        cache.get_or_compile(input, &view, None, &|| false, |_, _, _, _, _| {
            compiles.set(compiles.get() + 1);
            Some((None, None))
        });
    }
    assert_eq!(compiles.get(), 2);
}

#[test]
fn entity_cache_compiles_without_holding_its_lock() {
    let (view, cache) = (empty_view(), super::EntityCache::default());
    cache.get_or_compile(inputs(None), &view, None, &|| false, |_, _, _, _, _| {
        assert!(cache.last.try_lock().is_ok());
        Some((None, None))
    });
}

#[test]
fn later_vanilla_refs_replace_earlier_ones() {
    let later = refs_with_animation();
    super::super::set_vanilla_refs(assets::VanillaEntityRefs::new());
    super::super::set_vanilla_refs(later.clone());
    assert_eq!(super::super::vanilla_refs().as_deref(), Some(&later));
}

const FIXTURE_MATERIAL_PATH: &str = "materials/fixture.material";

fn entity_stack() -> std::sync::Arc<resource_pack::ValidatedPackStack> {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([37, 59, 83, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    crate::test_support::stack(&[
            ("entity/fixture.json", br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"fixture:actor","geometry":{"default":"geometry.fixture"},"materials":{"default":"fixture_alpha"},"textures":{"default":"textures/entity/fixture"},"render_controllers":["controller.render.fixture"]}}}"#),
            (FIXTURE_MATERIAL_PATH, br#"{"materials":{"version":"1.0.0","fixture_alpha:entity_alphatest":{}}}"#),
            ("models/entity/fixture.json", br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.fixture","texture_width":1,"texture_height":1},"bones":[{"name":"root","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]}]}"#),
            ("render_controllers/fixture.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.fixture":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#),
            ("textures/entity/fixture.png", png.get_ref()),
        ])
}

// A relaunch (a fresh in-memory cache) reads the pack and its reads back from disk; a
// damaged entry recompiles.
#[test]
fn a_disk_hit_skips_compilation_and_a_corrupt_entry_recompiles() {
    let dir = tempfile::tempdir().unwrap();
    let disk = crate::compile_cache::CompileCache::new(dir.path().into(), 1 << 30);
    let stack = entity_stack();
    let compiles = std::cell::Cell::new(0);
    let launch = || {
        let view = resource_pack::LayeredPackView::tracked(stack.clone());
        let pack = super::EntityCache::default().get_or_compile(
            inputs(None),
            &view,
            Some(&disk),
            &|| false,
            |view, vanilla, vanilla_pack_dir, encode, cancelled| {
                compiles.set(compiles.get() + 1);
                super::super::compile_encoded(view, vanilla, vanilla_pack_dir, encode, cancelled)
            },
        );
        let pack = pack.flatten().expect("the fixture defines an entity");
        let reads = view.dependencies().unwrap().snapshot();
        // Catalog encoding excludes metadata about whether a carrier was decoded.
        (
            pack.assets.encode().expect("the catalog is encodable"),
            pack.textures.clone(),
            pack.bindings.clone(),
            reads,
        )
    };
    let compiled = launch();
    assert!(
        compiled
            .3
            .contains(&resource_pack::PackDependency::Directory(
                "materials/".to_owned()
            ))
    );
    assert!(compiled.3.iter().any(|dependency| matches!(
        dependency,
        resource_pack::PackDependency::File { path, .. } if path == FIXTURE_MATERIAL_PATH
    )));
    assert!(
        launch() == compiled,
        "a disk hit preserves the catalog, artwork and tracked inputs"
    );
    assert_eq!(compiles.get(), 1, "the second launch was a disk hit");
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let path = entry.unwrap().path();
        let mut bytes = std::fs::read(&path).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0xff;
        std::fs::write(path, bytes).unwrap();
    }
    assert!(
        launch() == compiled,
        "recompilation preserves the catalog, artwork and tracked inputs"
    );
    assert_eq!(compiles.get(), 2, "a corrupt entry is a miss");
}

// A join gets a fresh compile before its disk entry is encoded and written; the deferred
// write then stores what a later launch reads back without compiling.
#[test]
fn a_fresh_compile_returns_before_its_disk_entry_is_written() {
    type Write = Box<dyn FnOnce() + Send>;
    static DEFERRED: std::sync::Mutex<Vec<Write>> = std::sync::Mutex::new(Vec::new());
    let dir = tempfile::tempdir().unwrap();
    let disk = crate::compile_cache::CompileCache::new(dir.path().into(), 1 << 30);
    let stack = entity_stack();
    let deferring = super::EntityCache {
        store: |write| DEFERRED.lock().unwrap().push(write),
        ..super::EntityCache::default()
    };
    let compiled = deferring.get_or_compile(
        inputs(None),
        &resource_pack::LayeredPackView::tracked(stack.clone()),
        Some(&disk),
        &|| false,
        super::super::compile_encoded,
    );
    let compiled = compiled.flatten();
    assert!(compiled.is_some());
    assert_eq!(std::fs::read_dir(dir.path()).map_or(0, Iterator::count), 0);
    for write in std::mem::take(&mut *DEFERRED.lock().unwrap()) {
        write();
    }
    let relaunched = super::EntityCache::default().get_or_compile(
        inputs(None),
        &resource_pack::LayeredPackView::tracked(stack),
        Some(&disk),
        &|| false,
        |_, _, _, _, _| panic!("the deferred write stored the compile"),
    );
    assert_eq!(
        relaunched
            .flatten()
            .map(|pack| pack.assets.encode().unwrap()),
        compiled.map(|pack| pack.assets.encode().unwrap())
    );
}

// Deferred writes run in order on one thread, never one thread per join; while one runs and
// another waits, a third is skipped rather than holding a third pack.
#[test]
fn deferred_writes_run_one_at_a_time_on_one_writer() {
    use std::sync::mpsc::channel;
    let timeout = std::time::Duration::from_secs(60);
    let (ran, runs) = channel();
    let (release, held) = channel::<()>();
    let (started, running) = channel();
    let report = |name: &'static str| {
        let ran = ran.clone();
        Box::new(move || ran.send((name, std::thread::current().id())).unwrap())
            as Box<dyn FnOnce() + Send>
    };
    super::store_in_background(Box::new(move || {
        started.send(()).unwrap();
        held.recv().unwrap();
    }));
    running.recv_timeout(timeout).unwrap();
    super::store_in_background(report("waiting"));
    super::store_in_background(report("skipped"));
    release.send(()).unwrap();
    let waiting = runs.recv_timeout(timeout).unwrap();
    super::store_in_background(report("later"));
    let later = runs.recv_timeout(timeout).unwrap();
    assert_eq!((waiting.0, later.0), ("waiting", "later"));
    assert_eq!(waiting.1, later.1, "one writer thread");
    assert_ne!(waiting.1, std::thread::current().id());
}

// A cancelled join stops its entity compile at the next step and keeps nothing, so a later
// join compiles the stack afresh rather than reusing a partial result.
#[test]
fn a_cancelled_compile_stops_between_steps_and_keeps_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let disk = crate::compile_cache::CompileCache::new(dir.path().into(), 1 << 30);
    let stack = entity_stack();
    let cache = super::EntityCache::default();
    let checks = std::cell::Cell::new(0);
    let stopped = cache.get_or_compile(
        inputs(None),
        &resource_pack::LayeredPackView::tracked(stack.clone()),
        Some(&disk),
        &|| {
            checks.set(checks.get() + 1);
            true
        },
        super::super::compile_encoded,
    );
    assert!(stopped.is_none());
    assert_eq!(checks.get(), 1, "the compile stopped at its first check");
    assert_eq!(std::fs::read_dir(dir.path()).map_or(0, Iterator::count), 0);
    let compiles = std::cell::Cell::new(0);
    let compiled = cache.get_or_compile(
        inputs(None),
        &resource_pack::LayeredPackView::tracked(stack),
        Some(&disk),
        &|| false,
        |view, vanilla, vanilla_pack_dir, encode, cancelled| {
            compiles.set(compiles.get() + 1);
            super::super::compile_encoded(view, vanilla, vanilla_pack_dir, encode, cancelled)
        },
    );
    assert!(compiled.flatten().is_some());
    assert_eq!(compiles.get(), 1);
}
