use std::{collections::HashMap, fs, path::Path};

use assets::carriers::{
    self, AUDIO, AUDIO_PCM, CARRIERS, Carrier, FONT, HUD, ICON, Sources, WEATHER, WORLD,
};

use super::{Command, Queue, Reporter, command, execute, take_ready};
use crate::prepare_plan::{Context, Entry, Plan, Scope, Stamp, plan, read_stamp, write_stamp};

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
    fs::write(dir.path().join("assets/fonts/Font.ttf"), b"font-2").unwrap();
    assert_eq!(stale(dir.path(), COMPILER), [FONT.name]);
    build_all(dir.path());
    let root = Sources::Checkout(dir.path().to_path_buf());
    let registry = context(dir.path()).files(&WORLD)[0].clone();
    assert_eq!(
        registry,
        root.resolve("crates/assets/data/block-registry-v2193.bin")
    );
    fs::write(registry, b"changed").unwrap();
    assert_eq!(stale(dir.path(), COMPILER), [WORLD.name, ICON.name]);
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
