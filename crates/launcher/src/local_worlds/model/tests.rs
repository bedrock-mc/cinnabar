use bridge::{
    Backend, Difficulty, GameMode, Generator, Prefs, Setup, SetupState, UnavailableReason, World,
    WorldState, WorldStatus, WorldUpdate,
};

use super::super::progress::Stage;
use super::super::prompt::{DOCKER_URL, Prompt, PromptButton, PromptFor, PromptKind};
use super::*;

fn world(id: &str, name: &str) -> World {
    World {
        id: id.to_owned(),
        name: name.to_owned(),
        game_mode: GameMode::Survival,
        generator: Generator::Flat,
        difficulty: Difficulty::Normal,
        backend: Backend::Dragonfly,
        seed: 1,
        created_unix: 0,
        last_played_unix: 0,
        size_bytes: 0,
    }
}

fn status(state: WorldState, id: &str) -> WorldStatus {
    WorldStatus {
        state,
        world_id: Some(id.to_owned()),
        backend: None,
        paused: false,
        pause_supported: true,
        error: None,
        setup: None,
        backend_unavailable_reason: None,
        max_players: None,
    }
}

fn setup(state: SetupState) -> Setup {
    Setup {
        state,
        version: None,
        bytes_done: 0,
        bytes_total: 0,
        layers_done: 0,
        layers_total: 0,
        eula_accepted: true,
        error: None,
        runtime: "container".to_owned(),
        reason: None,
    }
}

fn loaded(names: &[&str]) -> WorldsMenu {
    let mut menu = WorldsMenu::default();
    assert_eq!(menu.update(Input::Refresh), vec![Effect::List]);
    menu.apply(Event::Listed(
        names
            .iter()
            .enumerate()
            .map(|(i, n)| world(&format!("id{i}"), n))
            .collect(),
    ));
    menu
}

#[test]
fn refresh_selects_first_world_and_keeps_selection_by_id() {
    let mut menu = loaded(&["a", "b", "c"]);
    assert_eq!(menu.selected().map(|w| w.name.as_str()), Some("a"));
    menu.update(Input::Select(2));
    menu.update(Input::Refresh);
    menu.apply(Event::Listed(vec![world("new", "n"), world("id2", "c")]));
    assert_eq!(menu.selected().map(|w| w.id.as_str()), Some("id2"));
}

#[test]
fn empty_list_has_no_selection_and_ignores_actions() {
    let mut menu = loaded(&[]);
    assert!(menu.selected().is_none());
    assert!(menu.update(Input::Play).is_empty());
    assert!(menu.update(Input::BeginEdit(0)).is_empty());
    assert_eq!(menu.screen(), Screen::List);
}

