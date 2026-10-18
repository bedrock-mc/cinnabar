use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use assets::carriers::{
    self, AUDIO, AUDIO_PCM, BLOCK_PHYSICS, CARRIERS, Carrier, FONT, HUD, ICON, Input, Sources,
    WEATHER, WORLD,
};

use super::{Command, Queue, Reporter, command, execute, record, take_ready};
use crate::prepare_plan::{
    Context, Entry, Plan, Scope, Stamp, kit_files, plan, read_stamp, write_stamp,
};

const COMPILER: &str = "compiler-a";

/// A checkout with every tracked input the table names, and no unpacked pack.
fn checkout() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = Sources::Checkout(dir.path().to_path_buf());
    for carrier in CARRIERS {
        for input in carrier.inputs {
            if let carriers::Input::Manifest(path)
            | carriers::Input::File(path)
            | carriers::Input::FontFile(path) = input
            {
                let file = root.resolve(path);
                fs::create_dir_all(file.parent().unwrap()).unwrap();
                fs::write(&file, path).unwrap();
            }
        }
    }
    set_pack(dir.path(), "v1");
    fs::create_dir_all(root.resolve("assets/fonts")).unwrap();
    for carrier in CARRIERS {
        for input in carrier.inputs {
            if let carriers::Input::FontFile(manifest) = input {
                let name = if carrier.name == FONT.name {
                    "Font.ttf".to_owned()
                } else {
                    format!("{}.ttf", carrier.name)
                };
                fs::write(
                    root.resolve(manifest),
                    serde_json::json!({"font_file": name}).to_string(),
                )
                .unwrap();
                fs::write(root.resolve(&format!("assets/fonts/{name}")), b"font-1").unwrap();
            }
        }
    }
    dir
}

fn set_pack(root: &Path, tag: &str) {
    let manifest = format!(
        r#"{{"schema":1,"tag":"{tag}","commit":"c","archive":"pack.zip","url":"https://example.invalid/pack.zip","sha256":"00","artifact_policy":"local-only","cache_dir":".local/assets/bedrock-samples/{tag}/full"}}"#
    );
    fs::write(root.join(carriers::VANILLA_MANIFEST), manifest).unwrap();
}

fn context(root: &Path) -> Context {
    Context::new(
        Sources::Checkout(root.to_path_buf()),
        root.to_path_buf(),
        root.join("out"),
        None,
    )
    .unwrap()
}

fn plan_for(root: &Path, compiler: &str, only: &[String]) -> Plan {
    let context = context(root);
    let scope = Scope {
        installed_only: false,
        only,
    };
    plan(&context, compiler, &scope, &read_stamp(&context.out)).unwrap()
}

fn stale(root: &Path, compiler: &str) -> Vec<&'static str> {
    plan_for(root, compiler, &[])
        .stale
        .iter()
        .map(|carrier| carrier.name)
        .collect()
}

/// Stamps every carrier as built from the current inputs and writes its outputs.
fn build_all(root: &Path) {
    let plan = plan_for(root, COMPILER, &[]);
    let out = root.join("out");
    fs::create_dir_all(&out).unwrap();
    let mut stamp = Stamp::default();
    stamp.inputs.clone_from(&plan.inputs);
    for carrier in &plan.selected {
        for path in carrier.outputs(&out) {
            fs::write(path, b"carrier").unwrap();
        }
        stamp.carriers.insert(
            carrier.name.to_owned(),
            Entry {
                fingerprint: plan.fingerprints[carrier.name].clone(),
                failed: false,
            },
        );
    }
    write_stamp(&out, &stamp).unwrap();
}

#[test]
fn unchanged_inputs_skip_every_carrier() {
    let dir = checkout();
    assert_eq!(stale(dir.path(), COMPILER).len(), CARRIERS.len());
    build_all(dir.path());
    assert!(stale(dir.path(), COMPILER).is_empty());
}

#[test]
fn a_changed_input_rebuilds_its_carrier_and_the_carriers_reading_it() {
    let dir = checkout();
    build_all(dir.path());
    fs::write(dir.path().join("assets/fonts/Font.ttf"), b"font-22").unwrap();
    assert_eq!(stale(dir.path(), COMPILER), [FONT.name]);
    build_all(dir.path());
    let root = Sources::Checkout(dir.path().to_path_buf());
    let registry = context(dir.path()).files(&WORLD)[0].clone();
    assert_eq!(
        registry,
        root.resolve("crates/assets/data/block-registry-v2193.bin")
    );
    fs::write(registry, b"changed").unwrap();
    assert_eq!(
        stale(dir.path(), COMPILER),
        [BLOCK_PHYSICS.name, WORLD.name, ICON.name]
    );
}

