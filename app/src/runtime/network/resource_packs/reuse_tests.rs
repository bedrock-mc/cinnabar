use std::sync::Arc;

use protocol::{ResourcePackArchive, ResourcePackHandoff};
use resource_pack::{PackAdmission, ValidatedPackStack};

use super::{CompileEnvironment, CompiledStacks, compile_reusing};
use crate::runtime::network::resource_packs::{
    JoinBases, PackApplication, prepare_join_with,
    tests::{every_input, every_subscriber_archive, overlay_cache, summary},
};
use pack_runtime::PackInputs;

/// A copy of `archive`, as a later join receives its own bytes for the same pack.
fn copy(archive: &ResourcePackArchive) -> ResourcePackArchive {
    ResourcePackArchive::unencrypted(
        archive.pack_id,
        archive.version.clone(),
        archive.sub_pack_name.clone(),
        archive.archive.clone(),
    )
}

/// A fresh admission of a copy of `archive`.
fn admitted(archive: &ResourcePackArchive) -> Arc<ValidatedPackStack> {
    resource_pack::validate_handoff(ResourcePackHandoff::from_archives(vec![copy(archive)]))
}

/// An otherwise empty StartGame.
fn game_data() -> protocol::GameData {
    protocol::GameData::default()
}

/// A join's pack facts for a copy of `archive` under `game_data`.
fn preparation(
    archive: &ResourcePackArchive,
    game_data: &protocol::GameData,
) -> pack_runtime::PackPreparation {
    pack_runtime::prepare_session_packs(
        ResourcePackHandoff::from_archives(vec![copy(archive)]),
        game_data,
        u64::MAX,
    )
}

/// A join's preparation of `archive` under an otherwise empty StartGame.
fn join(kept: &CompiledStacks, archive: &ResourcePackArchive) -> PackApplication {
    let game_data = game_data();
    prepare_join_with(
        kept,
        &preparation(archive, &game_data),
        &game_data,
        &|| false,
        JoinBases::default(),
    )
    .expect("an uncancelled join completes")
    .expect("optional packs never refuse the join")
}

/// Returns the validated archives retained by this compiled fixture.
fn stack(application: &PackApplication) -> &Arc<ValidatedPackStack> {
    match &application.admission {
        PackAdmission::Validated(stack) => stack,
        PackAdmission::None => panic!("the fixture admits a stack"),
    }
}

/// Whether both hold the very same compiled output.
fn same<T>(left: &Option<Arc<T>>, right: &Option<Arc<T>>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if Arc::ptr_eq(left, right))
}

/// Both hold the same allocation, or neither holds one.
fn same_or_absent<T>(left: &Option<Arc<T>>, right: &Option<Arc<T>>) -> bool {
    left.is_none() && right.is_none() || same(left, right)
}

/// The subscriber outputs reading only the stack that `right` compiled again rather than
/// sharing with `left`. Outputs neither compiled, such as uninstalled carrier artwork, count as
/// shared.
fn recompiled(left: &PackApplication, right: &PackApplication) -> Vec<&'static str> {
    [
        ("language", same(&left.server_lang, &right.server_lang)),
        ("glyphs", same(&left.glyph_sheets, &right.glyph_sheets)),
        ("ui", same(&left.server_ui, &right.server_ui)),
        ("sounds", same(&left.server_sounds, &right.server_sounds)),
        ("entities", same(&left.entities, &right.entities)),
        (
            "artwork",
            same_or_absent(&left.entity_artwork, &right.entity_artwork),
        ),
        (
            "block entity art",
            same_or_absent(&left.block_entity_art, &right.block_entity_art),
        ),
        (
            "aim assist",
            left.aim_assist_textures
                .iter()
                .zip(&right.aim_assist_textures)
                .all(|(left, right)| same_or_absent(left, right)),
        ),
    ]
    .into_iter()
    .filter_map(|(output, shared)| (!shared).then_some(output))
    .collect()
}