/// The whole mock-core path: create, the list gains the world, and it opens, polls and hands off.
#[test]
fn create_lists_the_world_then_launches_it_through_every_stage() {
    let mut menu = loaded(&["a"]);
    menu.update(Input::BeginCreate);
    assert_eq!(menu.screen(), Screen::Create);
    menu.update(Input::SetName("  ".to_owned()));
    assert!(menu.update(Input::SubmitCreate).is_empty());
    assert!(menu.form_error().is_some());
    menu.update(Input::SetName("Fresh".to_owned()));
    menu.update(Input::SetSeed("0".to_owned()));
    menu.update(Input::SetGameMode(GameMode::Creative));
    let effects = menu.update(Input::SubmitCreate);
    let [Effect::Create(new_world)] = effects.as_slice() else {
        panic!("expected one create effect, got {effects:?}");
    };
    assert_eq!(
        (new_world.name.as_str(), new_world.seed, new_world.game_mode),
        ("Fresh", Some(0), GameMode::Creative)
    );
    assert!(
        menu.update(Input::SubmitCreate).is_empty(),
        "busy blocks double submit"
    );

    let mut created = world("fresh", "Fresh");
    created.backend = Backend::Bds;
    assert_eq!(
        menu.apply(Event::Created(created)),
        vec![Effect::Open("fresh".to_owned())],
        "a new world is entered at once"
    );
    assert_eq!(menu.worlds().len(), 2);
    assert_eq!(menu.worlds()[0].id, "fresh");
    assert_eq!(menu.screen(), Screen::Opening);

    let mut stages = vec![menu.progress().expect("progress").stage];
    for state in [
        SetupState::CheckingRuntime,
        SetupState::PullingImage,
        SetupState::Downloading,
        SetupState::Unpacking,
        SetupState::Ready,
    ] {
        let mut starting = status(WorldState::Starting, "fresh");
        starting.backend = Some(Backend::Bds);
        starting.setup = Some(setup(state));
        assert_eq!(
            menu.apply(Event::Status(starting)),
            vec![Effect::PollStatus]
        );
        stages.push(menu.progress().expect("progress").stage);
    }
    assert_eq!(
        stages,
        [
            Stage::StartingServer,
            Stage::CheckingDocker,
            Stage::PullingImage,
            Stage::DownloadingServer,
            Stage::InstallingServer,
            Stage::StartingServer,
        ]
    );
    assert!(menu.take_ready().is_none());
    assert!(
        menu.apply(Event::Status(status(WorldState::Running, "fresh")))
            .is_empty()
    );
    assert_eq!(menu.take_ready().as_deref(), Some("fresh"));
    assert!(menu.take_ready().is_none());
    assert_eq!((menu.screen(), menu.progress()), (Screen::List, None));
}

#[test]
fn delete_lives_behind_edit_and_needs_confirmation() {
    let mut menu = loaded(&["a", "b"]);
    assert!(menu.update(Input::RequestDelete).is_empty());
    assert!(
        menu.update(Input::ConfirmDelete).is_empty(),
        "no delete without the confirm screen"
    );
    menu.update(Input::BeginEdit(1));
    assert_eq!(menu.screen(), Screen::Edit);
    menu.update(Input::RequestDelete);
    assert_eq!(menu.screen(), Screen::ConfirmDelete);
    menu.update(Input::Back);
    assert_eq!(
        (menu.screen(), menu.worlds().len()),
        (Screen::Edit, 2),
        "cancel returns to the settings"
    );
    menu.update(Input::RequestDelete);
    assert_eq!(
        menu.update(Input::ConfirmDelete),
        vec![Effect::Delete("id1".to_owned())]
    );
    menu.apply(Event::Deleted("id1".to_owned()));
    assert_eq!(menu.worlds().len(), 1);
    assert_eq!(menu.selected().map(|w| w.id.as_str()), Some("id0"));
    assert_eq!(menu.screen(), Screen::List);
}

#[test]
fn edit_prefills_validates_and_sends_only_changes() {
    let mut menu = loaded(&["Old"]);
    menu.update(Input::BeginEdit(0));
    assert_eq!(menu.edit_form().map(|e| e.name.as_str()), Some("Old"));
    menu.update(Input::SetEditName(String::new()));
    assert!(menu.update(Input::SubmitEdit).is_empty());
    assert!(menu.form_error().is_some());
    menu.update(Input::SetEditName(" New ".to_owned()));
    menu.update(Input::SetDifficulty(Difficulty::Hard));
    assert_eq!(
        menu.update(Input::SubmitEdit),
        vec![Effect::Update {
            id: "id0".to_owned(),
            update: WorldUpdate {
                name: Some("New".to_owned()),
                game_mode: None,
                difficulty: Some(Difficulty::Hard),
            },
        }]
    );
    let mut saved = world("id0", "New");
    saved.difficulty = Difficulty::Hard;
    menu.apply(Event::Updated(saved));
    assert_eq!(menu.selected().map(|w| w.name.as_str()), Some("New"));
    assert_eq!(menu.screen(), Screen::List);

    menu.update(Input::BeginEdit(0));
    assert!(
        menu.update(Input::SubmitEdit).is_empty(),
        "unchanged settings send nothing"
    );
    assert_eq!(menu.screen(), Screen::List);
}