#[test]
fn font_carriers_with_one_recipe_are_selected_and_stamped_independently() {
    let dir = checkout();
    build_all(dir.path());
    for carrier in CARRIERS
        .iter()
        .filter(|carrier| carrier.recipe == carriers::Recipe::Font)
    {
        let only = [carrier.name.to_owned()];
        let plan = plan_for(dir.path(), COMPILER, &only);
        assert_eq!(plan.selected.len(), 1);
        assert_eq!(plan.selected[0].name, carrier.name);
        assert!(!plan.needs_pack());
        let context = context(dir.path());
        let file = context
            .font_file(match carrier.inputs[1] {
                carriers::Input::FontFile(manifest) => manifest,
                _ => unreachable!(),
            })
            .unwrap();
        fs::write(file, b"changed font").unwrap();
        assert_eq!(stale(dir.path(), COMPILER), [carrier.name]);
        build_all(dir.path());
    }
}

#[test]
fn a_new_pack_pin_rebuilds_every_pack_carrier() {
    let dir = checkout();
    build_all(dir.path());
    set_pack(dir.path(), "v2");
    let plan = plan_for(dir.path(), COMPILER, &[]);
    assert!(plan.needs_pack());
    let names: Vec<_> = plan.stale.iter().map(|carrier| carrier.name).collect();
    assert!(names.contains(&WORLD.name) && names.contains(&HUD.name));
    assert!(!names.contains(&FONT.name));
}

#[test]
fn a_changed_compiler_rebuilds_everything() {
    let dir = checkout();
    build_all(dir.path());
    assert_eq!(stale(dir.path(), "compiler-b").len(), CARRIERS.len());
}

#[test]
fn a_missing_output_rebuilds_only_that_carrier() {
    let dir = checkout();
    build_all(dir.path());
    fs::remove_file(dir.path().join("out").join(AUDIO.output)).unwrap();
    assert_eq!(stale(dir.path(), COMPILER), [AUDIO.name]);
}

#[test]
fn a_failed_optional_carrier_waits_for_new_inputs() {
    let dir = checkout();
    build_all(dir.path());
    let out = dir.path().join("out");
    let mut stamp = read_stamp(&out);
    stamp.carriers.get_mut(WEATHER.name).unwrap().failed = true;
    write_stamp(&out, &stamp).unwrap();
    fs::remove_file(out.join(WEATHER.output)).unwrap();
    assert!(stale(dir.path(), COMPILER).is_empty());
    assert!(stale(dir.path(), "compiler-b").contains(&WEATHER.name));
}

#[test]
fn explicit_preparation_retries_optional_failures_until_success() {
    let dir = checkout();
    build_all(dir.path());
    let context = context(dir.path());
    for carrier in CARRIERS.iter().filter(|carrier| !carrier.required) {
        let only = [carrier.name.to_owned()];
        let initial = plan_for(dir.path(), COMPILER, &only);
        assert!(initial.stale.is_empty());
        let stamp = Mutex::new(read_stamp(&context.out));
        let failure = Err("temporary resource failure".to_owned());
        for _ in 0..2 {
            record(&context, &stamp, &initial, carrier, &failure).unwrap();
            assert!(carrier.outputs(&context.out).all(|path| !path.exists()));
            assert!(stale(dir.path(), COMPILER).is_empty());
            let retry = plan_for(dir.path(), COMPILER, &only);
            assert_eq!(retry.fingerprints, initial.fingerprints);
            assert_eq!(
                retry
                    .stale
                    .iter()
                    .map(|carrier| carrier.name)
                    .collect::<Vec<_>>(),
                [carrier.name],
                "explicit preparation must retry {} after a failure",
                carrier.name,
            );
        }
        for path in carrier.outputs(&context.out) {
            fs::write(path, b"recovered carrier").unwrap();
        }
        record(&context, &stamp, &initial, carrier, &Ok(())).unwrap();
        assert!(!read_stamp(&context.out).carriers[carrier.name].failed);
        assert!(plan_for(dir.path(), COMPILER, &only).stale.is_empty());
        assert!(stale(dir.path(), COMPILER).is_empty());
    }
}

