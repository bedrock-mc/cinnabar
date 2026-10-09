use std::sync::Arc;

use protocol::{ResourcePackArchive, ResourcePackHandoff};
use resource_pack::{PackAdmission, ValidatedPackStack};

use super::{CompileEnvironment, CompiledStacks, compile_reusing};
use crate::runtime::network::{
    pack_reload::PackInputs,
    resource_packs::{
        JoinBases, PackApplication, prepare_join_with,
        tests::{every_input, every_subscriber_archive, overlay_cache, summary},
    },
};

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
    protocol::GameData {
        start_game: Default::default(),
        item_registry: Default::default(),
        biome_definitions: None,
        entity_identifiers: None,
        creative_content: None,
    }
}

/// A join's pack facts for a copy of `archive` under `game_data`.
fn preparation(
    archive: &ResourcePackArchive,
    game_data: &protocol::GameData,
) -> client_session::PackPreparation {
    client_session::prepare_session_packs(
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

/// Asserts that every subscriber output reading only the stack is the very same allocation in
/// both. The fixture compiles each of them except carrier artwork, which tests do not install.
fn assert_shares_stack_outputs(left: &PackApplication, right: &PackApplication) {
    let unshared = [
        ("language", same(&left.server_lang, &right.server_lang)),
        ("glyphs", same(&left.glyph_sheets, &right.glyph_sheets)),
        ("ui", same(&left.server_ui, &right.server_ui)),
        ("sounds", same(&left.server_sounds, &right.server_sounds)),
        ("entities", same(&left.entities, &right.entities)),
        (
            "artwork",
            left.entity_artwork.is_none() && right.entity_artwork.is_none()
                || same(&left.entity_artwork, &right.entity_artwork),
        ),
    ]
    .into_iter()
    .filter_map(|(output, shared)| (!shared).then_some(output))
    .collect::<Vec<_>>();
    assert!(unshared.is_empty(), "compiled again: {unshared:?}");
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
    kept.remember(environment.clone(), &first, first.server_ui.clone());
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
    kept.remember(environment.clone(), &first, first.server_ui.clone());
    let with_block = Arc::new(PackInputs {
        blocks: protocol::CustomBlocks {
            blocks: vec![protocol::CustomBlock {
                state_physics: Default::default(),
                name: "reuse:block".into(),
                tags: Default::default(),
                state_count: 1,
                collides: true,
                collision_boxes: None,
                selection: Default::default(),
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
    kept.keep_join(now.clone(), &now, &application, None, &|| false);
    assert_eq!(kept.len(), 1);
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
    kept.remember(environment.clone(), &first, first.server_ui.clone());
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

// The reload every join starts publishes the join's outputs, so the UI keeps its packed atlases.
#[test]
fn the_post_join_reload_publishes_the_join_outputs() {
    use crate::runtime::network::pack_reload::PackReload;
    use crate::runtime::network::pack_reload_tests::{app_with_assets, settle};
    let _cache = overlay_cache();
    let _sounds = crate::audio::SERVER_SOUNDS_TEST_LOCK
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
    let ui = app.world().resource::<client_ui::ui_runtime::UiRuntime>();
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

// Comparing archives reads every byte; a release from the frame must not wait behind it.
#[test]
fn kept_stacks_compare_archives_without_holding_their_lock() {
    let kept = CompiledStacks::new();
    let archive = ResourcePackArchive::unencrypted(
        "00000000-0000-0000-0000-00000000c0de".parse().unwrap(),
        "1.0.0".into(),
        String::new(),
        vec![0; 32],
    );
    let stack = admitted(&archive);
    kept.keep_for_test(Arc::clone(&stack));
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
    kept.remember_by(environment, &application, None, unlocked);
    assert_eq!(kept.len(), 1, "the same contents replace their entry");
}

// Leaving for the menu releases the kept archives and compiled outputs.
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