#[test]
fn templates_show_the_empty_state_and_back_returns() {
    let mut menu = loaded(&["a"]);
    menu.update(Input::OpenTemplates);
    assert_eq!(menu.screen(), Screen::Templates);
    menu.update(Input::Back);
    assert_eq!(menu.screen(), Screen::List);
}

#[test]
fn opening_failure_shows_error_and_clears_core_failure() {
    let mut menu = loaded(&["a"]);
    menu.update(Input::Play);
    let mut failed = status(WorldState::Failed, "id0");
    failed.error = Some("server exited".to_owned());
    assert_eq!(menu.apply(Event::Status(failed)), vec![Effect::Close]);
    assert_eq!(
        (menu.screen(), menu.error()),
        (Screen::Error, Some("server exited"))
    );
    assert!(menu.take_ready().is_none());
    menu.update(Input::Back);
    assert_eq!((menu.screen(), menu.error()), (Screen::List, None));
}

#[test]
fn back_while_opening_closes_and_ignores_late_status() {
    let mut menu = loaded(&["a"]);
    menu.update(Input::Play);
    assert_eq!(menu.update(Input::Back), vec![Effect::Close]);
    assert_eq!(menu.screen(), Screen::List);
    assert!(
        menu.apply(Event::Status(status(WorldState::Running, "id0")))
            .is_empty()
    );
    assert!(
        menu.take_ready().is_none(),
        "a cancelled open must not hand off"
    );
}

#[test]
fn status_for_another_world_is_ignored() {
    let mut menu = loaded(&["a", "b"]);
    menu.update(Input::Play);
    assert!(
        menu.apply(Event::Status(status(WorldState::Running, "id1")))
            .is_empty()
    );
    assert!(menu.take_ready().is_none());
    assert_eq!(menu.screen(), Screen::Opening);
}

#[test]
fn request_failure_surfaces_message_and_unblocks() {
    let mut menu = loaded(&["a"]);
    menu.update(Input::Refresh);
    assert!(menu.busy());
    assert!(
        menu.apply(Event::Failed(
            "Local world service is unavailable".to_owned()
        ))
        .is_empty()
    );
    assert_eq!(menu.screen(), Screen::Error);
    assert!(!menu.busy());
}

fn with_reason(reason: UnavailableReason) -> WorldStatus {
    let mut status = status(WorldState::Idle, "");
    status.world_id = None;
    status.backend_unavailable_reason = Some(reason);
    status.setup = Some(setup(SetupState::Unsupported));
    status
}

fn docker_menu(reason: UnavailableReason, names: &[&str]) -> WorldsMenu {
    let mut menu = loaded(names);
    menu.apply(Event::Prefs(Prefs::default(), with_reason(reason)));
    menu
}

fn prompt(kind: PromptKind, blocking: PromptFor) -> Option<Prompt> {
    Some(Prompt { kind, blocking })
}

/// The core probes Docker after startup, so the menu keeps reading prefs until a verdict arrives.
#[test]
fn prefs_are_polled_until_docker_detection_settles() {
    let mut menu = loaded(&["a"]);
    let mut checking = status(WorldState::Idle, "");
    checking.setup = Some(setup(SetupState::CheckingRuntime));
    assert_eq!(
        menu.apply(Event::Prefs(Prefs::default(), checking)),
        vec![Effect::PollPrefs]
    );
    assert!(
        menu.apply(Event::Prefs(
            Prefs::default(),
            with_reason(UnavailableReason::DockerNotRunning)
        ))
        .is_empty()
    );
    assert!(menu.update(Input::BeginCreate).is_empty());
    assert_eq!(menu.screen(), Screen::Create);
    assert_eq!(menu.create_form().backend, Backend::Dragonfly);
}

