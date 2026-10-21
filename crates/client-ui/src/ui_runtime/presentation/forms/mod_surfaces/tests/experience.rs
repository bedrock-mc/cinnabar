use crate::test_support::mini_engine_presentation;
use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use server_experience::policy::Choice;
use server_experience::policy::{
    MAX_FALLBACK_BYTES, MAX_IDENTIFIER_BYTES, MAX_ORIGINS, MAX_URL_BYTES,
};
use ui_engine::text_metrics::TextMetrics;

#[test]
fn small_viewports_scroll_maximum_disclosures_before_enabling_approval() {
    let player_runtime = player_state::PlayerState::new(1);

    let packages = (0..server_experience::policy::MAX_BUNDLES)
        .map(|i| {
            format!(
                "Package: {i}{}\nPublisher: {}",
                "a".repeat(MAX_IDENTIFIER_BYTES - 1),
                server_experience::crypto::hex(&[7; 32])
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let hosts = (0..MAX_ORIGINS)
        .map(|i| {
            let prefix = format!("https://{i}");
            format!(
                "{prefix}{}.com",
                "a".repeat(MAX_URL_BYTES - prefix.len() - ".com".len())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let origins = hosts.lines().map(str::to_owned).collect();
    for host in hosts.lines() {
        server_experience::fetch::approved_url(host, &origins).unwrap();
    }
    let text = format!(
        "Cinnabar server experience\n{packages}\nPermissions: Messaging, Media\nMedia/download hosts: {hosts}\nThese hosts see your IP address.\nFallback: {}\nServer code is untrusted.",
        "f".repeat(MAX_FALLBACK_BYTES)
    );
    for size in [[320, 240], [640, 360]] {
        let mut presentation = mini_engine_presentation();
        let runtime = UiRuntime::new(1);
        presentation
            .mod_ui_mut()
            .set_experience_chrome(Some(&text), true)
            .unwrap();
        presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                size,
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        assert!(presentation.mod_ui().experience_prompt_visible());
        assert!(!presentation.mod_ui().experience_approval_ready());
        let frame = presentation.mod_ui().experience_frame().unwrap();
        assert!(
            frame
                .report
                .scrolls
                .values()
                .all(|metrics| metrics.content > metrics.viewport)
        );
        assert!(
            frame
                .hits
                .iter()
                .filter(|hit| matches!(
                    hit.pressed.as_deref(),
                    Some("experience.once" | "experience.always")
                ))
                .all(|hit| !hit.enabled)
        );
        for _ in 0..500 {
            presentation.mod_ui_mut().scroll_experience(1.0);
            presentation
                .build(
                    &player_runtime,
                    &runtime,
                    0,
                    size,
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
            if presentation.mod_ui().experience_approval_ready() {
                break;
            }
        }
        assert!(presentation.mod_ui().experience_approval_ready());
        let mut nodes = Vec::new();
        presentation.append_experience_chrome(
            &runtime,
            &mut nodes,
            &mut 1,
            TextMetrics::for_viewport(size, ui::DpiScale::new(1.0).unwrap(), None),
            size.map(|v| v as f32),
        );
        let texts = super::super::super::pack_harness::drawn_texts(&nodes);
        assert!(
            texts.iter().any(|text| text == "Server code is untrusted."),
            "last disclosure was not painted: {texts:?}"
        );
        presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                size,
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let frame = presentation.mod_ui().experience_frame().unwrap();
        let once = frame
            .hits
            .iter()
            .find(|hit| hit.pressed.as_deref() == Some("experience.once"))
            .unwrap();
        assert!(once.enabled);
        let point = [
            frame.origin[0] + (once.rect.x + once.rect.w / 2.0) as f32 * frame.scale,
            frame.origin[1] + (once.rect.y + once.rect.h / 2.0) as f32 * frame.scale,
        ];
        assert_eq!(
            presentation.mod_ui().experience_choice(point),
            Some(Choice::Once)
        );
    }
}

#[test]
fn popup_buttons_answer_with_their_choices() {
    let mut presentation = mini_engine_presentation();
    let runtime = UiRuntime::new(1);
    let player_runtime = player_state::PlayerState::new(1);
    presentation
        .mod_ui_mut()
        .set_experience_chrome(Some("Server: 127.0.0.1:19132"), true)
        .unwrap();
    let size = [1280, 720];
    let build = |presentation: &mut UiPresentationRuntime| {
        presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                size,
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    };
    // A disclosure that fits is reviewed by its first frame; the next one arms approval.
    build(&mut presentation);
    build(&mut presentation);
    assert!(presentation.mod_ui().experience_approval_ready());
    let frame = presentation.mod_ui().experience_frame().cloned().unwrap();
    for (action, choice) in [
        ("experience.once", Choice::Once),
        ("experience.always", Choice::Always),
        ("experience.never", Choice::Never),
        ("experience.cancel", Choice::Cancel),
    ] {
        let hit = frame
            .hits
            .iter()
            .find(|hit| hit.pressed.as_deref() == Some(action))
            .unwrap_or_else(|| panic!("{action} has no button"));
        assert!(hit.enabled, "{action}");
        let point = [
            frame.origin[0] + (hit.rect.x + hit.rect.w / 2.0) as f32 * frame.scale,
            frame.origin[1] + (hit.rect.y + hit.rect.h / 2.0) as f32 * frame.scale,
        ];
        assert_eq!(
            presentation.mod_ui().experience_choice(point),
            Some(choice),
            "{action}"
        );
    }
}