#[test]
fn explicit_preparation_retries_a_failed_optional_dependency() {
    let dir = checkout();
    build_all(dir.path());
    let context = context(dir.path());
    let initial = plan_for(dir.path(), COMPILER, &[AUDIO.name.to_owned()]);
    let stamp = Mutex::new(read_stamp(&context.out));
    record(
        &context,
        &stamp,
        &initial,
        &AUDIO,
        &Err("temporary resource failure".to_owned()),
    )
    .unwrap();
    let retry = plan_for(dir.path(), COMPILER, &[AUDIO_PCM.name.to_owned()]);
    assert_eq!(
        retry
            .stale
            .iter()
            .map(|carrier| carrier.name)
            .collect::<Vec<_>>(),
        [AUDIO.name]
    );
}

#[test]
fn only_selects_a_carrier_and_what_it_reads() {
    let dir = checkout();
    let selected = plan_for(dir.path(), COMPILER, &[ICON.name.to_owned()]).selected;
    let names: Vec<_> = selected.iter().map(|carrier| carrier.name).collect();
    assert_eq!(names, [WORLD.name, ICON.name]);
    let context = context(dir.path());
    let scope = Scope {
        installed_only: false,
        only: &["nope".to_owned()],
    };
    let error = plan(&context, COMPILER, &scope, &Stamp::default())
        .err()
        .unwrap();
    assert!(error.to_string().contains("unknown carrier 'nope'"));
}

#[test]
fn kit_runs_skip_development_only_carriers() {
    let dir = checkout();
    let context = context(dir.path());
    let scope = Scope {
        installed_only: true,
        only: &[],
    };
    let plan = plan(&context, COMPILER, &scope, &Stamp::default()).unwrap();
    assert!(plan.selected.iter().all(|carrier| carrier.installed));
    assert!(!plan.selected.iter().any(|c| c.name == AUDIO_PCM.name));
}

fn queue(jobs: &[&'static Carrier], finished: &[(&Carrier, bool)]) -> Queue {
    Queue {
        pending: (0..jobs.len()).collect(),
        finished: finished
            .iter()
            .map(|(carrier, ok)| (carrier.name, *ok))
            .collect::<HashMap<_, _>>(),
        abort: None,
    }
}

#[test]
fn a_reader_waits_for_the_carrier_it_reads() {
    let jobs = [&ICON, &WORLD];
    let mut waiting = queue(&jobs, &[]);
    assert_eq!(take_ready(&mut waiting, &jobs), Some((1, None)));
    assert_eq!(take_ready(&mut waiting, &jobs), None);
    let mut built = queue(&jobs[..1], &[(&WORLD, true)]);
    assert_eq!(take_ready(&mut built, &jobs[..1]), Some((0, None)));
    let mut failed = queue(&jobs[..1], &[(&WORLD, false)]);
    assert_eq!(
        take_ready(&mut failed, &jobs[..1]),
        Some((0, Some(WORLD.name)))
    );
    // A current dependency outside the run never blocks its reader.
    let mut current = queue(&jobs[..1], &[]);
    assert_eq!(take_ready(&mut current, &jobs[..1]), Some((0, None)));
}

#[test]
fn every_recipe_reads_the_outputs_the_table_names() {
    let dir = checkout();
    let context = context(dir.path());
    for carrier in CARRIERS {
        let command = command(carrier, &context).unwrap();
        match command {
            Command::IconAssets { block_assets, .. } => {
                assert_eq!(block_assets, Some(context.out.join(WORLD.output)));
            }
            Command::AudioPcmAssets { catalog, .. } => {
                assert_eq!(catalog, context.out.join(AUDIO.output));
            }
            _ => {}
        }
    }
}

/// The world recipe receives exactly its three registries, in their declared roles.
#[test]
fn world_recipe_passes_only_its_registries() {
    let dir = checkout();
    let context = context(dir.path());
    let Command::Compile {
        registry,
        light_registry,
        biome_registry,
        ..
    } = command(&WORLD, &context).unwrap()
    else {
        panic!("the world recipe compiles block assets");
    };
    let named = |suffix: &str| {
        context
            .files(&WORLD)
            .into_iter()
            .find(|path| path.to_string_lossy().ends_with(suffix))
            .unwrap()
    };
    assert_eq!(registry, named("block-registry-v2193.bin"));
    assert_eq!(light_registry, named("block-light-registry-v2193.bin"));
    assert_eq!(biome_registry, named("biome-registry-v2193.bin"));
}

fn run(root: &Path, only: &str) -> (Result<(), String>, Stamp) {
    let context = context(root);
    let only = [only.to_owned()];
    let scope = Scope {
        installed_only: false,
        only: &only,
    };
    let plan = plan(&context, COMPILER, &scope, &Stamp::default()).unwrap();
    fs::create_dir_all(&context.out).unwrap();
    let result = execute(&context, &plan, Stamp::default(), &Reporter { json: true });
    (result, read_stamp(&context.out))
}

#[test]
fn an_optional_failure_is_stamped_and_a_required_failure_aborts() {
    let dir = checkout();
    let (result, stamp) = run(dir.path(), WEATHER.name);
    assert!(result.is_ok(), "{result:?}");
    assert!(stamp.carriers[WEATHER.name].failed);
    let (result, stamp) = run(dir.path(), HUD.name);
    let error = result.unwrap_err();
    assert!(error.starts_with(&format!("{}: ", HUD.label)), "{error}");
    assert!(!stamp.carriers.contains_key(HUD.name));
}

#[test]
fn a_required_failure_starts_nothing_that_reads_it() {
    let dir = checkout();
    let (result, stamp) = run(dir.path(), ICON.name);
    assert!(result.unwrap_err().starts_with(WORLD.label));
    assert!(stamp.carriers.is_empty());
}

#[test]
fn deleting_the_extracted_pack_rebuilds_nothing() {
    let dir = checkout();
    let pack = context(dir.path()).pack.cache;
    fs::create_dir_all(pack.join("behavior_pack/items")).unwrap();
    fs::create_dir_all(pack.join("resource_pack")).unwrap();
    build_all(dir.path());
    fs::remove_dir_all(&pack).unwrap();
    assert!(stale(dir.path(), COMPILER).is_empty());
}

#[test]
fn an_input_with_unchanged_size_and_mtime_is_not_rehashed() {
    let dir = checkout();
    let font = dir.path().join("assets/fonts/Font.ttf");
    let modified = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1 << 30);
    let set = |bytes: &[u8]| {
        fs::write(&font, bytes).unwrap();
        fs::File::options()
            .write(true)
            .open(&font)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    };
    set(b"font-1");
    build_all(dir.path());
    // Same size and mtime: the stamped digest stands even though the bytes differ.
    set(b"font-2");
    assert!(stale(dir.path(), COMPILER).is_empty());
    set(b"font-22");
    assert_eq!(stale(dir.path(), COMPILER), [FONT.name]);
}