#[test]
fn no_backend_reason_never_shows_the_docker_modal() {
    let mut menu = loaded(&["a"]);
    menu.apply(Event::Prefs(Prefs::default(), status(WorldState::Idle, "")));
    menu.update(Input::BeginCreate);
    assert_eq!(menu.screen(), Screen::Create);
    assert_eq!(menu.create_form().generator, Generator::Normal);
}

#[test]
fn missing_docker_blocks_only_bds_and_fallback_preserves_the_form() {
    for generator in [Generator::Normal, Generator::Flat] {
        let mut menu = docker_menu(UnavailableReason::DockerMissing, &[]);
        menu.update(Input::BeginCreate);
        menu.update(Input::SetName("My saved form".into()));
        menu.update(Input::SetSeed("-7".into()));
        menu.update(Input::SetFlat(generator == Generator::Flat));
        menu.update(Input::SetBackend(Backend::Bds));
        assert!(menu.update(Input::SubmitCreate).is_empty());
        assert_eq!(
            menu.prompt(),
            prompt(PromptKind::DockerMissing, PromptFor::CreateBds)
        );
        assert_eq!(
            menu.update(Input::Prompt(PromptButton::GetDocker)),
            vec![Effect::OpenUrl(DOCKER_URL)]
        );
        menu.update(Input::Back);
        assert_eq!(menu.screen(), Screen::Create);
        assert_eq!(menu.create_form().backend, Backend::Bds);
        menu.update(Input::SubmitCreate);
        let effects = menu.update(Input::Prompt(PromptButton::UseDragonfly));
        let [Effect::Create(spec)] = effects.as_slice() else {
            panic!("missing create");
        };
        assert_eq!(spec.backend, Some(Backend::Dragonfly));
        assert_eq!(spec.generator, generator);
        assert_eq!(spec.name, "My saved form");
        assert_eq!(spec.seed, Some(-7));
    }
}

#[test]
fn backend_selection_preserves_terrain_and_seed() {
    let mut menu = loaded(&[]);
    menu.update(Input::BeginCreate);
    menu.update(Input::SetSeed("42".into()));
    menu.update(Input::SetFlat(true));
    menu.update(Input::SetBackend(Backend::Bds));
    assert_eq!(menu.create_form().generator, Generator::Flat);
    menu.update(Input::SetFlat(false));
    assert_eq!(menu.create_form().backend, Backend::Bds);
    menu.update(Input::SetBackend(Backend::Dragonfly));
    assert_eq!(menu.create_form().generator, Generator::Normal);
    assert_eq!(menu.create_form().seed_text, "42");
}

#[test]
fn retry_redetects_and_continues_once_docker_is_up() {
    let mut menu = docker_menu(UnavailableReason::DockerNotRunning, &[]);
    menu.update(Input::BeginCreate);
    menu.update(Input::SetBackend(Backend::Bds));
    menu.update(Input::SubmitCreate);
    assert_eq!(
        menu.prompt(),
        prompt(PromptKind::DockerNotRunning, PromptFor::CreateBds)
    );
    assert_eq!(
        menu.update(Input::Prompt(PromptButton::Retry)),
        vec![Effect::SetPrefs {
            dismiss_docker_prompt: false,
            redetect: true
        }]
    );
    assert!(menu.busy());
    menu.apply(Event::Prefs(
        Prefs::default(),
        with_reason(UnavailableReason::DockerNotRunning),
    ));
    assert_eq!((menu.screen(), menu.busy()), (Screen::BackendPrompt, false));
    menu.update(Input::Prompt(PromptButton::Retry));
    menu.apply(Event::Prefs(Prefs::default(), status(WorldState::Idle, "")));
    assert_eq!(menu.screen(), Screen::Create);
    assert!(menu.busy());
}

