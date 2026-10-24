//! A settled engine HUD reuses its paint and its publication; a visible change paints once, and
//! running animations paint every frame until they settle. Every frame matches a full paint.

use super::*;

const VIEWPORT: [u32; 2] = [1280, 720];

/// A survival HUD stepped beside a presentation that paints and lays out every frame in full.
struct Fixture {
    presentation: UiPresentationRuntime,
    full: UiPresentationRuntime,
    player: player_state::PlayerState,
    runtime: UiRuntime,
    now: u64,
    physical: [u32; 2],
}

impl Fixture {
    /// A HUD whose first frames' fades have settled, or `None` without the local UI carrier.
    fn settled() -> Option<Self> {
        let (presentation, full) = (engine_presentation()?, engine_presentation()?);
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = UiRuntime::new(1);
        player
            .facts
            .publish_player_game_mode(PlayerGameMode::Survival);
        select_slot(&mut player, &mut runtime);
        full_stats(&mut player, &mut runtime, 1);
        let mut fixture = Self {
            presentation,
            full,
            player,
            runtime,
            now: 0,
            physical: VIEWPORT,
        };
        fixture.step();
        fixture.now = 10_000;
        test_time::eventually("the HUD's first fades and artwork to settle", || {
            fixture.next().1 == [0, 0]
        });
        Some(fixture)
    }

    /// Builds the frame at `now` beside a full paint and layout, asserts both publish the same
    /// frame, and returns it with the `[hud, crosshair]` paints it ran.
    fn step(&mut self) -> (render_model::UiRenderInput, [usize; 2]) {
        let before = self.presentation.hud_paints();
        let input = build_at(
            &self.player,
            &mut self.presentation,
            &self.runtime,
            self.now,
            self.physical,
            1.0,
        );
        let after = self.presentation.hud_paints();
        self.full.last_frame = None;
        self.full.forget_hud_paints();
        let expected = build_at(
            &self.player,
            &mut self.full,
            &self.runtime,
            self.now,
            self.physical,
            1.0,
        );
        assert_eq!(input.vertices, expected.vertices, "frame at {}", self.now);
        assert_eq!(input.indices, expected.indices, "frame at {}", self.now);
        assert_eq!(input.batches, expected.batches, "frame at {}", self.now);
        (input, [after[0] - before[0], after[1] - before[1]])
    }

    /// Steps one 16 ms frame.
    fn next(&mut self) -> (render_model::UiRenderInput, [usize; 2]) {
        self.now += 16;
        self.step()
    }

    /// Applies `change` to both presentations' captured frame state.
    fn frame(&mut self, change: impl Fn(&mut HudFrame)) {
        change(self.presentation.hud_frame_mut());
        change(self.full.hud_frame_mut());
    }

    /// The frame after a change paints the HUD exactly once; the one after reuses it.
    fn paints_once(&mut self, change: &str) {
        let (_, paints) = self.next();
        assert_eq!(paints[0], 1, "{change} paints the HUD once");
        let (_, paints) = self.next();
        assert_eq!(paints, [0, 0], "{change} is then reused");
    }
}

#[test]
fn a_settled_hud_paints_and_uploads_nothing() {
    let Some(mut fixture) = Fixture::settled() else {
        eprintln!(
            "skipping a_settled_hud_paints_and_uploads_nothing: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let (settled, _) = fixture.next();
    for _ in 0..30 {
        let (input, paints) = fixture.next();
        assert_eq!(paints, [0, 0], "nothing changed at {}", fixture.now);
        assert_eq!(
            input.revision, settled.revision,
            "no upload at {}",
            fixture.now
        );
        assert!(Arc::ptr_eq(&input.vertices, &settled.vertices));
        assert!(Arc::ptr_eq(&input.batches, &settled.batches));
    }
}

#[test]
fn each_visible_hud_change_paints_once() {
    let Some(mut fixture) = Fixture::settled() else {
        eprintln!(
            "skipping each_visible_hud_change_paints_once: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    fixture.next();
    fixture.runtime.hud.set_experience(4, 0.1);
    fixture.paints_once("an experience binding");
    fixture.frame(|frame| frame.hotbar_stacks[3] = Some(item(5, 16)));
    fixture.paints_once("a hotbar stack");
    fixture.frame(|frame| frame.hotbar_cooldowns[0] = 0.5);
    fixture.paints_once("a hotbar cooldown");
    fixture.physical = [1600, 900];
    fixture.paints_once("a viewport");
}

#[test]
fn a_fading_chat_line_paints_every_frame_until_it_settles() {
    let Some(mut fixture) = Fixture::settled() else {
        eprintln!(
            "skipping a_fading_chat_line_paints_every_frame_until_it_settles: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    fixture.next();
    let line = SequencedUiEvent {
        local_millis: fixture.now,
        ..chat_line(2, "gg")
    };
    fixture.runtime.apply(&mut fixture.player, line).unwrap();
    for _ in 0..10 {
        let (_, paints) = fixture.next();
        assert_eq!(
            paints[0], 1,
            "the visible chat line animates at {}",
            fixture.now
        );
    }
    fixture.now += 12_000;
    fixture.step();
    let (_, paints) = fixture.next();
    assert_eq!(paints, [0, 0], "the faded chat line settles");
}