#[test]
fn physics_compiles_from_the_seed_and_is_reused_until_an_input_changes() {
    let dir = checkout();
    let files = context(dir.path()).files(&BLOCK_PHYSICS);
    fs::write(&files[0], assets::pinned_block_registry_bytes()).unwrap();
    fs::write(
        &files[1],
        include_bytes!("../../../../assets/data/block-physics-seed-v2193.bin"),
    )
    .unwrap();
    let (result, _) = run(dir.path(), BLOCK_PHYSICS.name);
    result.unwrap();
    let ctx = context(dir.path());
    let output = fs::read(ctx.out.join(BLOCK_PHYSICS.output)).unwrap();
    assert_eq!(
        format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(&output)),
        include_str!("../../../../assets/data/block-physics-v2193.sha256").trim(),
        "prepared physics must match the client's startup pin"
    );
    let protocol = assets::active_content_registry_protocol();
    let records =
        assets::read_registry_for_protocol(assets::pinned_block_registry_bytes(), protocol)
            .unwrap();
    let registry = assets::read_physics_registry_for_protocol(
        &output,
        assets::pinned_block_registry_bytes(),
        &records,
        protocol,
    )
    .unwrap();
    assert_eq!(registry.len(), records.len());
    assert!(records.iter().all(|record| {
        registry
            .by_sequential_id(record.sequential_id)
            .unwrap()
            .shapes
            .is_some()
    }));
    assert!(
        plan_for(dir.path(), COMPILER, &[BLOCK_PHYSICS.name.to_owned()])
            .stale
            .is_empty()
    );
    fs::write(&files[1], b"changed seed").unwrap();
    assert_eq!(
        plan_for(dir.path(), COMPILER, &[BLOCK_PHYSICS.name.to_owned()]).stale[0].name,
        BLOCK_PHYSICS.name
    );
}