#[test]
fn playing_a_dragonfly_world_skips_the_modal_but_a_bds_world_needs_docker() {
    let mut menu = docker_menu(UnavailableReason::DockerNotRunning, &["a"]);
    assert_eq!(
        menu.update(Input::Play),
        vec![Effect::Open("id0".to_owned())]
    );
    menu.update(Input::Back);
    let mut bds = world("id0", "a");
    bds.backend = Backend::Bds;
    menu.apply(Event::Listed(vec![bds]));
    assert!(menu.update(Input::Play).is_empty());
    assert_eq!(
        menu.prompt(),
        prompt(PromptKind::DockerNotRunning, PromptFor::Play)
    );
    assert!(
        !menu
            .prompt()
            .expect("prompt")
            .buttons()
            .contains(&PromptButton::UseDragonfly),
        "a saved BDS world never falls back to another server"
    );
    menu.update(Input::Prompt(PromptButton::Retry));
    assert_eq!(
        menu.apply(Event::Prefs(Prefs::default(), status(WorldState::Idle, ""))),
        vec![Effect::Open("id0".to_owned())]
    );
}

#[test]
fn docker_stopping_mid_open_offers_retry() {
    let mut menu = loaded(&["a"]);
    let mut bds = world("id0", "a");
    bds.backend = Backend::Bds;
    menu.apply(Event::Listed(vec![bds]));
    menu.update(Input::Play);
    let mut failed = with_reason(UnavailableReason::DockerNotRunning);
    failed.state = WorldState::Failed;
    failed.world_id = Some("id0".to_owned());
    failed.error = Some("Docker is not running".to_owned());
    assert_eq!(menu.apply(Event::Status(failed)), vec![Effect::Close]);
    assert_eq!(
        menu.prompt(),
        prompt(PromptKind::DockerNotRunning, PromptFor::Play)
    );
}

#[test]
fn eula_required_prompts_then_reopens_the_same_world_after_acceptance() {
    let mut menu = loaded(&["a"]);
    menu.update(Input::Play);
    assert!(menu.apply(Event::EulaRequired).is_empty());
    assert_eq!(menu.screen(), Screen::Eula);
    assert!(menu.update(Input::Back).is_empty());
    assert_eq!(menu.screen(), Screen::List);

    menu.update(Input::Play);
    menu.apply(Event::EulaRequired);
    assert_eq!(
        menu.update(Input::OpenEulaLink),
        vec![Effect::OpenUrl(EULA_URL)]
    );
    assert_eq!(menu.update(Input::AcceptEula), vec![Effect::AcceptEula]);
    assert!(menu.busy());
    assert_eq!(
        menu.apply(Event::EulaAccepted),
        vec![Effect::Open("id0".to_owned())]
    );
    assert_eq!(menu.screen(), Screen::Opening);
}

#[test]
fn accept_eula_outside_the_eula_screen_does_nothing() {
    let mut menu = loaded(&["a"]);
    assert!(menu.update(Input::AcceptEula).is_empty());
    assert!(!menu.busy());
}

#[test]
fn create_defaults_to_dragonfly_and_normal_independent_of_bds_availability() {
    for state in [SetupState::Ready, SetupState::Unsupported] {
        let mut menu = loaded(&[]);
        let mut idle = status(WorldState::Idle, "");
        idle.setup = Some(setup(state));
        menu.apply(Event::Prefs(Prefs::default(), idle));
        menu.update(Input::BeginCreate);
        assert_eq!(menu.create_form().generator, Generator::Normal);
        assert_eq!(menu.create_form().backend, Backend::Dragonfly);
    }
}

/// New worlds offer Survival and Creative only; Adventure is an edit-screen choice.
#[test]
fn adventure_is_offered_only_when_editing() {
    let mut menu = loaded(&["a"]);
    menu.update(Input::BeginCreate);
    menu.update(Input::SetGameMode(GameMode::Adventure));
    assert_eq!(menu.create_form().game_mode, GameMode::Survival);
    menu.update(Input::Back);
    menu.update(Input::BeginEdit(0));
    menu.update(Input::SetGameMode(GameMode::Adventure));
    assert_eq!(
        menu.edit_form().map(|e| e.game_mode),
        Some(GameMode::Adventure)
    );
}