/// Asserts that every subscriber output reading only the stack is the very same allocation in
/// both. The fixture compiles each of them except carrier artwork, which tests do not install.
fn assert_shares_stack_outputs(left: &PackApplication, right: &PackApplication) {
    let unshared = recompiled(left, right);
    assert!(unshared.is_empty(), "compiled again: {unshared:?}");
}

/// Hashes compiled payloads without read ledgers or empty UI source layers.
fn payload_fingerprint(application: &PackApplication) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut payload = application.clone();
    payload.dependencies.clear();
    if let Some(ui) = &mut payload.server_ui {
        Arc::make_mut(ui)
            .ui_layers
            .retain(|layer| !layer.is_empty());
    }
    Sha256::digest(summary(&payload).as_bytes()).into()
}

// A transfer to a server offering the same packs reuses everything the last join compiled, so
// downstream atlases see the same outputs and one copy of the archives stays alive.
#[test]
fn a_join_with_the_same_packs_reuses_the_previous_compile() {
    let _cache = overlay_cache();
    let kept = CompiledStacks::new();
    let archive = every_subscriber_archive(41, b"a=b");
    let first = join(&kept, &archive);
    let second = join(&kept, &archive);
    assert_shares_stack_outputs(&first, &second);
    assert!(Arc::ptr_eq(stack(&first), stack(&second)));
    assert_eq!(summary(&first), summary(&second));
}

// Different packs compile from scratch; going back to the first server reuses its compile.
#[test]
fn a_join_with_different_packs_compiles_them_and_keeps_both() {
    let _cache = overlay_cache();
    let kept = CompiledStacks::new();
    let lobby = every_subscriber_archive(42, b"a=lobby");
    let game = every_subscriber_archive(42, b"a=game");
    let first = join(&kept, &lobby);
    let second = join(&kept, &game);
    let lookup = |application: &PackApplication| {
        application
            .server_lang
            .as_ref()
            .unwrap()
            .lookup("a")
            .map(str::to_owned)
    };
    assert_eq!(lookup(&second).as_deref(), Some("game"));
    assert!(!same(&first.server_lang, &second.server_lang));
    assert!(!same(&first.glyph_sheets, &second.glyph_sheets));
    assert!(!Arc::ptr_eq(stack(&first), stack(&second)));
    assert_eq!(kept.len(), 2);
    let back = join(&kept, &lobby);
    assert_eq!(lookup(&back).as_deref(), Some("lobby"));
    assert_shares_stack_outputs(&first, &back);
}