#[test]
fn entity_only_behavior_is_available_to_entity_actor_and_equipment_compilation() {
    let root = checkout();
    let context = context(root.path());
    let behavior = context.pack.cache.join("behavior_pack");
    fs::create_dir_all(behavior.join("entities")).unwrap();
    assert_eq!(context.behavior_pack(), Some(behavior.clone()));
    assert_eq!(context.seat_behavior_pack(), Some(behavior.clone()));
    for carrier in [carriers::ENTITY, carriers::ACTOR, carriers::EQUIPMENT] {
        let selected = match command(&carrier, &context).unwrap() {
            Command::EntityAssets { behavior_pack, .. }
            | Command::ActorAssets { behavior_pack, .. }
            | Command::EquipmentAssets { behavior_pack, .. } => behavior_pack,
            _ => unreachable!(),
        };
        assert_eq!(selected, Some(behavior.clone()));
    }
}

#[test]
fn prepared_actor_and_equipment_decode_against_the_published_entity_carrier() {
    let root = checkout();
    let context = context(root.path());
    let resource = context.resource_pack();
    for family in [
        "entity",
        "models/entity",
        "animations",
        "animation_controllers",
        "render_controllers",
        "textures/entity",
    ] {
        fs::create_dir_all(resource.join(family)).unwrap();
    }
    fs::write(resource.join("entity/mount.json"), br#"{"format_version":"1.8.0","minecraft:client_entity":{"description":{"identifier":"fixture:mount","geometry":{"default":"geometry.mount"},"materials":{"default":"entity_alphatest"},"textures":{"default":"textures/entity/mount"},"render_controllers":["controller.render.mount"]}}}"#).unwrap();
    fs::write(resource.join("models/entity/mount.json"), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.mount","texture_width":16,"texture_height":16},"bones":[{"name":"root","cubes":[{"origin":[0,0,0],"size":[0,2,7],"uv":[0,0]}]}]}]}"#).unwrap();
    fs::write(resource.join("render_controllers/mount.json"), br#"{"format_version":"1.8.0","render_controllers":{"controller.render.mount":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#).unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([17, 31, 47, 255]))
        .save(resource.join("textures/entity/mount.png"))
        .unwrap();
    let behavior = context.pack.cache.join("behavior_pack/entities");
    fs::create_dir_all(&behavior).unwrap();
    fs::write(behavior.join("mount.json"), br#"{"format_version":"1.21.0","minecraft:entity":{"description":{"identifier":"fixture:mount"},"components":{"minecraft:rideable":{"seats":{"position":[0,1,0]}}}}}"#).unwrap();
    fs::write(
        root.path().join(carriers::VANILLA_MANIFEST),
        include_bytes!("../../../../../assets/vanilla-source.json"),
    )
    .unwrap();
    fs::create_dir_all(&context.out).unwrap();
    for with_behavior in [true, false] {
        if !with_behavior {
            fs::remove_dir_all(&behavior).unwrap();
        }
        for carrier in [carriers::ENTITY, carriers::ACTOR, carriers::EQUIPMENT] {
            crate::run(command(&carrier, &context).unwrap()).unwrap();
        }
        let entity = assets::RuntimeEntityAssets::decode(
            &fs::read(context.out.join(carriers::ENTITY.output)).unwrap(),
        )
        .unwrap();
        let actor = assets::RuntimeActorCatalog::decode(
            &fs::read(context.out.join(carriers::ACTOR.output)).unwrap(),
            &entity,
        )
        .unwrap();
        assert_eq!(actor.bindings().len(), 1);
        assert_eq!(&actor.textures()[0].rgba8[..4], &[17, 31, 47, 255]);
        let equipment = assets::RuntimeEquipmentCatalog::decode(
            &fs::read(context.out.join(carriers::EQUIPMENT.output)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            Some(equipment.entity_blob_sha256()),
            entity.carrier_identity()
        );
    }
}

/// The list both packaging scripts stage the installer's preparation kit from.
const KIT_LIST: &str = "packaging/common/prep-kit-files.txt";

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn packaged_kit_files() -> Vec<String> {
    let list = fs::read_to_string(repository().join(KIT_LIST)).unwrap();
    list.lines().map(str::to_owned).collect()
}

/// Copies every listed file to the same path below `kit`, as the packaging scripts do.
fn stage_kit(kit: &Path) {
    let (checkout, staged) = (Sources::Checkout(repository()), Sources::Kit(kit.into()));
    for file in packaged_kit_files() {
        let target = staged.resolve(&file);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(checkout.resolve(&file), target).unwrap_or_else(|error| panic!("{file}: {error}"));
    }
}

const INSTALLED: Scope = Scope {
    installed_only: true,
    only: &[],
};

#[test]
fn the_packaged_kit_list_names_every_file_a_kit_run_reads() {
    let derived = kit_files(&Sources::Checkout(repository())).unwrap();
    assert!(
        packaged_kit_files() == derived,
        "{KIT_LIST} is stale; regenerate it with \
         `cargo run -p asset-compiler --bin assetc -- kit-files > {KIT_LIST}`"
    );
}

/// Marks every file and directory below `root` read-only, or writable again.
fn set_read_only(root: &Path, read_only: bool) -> std::io::Result<()> {
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            set_read_only(&path, read_only)?;
        }
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_readonly(read_only);
        fs::set_permissions(&path, permissions)?;
    }
    Ok(())
}

/// Keeps a tree read-only until dropped, so the temporary directory can still be removed.
struct ReadOnly<'a>(&'a Path);

impl<'a> ReadOnly<'a> {
    fn new(root: &'a Path) -> Self {
        set_read_only(root, true).unwrap();
        Self(root)
    }
}

impl Drop for ReadOnly<'_> {
    fn drop(&mut self) {
        let _ = set_read_only(self.0, false);
    }
}

/// Installs once failed because the kit lacked files that planning hashes. The kit is read-only,
/// as in an install directory, and plans the fingerprints the checkout does.
#[test]
fn a_kit_staged_from_the_packaged_list_plans_and_builds_without_the_checkout() {
    let dir = tempfile::tempdir().unwrap();
    let kit = dir.path().join("kit");
    stage_kit(&kit);
    let _read_only = ReadOnly::new(&kit);
    let workspace = dir.path().join("workspace");
    let out = dir.path().join("out");
    let context = Context::new(Sources::Kit(kit.clone()), workspace.clone(), out, None).unwrap();
    let planned = plan(&context, COMPILER, &INSTALLED, &Stamp::default()).unwrap();
    let checkout = Context::new(
        Sources::Checkout(repository()),
        workspace,
        dir.path().join("checkout-out"),
        None,
    )
    .unwrap();
    let from_checkout = plan(&checkout, COMPILER, &INSTALLED, &Stamp::default()).unwrap();
    assert_eq!(planned.fingerprints, from_checkout.fingerprints);
    for carrier in &planned.selected {
        command(carrier, &context).unwrap_or_else(|error| panic!("{}: {error}", carrier.name));
    }
    let pack_free: Vec<String> = planned
        .selected
        .iter()
        .filter(|carrier| {
            !carrier
                .inputs
                .iter()
                .any(|input| matches!(input, Input::Pack | Input::BehaviorPack))
        })
        .map(|carrier| carrier.name.to_owned())
        .collect();
    assert!(pack_free.iter().any(|name| name == BLOCK_PHYSICS.name));
    let scope = Scope {
        installed_only: true,
        only: &pack_free,
    };
    let planned = plan(&context, COMPILER, &scope, &Stamp::default()).unwrap();
    fs::create_dir_all(&context.out).unwrap();
    execute(
        &context,
        &planned,
        Stamp::default(),
        &Reporter { json: true },
    )
    .unwrap();
    let stamp = read_stamp(&context.out);
    for name in &pack_free {
        assert!(
            stamp.carriers.get(name).is_some_and(|entry| !entry.failed),
            "{name} did not build from the kit"
        );
    }
}

#[test]
fn a_kit_staged_from_the_packaged_list_prepares_every_required_carrier_from_the_pinned_pack() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    let checkout = Context::new(
        Sources::Checkout(repository()),
        repository(),
        out.clone(),
        None,
    )
    .unwrap();
    if !checkout.pack.is_unpacked() {
        eprintln!(
            "missing fixture: pinned sample pack at {}; skipping (run `make vanilla-assets`)",
            checkout.pack.cache.display()
        );
        return;
    }
    let kit = dir.path().join("kit");
    stage_kit(&kit);
    let context = Context::new(Sources::Kit(kit), repository(), out, None).unwrap();
    let planned = plan(&context, COMPILER, &INSTALLED, &Stamp::default()).unwrap();
    fs::create_dir_all(&context.out).unwrap();
    execute(
        &context,
        &planned,
        Stamp::default(),
        &Reporter { json: true },
    )
    .unwrap();
    assert!(carriers::required_present(&context.out));
}