#[test]
fn leaving_edit_with_changes_asks_to_save_or_discard() {
    let mut menu = loaded(&["a"]);
    menu.update(Input::BeginEdit(0));
    assert!(menu.update(Input::Back).is_empty());
    assert_eq!(menu.screen(), Screen::List, "no edits leave at once");
    menu.update(Input::BeginEdit(0));
    menu.update(Input::SetEditName("b".to_owned()));
    menu.update(Input::Back);
    assert_eq!(menu.screen(), Screen::ConfirmLeaveEdit);
    menu.update(Input::Back);
    assert_eq!(menu.screen(), Screen::Edit, "cancel keeps editing");
    menu.update(Input::Back);
    assert!(menu.update(Input::DiscardEdit).is_empty());
    assert_eq!(
        (menu.screen(), menu.worlds()[0].name.as_str()),
        (Screen::List, "a")
    );
    menu.update(Input::BeginEdit(0));
    menu.update(Input::SetEditName("b".to_owned()));
    menu.update(Input::Back);
    assert!(matches!(
        menu.update(Input::SubmitEdit).as_slice(),
        [Effect::Update { .. }]
    ));
}

#[test]
fn play_from_edit_saves_then_opens() {
    let mut menu = loaded(&["a"]);
    menu.update(Input::BeginEdit(0));
    assert_eq!(
        menu.update(Input::PlayFromEdit),
        vec![Effect::Open("id0".to_owned())],
        "nothing to save"
    );
    menu.update(Input::Back);
    menu.update(Input::BeginEdit(0));
    menu.update(Input::SetDifficulty(Difficulty::Peaceful));
    assert!(matches!(
        menu.update(Input::PlayFromEdit).as_slice(),
        [Effect::Update { .. }]
    ));
    let mut saved = world("id0", "a");
    saved.difficulty = Difficulty::Peaceful;
    assert_eq!(
        menu.apply(Event::Updated(saved)),
        vec![Effect::Open("id0".to_owned())]
    );
}

#[test]
fn review_preferences_reply_cannot_unlock_a_pending_creation() {
    let mut menu = loaded(&[]);
    menu.update(Input::BeginCreate);
    menu.update(Input::SetFlat(true));
    assert!(matches!(
        menu.update(Input::SubmitCreate).as_slice(),
        [Effect::Create(_)]
    ));
    menu.apply(Event::Prefs(Prefs::default(), status(WorldState::Idle, "")));
    assert!(menu.busy());
    assert!(menu.update(Input::SubmitCreate).is_empty());
}

#[test]
fn review_cancelled_creation_updates_the_list_without_opening_it() {
    let mut menu = loaded(&[]);
    menu.update(Input::BeginCreate);
    menu.update(Input::SetFlat(true));
    menu.update(Input::SubmitCreate);
    menu.update(Input::Back);
    assert!(menu.apply(Event::Created(world("fresh", "new"))).is_empty());
    assert_eq!(menu.screen(), Screen::List);
    assert_eq!(menu.worlds()[0].id, "fresh");
}

#[test]
fn template_create_action_opens_the_create_form() {
    let mut menu = loaded(&[]);
    menu.update(Input::OpenTemplates);
    assert_eq!(menu.screen(), Screen::Templates);
    assert!(menu.update(Input::BeginCreate).is_empty());
    assert_eq!(menu.screen(), Screen::Create);
}

#[test]
fn new_world_backend_defaults_to_dragonfly_without_a_docker_gate() {
    let mut menu = docker_menu(UnavailableReason::DockerMissing, &[]);
    menu.update(Input::BeginCreate);
    assert_eq!(menu.screen(), Screen::Create);
    let effects = menu.update(Input::SubmitCreate);
    let [Effect::Create(spec)] = effects.as_slice() else {
        panic!("Dragonfly creation must not be blocked by Docker");
    };
    assert_eq!(spec.backend, Some(Backend::Dragonfly));
    assert_eq!(spec.generator, Generator::Normal);
}