// Only icons read the item registry: the same icon keys reuse them, other keys compile icons
// alone again.
#[test]
fn different_start_game_items_recompile_only_icons() {
    let _cache = overlay_cache();
    let kept = CompiledStacks::new();
    let environment = CompileEnvironment::current();
    let archive = every_subscriber_archive(43, b"a=b");
    let first = compile_reusing(
        &kept,
        admitted(&archive),
        every_input(),
        &environment,
        &|| false,
    )
    .unwrap();
    kept.remember(
        environment.clone(),
        &first,
        first.server_ui.clone(),
        &|| false,
    );
    let same_items = compile_reusing(
        &kept,
        admitted(&archive),
        every_input(),
        &environment,
        &|| false,
    )
    .unwrap();
    assert_shares_stack_outputs(&first, &same_items);
    assert!(same(&first.item_icons, &same_items.item_icons));
    let renamed = Arc::new(PackInputs {
        icons: vec![("x:renamed".into(), "gem".into())],
        ..Default::default()
    });
    let second =
        compile_reusing(&kept, admitted(&archive), renamed, &environment, &|| false).unwrap();
    assert_shares_stack_outputs(&first, &second);
    assert!(!same(&first.item_icons, &second.item_icons));
    let identifiers = |application: &PackApplication| {
        application
            .item_icons
            .as_ref()
            .unwrap()
            .icons
            .iter()
            .map(|icon| icon.identifier.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(identifiers(&second), ["x:renamed"]);
    assert_eq!(identifiers(&first), ["x:gem"]);
}

// Only blocks and icons read the custom block definitions: another block set recompiles those
// two alone, as a transfer to a server adding a block does.
#[test]
fn different_custom_blocks_recompile_only_blocks_and_icons() {
    let _cache = overlay_cache();
    let kept = CompiledStacks::new();
    let environment = CompileEnvironment::current();
    let archive = every_subscriber_archive(50, b"a=b");
    let first = compile_reusing(
        &kept,
        admitted(&archive),
        every_input(),
        &environment,
        &|| false,
    )
    .unwrap();
    kept.remember(
        environment.clone(),
        &first,
        first.server_ui.clone(),
        &|| false,
    );
    let with_block = Arc::new(PackInputs {
        blocks: protocol::CustomBlocks {
            blocks: vec![protocol::CustomBlock {
                mining: Default::default(),
                state_physics_complete: false,
                state_physics: Default::default(),
                state_mining: Default::default(),
                player_interact: false,
                name: "reuse:block".into(),
                tags: Default::default(),
                state_count: 1,
                collides: true,
                collision_boxes: None,
                selection: Default::default(),
                friction: None,
                visual: Default::default(),
            }]
            .into(),
            vanilla_blocks: Default::default(),
            skipped: 0,
        },
        ..(*every_input()).clone()
    });
    let second = compile_reusing(&kept, admitted(&archive), with_block, &environment, &|| {
        false
    })
    .unwrap();
    assert_shares_stack_outputs(&first, &second);
    assert!(first.block_overlay.is_none());
    let overlay = second
        .block_overlay
        .as_ref()
        .expect("the new block compiles");
    assert_eq!(overlay.overlay.visuals.len(), 1);
    assert!(!same(&first.item_icons, &second.item_icons));
}

// A join that was cancelled, having left, or whose UI language changed while it compiled is not
// kept: it may have read either language.
#[test]
fn only_an_uncancelled_join_under_unchanged_tables_is_kept() {
    let kept = CompiledStacks::new();
    let archive = ResourcePackArchive::unencrypted(
        "00000000-0000-0000-0000-0000000c0ffe".parse().unwrap(),
        "1.0.0".into(),
        String::new(),
        vec![0; 32],
    );
    let application = PackApplication {
        admission: PackAdmission::Validated(admitted(&archive)),
        ..Default::default()
    };
    let now = CompileEnvironment::current();
    kept.keep_join(now.clone(), &now, &application, None, &|| true);
    assert_eq!(kept.len(), 0, "cancelled");
    let before_the_change = CompileEnvironment {
        language: "xx_XX".into(),
        ..now.clone()
    };
    kept.keep_join(before_the_change, &now, &application, None, &|| false);
    assert_eq!(kept.len(), 0, "the language changed mid-compile");
    let before_device = CompileEnvironment {
        max_layers: now.max_layers + 1,
        ..now.clone()
    };
    kept.keep_join(before_device, &now, &application, None, &|| false);
    assert_eq!(kept.len(), 0, "device capacity changed mid-compile");
    kept.keep_join(now.clone(), &now, &application, None, &|| false);
    assert_eq!(kept.len(), 1);
}

/// Leaving after the first cancellation sample must not let the compile repopulate the cache.
#[test]
fn a_join_cancelled_after_its_first_check_does_not_retain_packs() {
    let archive = ResourcePackArchive::unencrypted(
        "00000000-0000-0000-0000-0000000c0fff".parse().unwrap(),
        "1.0.0".into(),
        String::new(),
        vec![0; 32],
    );
    let application = PackApplication {
        admission: PackAdmission::Validated(admitted(&archive)),
        ..Default::default()
    };
    let kept = CompiledStacks::new();
    let environment = CompileEnvironment::current();
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    let sample = || {
        let was_cancelled = cancelled.swap(true, std::sync::atomic::Ordering::SeqCst);
        if !was_cancelled {
            drop(kept.take());
        }
        was_cancelled
    };
    kept.keep_join(
        environment.clone(),
        &environment,
        &application,
        None,
        &sample,
    );
    assert_eq!(kept.len(), 0, "leaving released the retained packs");
}

// A changed UI language or carrier table means nothing kept may stand in for a compile.
#[test]
fn another_environment_compiles_again() {
    let _cache = overlay_cache();
    let kept = CompiledStacks::new();
    let environment = CompileEnvironment::current();
    let archive = every_subscriber_archive(44, b"a=b");
    let first = compile_reusing(
        &kept,
        admitted(&archive),
        every_input(),
        &environment,
        &|| false,
    )
    .unwrap();
    kept.remember(
        environment.clone(),
        &first,
        first.server_ui.clone(),
        &|| false,
    );
    let mut tables = environment.carrier_tables;
    tables[0] = !tables[0];
    for other in [
        CompileEnvironment {
            language: "xx_XX".into(),
            ..environment.clone()
        },
        CompileEnvironment {
            carrier_tables: tables,
            ..environment.clone()
        },
    ] {
        assert_ne!(other, environment);
        let again =
            compile_reusing(&kept, admitted(&archive), every_input(), &other, &|| false).unwrap();
        assert!(!same(&first.server_lang, &again.server_lang));
        assert!(!same(&first.glyph_sheets, &again.glyph_sheets));
    }
}

// The reload that composes global packs under the joined stack reuses the join's outputs when
// there are no global packs, instead of compiling the same stack a second time.
#[test]
fn the_post_join_reload_reuses_the_join() {
    let _cache = overlay_cache();
    let kept = CompiledStacks::new();
    let archive = every_subscriber_archive(45, b"a=b");
    let joined = join(&kept, &archive);
    let globals = resource_pack::validate_handoff(ResourcePackHandoff::default());
    let composed = Arc::new(ValidatedPackStack::compose(&globals, stack(&joined)).unwrap());
    let reloaded = compile_reusing(
        &kept,
        composed,
        Arc::clone(&joined.inputs),
        &CompileEnvironment::current(),
        &|| false,
    )
    .unwrap();
    assert_shares_stack_outputs(&joined, &reloaded);
    assert!(Arc::ptr_eq(stack(&joined), stack(&reloaded)));
}

#[test]
fn reconnect_reuses_icons_when_registry_order_changes() {
    let _cache = overlay_cache();
    let kept = CompiledStacks::new();
    let archive = every_subscriber_archive(145, b"a=b");
    let mut inputs = (*every_input()).clone();
    inputs.icons.push(("x:other".into(), "gem".into()));
    let environment = CompileEnvironment::current();
    let first = compile_reusing(
        &kept,
        admitted(&archive),
        Arc::new(inputs.clone()),
        &environment,
        &|| false,
    )
    .unwrap();
    kept.remember(
        environment.clone(),
        &first,
        first.server_ui.clone(),
        &|| false,
    );
    inputs.icons.reverse();
    let next = compile_reusing(
        &kept,
        admitted(&archive),
        Arc::new(inputs.clone()),
        &environment,
        &|| false,
    )
    .unwrap();
    let fresh = super::super::prepare_validated_application(admitted(&archive), Arc::new(inputs));
    assert!(same(&first.item_icons, &next.item_icons));
    assert_eq!(summary(&next), summary(&fresh));
}

#[test]
fn composing_globals_preserves_unchanged_compiled_payloads() {
    let _cache = overlay_cache();
    let joined = super::super::prepare_validated_application(
        admitted(&every_subscriber_archive(146, b"a=b")),
        every_input(),
    );
    let globals = crate::runtime::network::pack_reload_tests::stack(&[
        ("texts/en_US.lang", b"global=value"),
        ("extras/global.txt", b"global-only"),
    ]);
    let composed = Arc::new(ValidatedPackStack::compose(&globals, stack(&joined)).unwrap());
    let reused = super::super::prepare_changed_application(
        &CompiledStacks::new(),
        composed.clone(),
        joined.inputs.clone(),
        Some(&joined),
    );
    let fresh = super::super::prepare_validated_application(composed, joined.inputs.clone());
    assert_eq!(recompiled(&joined, &reused), ["language", "sounds"]);
    assert!(same(&joined.item_icons, &reused.item_icons));
    assert_eq!(payload_fingerprint(&reused), payload_fingerprint(&fresh));
    for application in [&reused, &fresh] {
        let pack = application
            .entities
            .as_ref()
            .expect("fixture compiles entities");
        assert_eq!(
            pack.encode(&pack.assets.encode().unwrap()).unwrap(),
            joined
                .entities
                .as_ref()
                .unwrap()
                .encode(&joined.entities.as_ref().unwrap().assets.encode().unwrap())
                .unwrap()
        );
    }
}

// The reload every join starts publishes the join's outputs, so the UI keeps its packed atlases.
#[test]
fn the_post_join_reload_publishes_the_join_outputs() {
    use crate::runtime::network::pack_reload::PackReload;
    use crate::runtime::network::pack_reload_tests::{app_with_assets, settle};
    let _cache = overlay_cache();
    let _sounds = client_audio::audio::SERVER_SOUNDS_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let kept: &'static CompiledStacks = Box::leak(Box::new(CompiledStacks::new()));
    let joined = join(kept, &every_subscriber_archive(47, b"a=b"));
    let mut app = app_with_assets(Arc::new(assets::RuntimeAssets::diagnostic()));
    app.insert_resource(PackReload::with_kept_stacks(kept));
    app.world_mut()
        .resource_mut::<PackReload>()
        .begin_session(1, &joined);
    settle(&mut app, 1);
    assert!(app.world().resource::<PackReload>().error().is_none());
    let ui = app
        .world()
        .resource::<client_shell::ui_resources::UiRuntimeResource>();
    assert!(same(&ui.session_glyphs().cloned(), &joined.glyph_sheets));
    assert!(same(&ui.server_ui().cloned(), &joined.server_ui));
    let world = app.world().resource::<crate::runtime::world::ClientWorld>();
    assert!(same(&world.pack_entities, &joined.entities));
}

// A join compiles on as many threads as the world workers, which idle until its world exists,
// rather than on the two-thread shared pool gameplay uses.
#[test]
fn a_join_compiles_on_the_idle_world_cores() {
    let _cache = overlay_cache();
    let widths = std::sync::Mutex::new(Vec::new());
    let probe = || {
        widths.lock().unwrap().push(rayon::current_num_threads());
        false
    };
    let game_data = game_data();
    let preparation = preparation(&every_subscriber_archive(48, b"a=b"), &game_data);
    crate::runtime::network::resource_packs::prepare_join(
        &preparation,
        &game_data,
        &probe,
        JoinBases::default(),
    )
    .unwrap()
    .unwrap();
    crate::runtime::network::resource_packs::compiled_stacks().release();
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let widths = widths.into_inner().unwrap();
    assert!(!widths.is_empty());
    assert!(
        widths
            .iter()
            .all(|&width| width == chunk_pipeline::world_worker_threads(cores)),
        "{widths:?}"
    );
}

// Comparing layered contents runs outside the cache lock, so a release from the frame stays free.
#[test]
fn kept_stacks_compare_archives_without_holding_their_lock() {
    let archive = ResourcePackArchive::unencrypted(
        "00000000-0000-0000-0000-00000000c0de".parse().unwrap(),
        "1.0.0".into(),
        String::new(),
        vec![0; 32],
    );
    let stack = admitted(&archive);
    let kept = CompiledStacks::from(Arc::clone(&stack));
    let unlocked = |_: &ValidatedPackStack, _: &ValidatedPackStack| {
        assert!(kept.0.try_lock().is_ok(), "compared under the lock");
        true
    };
    let environment = CompileEnvironment::current();
    assert!(kept.matching_by(&stack, &environment, unlocked).is_some());
    let application = PackApplication {
        admission: PackAdmission::Validated(admitted(&archive)),
        ..Default::default()
    };
    kept.remember_by(environment, &application, None, unlocked, &|| false);
    assert_eq!(kept.len(), 1, "the same contents replace their entry");
}

// Rejoining the same server from the menu reuses the last compile; releasing it on leave
// compiled every stack output again.
#[test]
fn a_rejoin_from_the_menu_reuses_the_last_compile() {
    let _cache = overlay_cache();
    let archive = every_subscriber_archive(47, b"a=b");
    let kept = CompiledStacks::new();
    let first = join(&kept, &archive);
    kept.release();
    let released = recompiled(&first, &join(&kept, &archive));
    let kept = CompiledStacks::new();
    let first = join(&kept, &archive);
    kept.keep_for_rejoin();
    let rejoined = join(&kept, &archive);
    println!(
        "rejoin recompiled {} outputs after a release ({released:?}), {} when kept",
        released.len(),
        recompiled(&first, &rejoined).len()
    );
    assert_eq!(released, ["language", "glyphs", "ui", "sounds"]);
    assert_shares_stack_outputs(&first, &rejoined);
    assert!(same_or_absent(&first.item_icons, &rejoined.item_icons));
    assert!(Arc::ptr_eq(stack(&first), stack(&rejoined)));
    assert_eq!(summary(&first), summary(&rejoined));
}

// The kept join holds its prepared UI catalog, so a rejoin binds it instead of resolving the
// server UI against the carrier again.
#[test]
fn a_rejoin_from_the_menu_reuses_the_prepared_ui_catalog() {
    let _cache = overlay_cache();
    let base = Arc::new(
        json_ui::Catalog::from_files([
            ("ui/_global_variables.json", b"{}".as_slice()),
            ("ui/_ui_defs.json", br#"{"ui_defs":[]}"#.as_slice()),
        ])
        .unwrap(),
    );
    let archive = every_subscriber_archive(49, b"a=b");
    let join = |kept: &CompiledStacks| {
        let game_data = game_data();
        prepare_join_with(
            kept,
            &preparation(&archive, &game_data),
            &game_data,
            &|| false,
            JoinBases {
                ui_catalog: Some(&base),
                ..JoinBases::default()
            },
        )
        .unwrap()
        .unwrap()
    };
    let kept = CompiledStacks::new();
    let prepared = Arc::downgrade(join(&kept).server_ui.as_ref().unwrap());
    kept.keep_for_rejoin();
    assert!(prepared.upgrade().is_some(), "kept with the join");
    let rejoined = join(&kept);
    assert!(Arc::ptr_eq(
        rejoined.server_ui.as_ref().unwrap(),
        &prepared.upgrade().unwrap()
    ));
}

// Leaving keeps only the newest stack, and only while its archives fit the rejoin budget.
#[test]
fn leaving_keeps_only_the_newest_stack_within_the_rejoin_budget() {
    let _cache = overlay_cache();
    let kept = CompiledStacks::new();
    join(&kept, &every_subscriber_archive(48, b"a=lobby"));
    let game = join(&kept, &every_subscriber_archive(48, b"a=game"));
    assert_eq!(kept.len(), 2);
    kept.keep_for_rejoin();
    assert_eq!(kept.len(), 1);
    let environment = CompileEnvironment::current();
    assert!(kept.matching(stack(&game), &environment).is_some());
    let archive_bytes = stack(&game)
        .packs()
        .iter()
        .map(|pack| pack.archive_bytes().len())
        .sum::<usize>();
    kept.keep_newest_within(archive_bytes);
    assert_eq!(kept.len(), 1, "a stack exactly at the budget stays");
    kept.keep_newest_within(archive_bytes - 1);
    assert_eq!(kept.len(), 0);
}

// Taking the kept stacks releases their archives and compiled outputs.
#[test]
fn taking_the_kept_stacks_releases_them() {
    let _cache = overlay_cache();
    let kept = CompiledStacks::new();
    let application = join(&kept, &every_subscriber_archive(46, b"a=b"));
    let archives = Arc::downgrade(stack(&application));
    drop(application);
    assert!(archives.upgrade().is_some(), "kept for the next join");
    drop(kept.take());
    assert!(archives.upgrade().is_none());
    assert_eq!(kept.len(), 0);
}
