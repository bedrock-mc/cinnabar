use ui::{SafeArea, TextLayoutCache, UiNode, UiNodeId, UiRect, UiVisual};

use crate::menu::{MenuAction, MenuField, MenuView, auth::AuthState};

use super::{
    super::{
        components::{button, field, text},
        rect,
        tokens::*,
    },
    ContentArea, TextMetrics, UiPresentationError, panel, section_heading,
};

pub(super) fn profile(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    area: ContentArea,
) -> Result<(), UiPresentationError> {
    section_heading(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Profile & Character",
        "Account sign-in and client-side appearance",
        [area.left, area.top],
        area.width,
    )?;
    let top = area.top + 62.0;
    let identity_width = if area.compact {
        area.width
    } else {
        area.width * 0.56
    };
    panel(
        nodes,
        next_id,
        solid_page,
        area.left,
        top,
        identity_width,
        320.0,
    )?;
    if let Some(icon) = view.profile_icon {
        nodes.push(
            UiNode::new(
                UiNodeId::new(*next_id),
                None,
                rect(
                    area.left + SPACE_LG,
                    top + 36.0,
                    area.left + 120.0,
                    top + 148.0,
                )?,
            )
            .with_visual(UiVisual::Sprite {
                texture_page: icon.page,
                uv: icon.uv,
                color: [255; 4],
            }),
        );
        *next_id = next_id.saturating_add(1);
    }
    let (account_status, status_color) = match &view.auth_state {
        AuthState::SignedOut => ("Not signed in", MUTED),
        AuthState::Checking => ("Checking saved account…", MUTED),
        AuthState::AwaitingCode { .. } => ("Waiting for Microsoft sign-in", MUTED),
        AuthState::Authenticated => ("Microsoft account connected", SUCCESS),
        AuthState::Failed(message) => (message.as_str(), DANGER),
    };
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        &view.display_name,
        [area.left + 142.0, top + 46.0],
        identity_width - 166.0,
        TEXT,
    )?;
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        account_status,
        [area.left + 142.0, top + 78.0],
        identity_width - 166.0,
        status_color,
    )?;
    let action = if matches!(
        view.auth_state,
        AuthState::Checking | AuthState::AwaitingCode { .. }
    ) {
        MenuAction::CancelSignIn
    } else {
        MenuAction::StartSignIn
    };
    let label = match &view.auth_state {
        AuthState::Checking => "Cancel account check",
        AuthState::AwaitingCode { .. } => "Cancel sign-in",
        AuthState::Authenticated => "Check account again",
        AuthState::Failed(_) => "Try sign-in again",
        AuthState::SignedOut => "Sign in with Microsoft",
    };
    button(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        action,
        usize::MAX,
        label,
        area.left + 142.0,
        top + 104.0,
        (identity_width - 166.0).max(1.0),
        CONTROL_HEIGHT,
        SafeArea::ZERO,
    )?;
    if let AuthState::AwaitingCode { uri, code } = &view.auth_state {
        text(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            &format!("Open {uri} and enter {code}"),
            [area.left + SPACE_LG, top + 154.0],
            identity_width - SPACE_LG * 2.0,
            TEXT,
        )?;
    }
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        &format!(
            "{} Realms  •  {} joinable friends  •  {} catalog destinations",
            view.realms.len(),
            view.friends.len(),
            view.featured.len() + view.gatherings.len()
        ),
        [area.left + SPACE_LG, top + 238.0],
        identity_width - SPACE_LG * 2.0,
        MUTED,
    )?;
    if !area.compact {
        let right = area.left + identity_width + SPACE_MD;
        let right_width = area.width - identity_width - SPACE_MD;
        panel(nodes, next_id, solid_page, right, top, right_width, 320.0)?;
        section_heading(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            "Character",
            "Appearance controls",
            [right + SPACE_LG, top + SPACE_LG],
            right_width - SPACE_LG * 2.0,
        )?;
        text(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            "The live skin preview is available now. Persona, capes, and cosmetics need a dedicated account inventory backend before they can be edited safely.",
            [right + SPACE_LG, top + 92.0],
            right_width - SPACE_LG * 2.0,
            MUTED,
        )?;
    }
    Ok(())
}

pub(super) fn settings(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    area: ContentArea,
) -> Result<(), UiPresentationError> {
    section_heading(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Settings",
        "Client presentation and interaction preferences",
        [area.left, area.top],
        area.width,
    )?;
    let top = area.top + 62.0;
    panel(
        nodes, next_id, solid_page, area.left, top, area.width, 246.0,
    )?;
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Interface scale",
        [area.left + SPACE_LG, top + SPACE_LG],
        area.width - SPACE_LG * 2.0,
        TEXT,
    )?;
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Choose a fixed Java-style GUI scale. Auto scaling remains available from the command line for reference captures.",
        [area.left + SPACE_LG, top + 54.0],
        area.width - SPACE_LG * 2.0,
        MUTED,
    )?;
    let gap = SPACE_SM;
    let button_width = ((area.width - SPACE_LG * 2.0 - gap * 3.0) / 4.0).max(1.0);
    for scale in 1..=4u8 {
        button(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            MenuAction::SettingsScale(scale),
            usize::MAX,
            if view.gui_scale == scale {
                match scale {
                    1 => "1×  Selected",
                    2 => "2×  Selected",
                    3 => "3×  Selected",
                    _ => "4×  Selected",
                }
            } else {
                match scale {
                    1 => "1×",
                    2 => "2×",
                    3 => "3×",
                    _ => "4×",
                }
            },
            area.left + SPACE_LG + (scale - 1) as f32 * (button_width + gap),
            top + 116.0,
            button_width,
            TOUCH_CONTROL_HEIGHT,
            SafeArea::ZERO,
        )?;
    }
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Mouse, keyboard, controller D-pad, controller confirm/back, and touch selection share the same focus model.",
        [area.left + SPACE_LG, top + 190.0],
        area.width - SPACE_LG * 2.0,
        SUBTLE,
    )
}

pub(super) fn add_server(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    area: ContentArea,
) -> Result<(), UiPresentationError> {
    section_heading(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Add server",
        "Save a direct Bedrock endpoint to this device",
        [area.left, area.top],
        area.width,
    )?;
    let form_width = area.width.min(680.0);
    let left = area.left + (area.width - form_width) * 0.5;
    let top = area.top + 72.0;
    panel(nodes, next_id, solid_page, left, top, form_width, 354.0)?;
    field(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        MenuAction::AddName,
        usize::MAX,
        MenuField::Name,
        "Server name",
        &view.name,
        [left + SPACE_LG, top + SPACE_LG],
        form_width - SPACE_LG * 2.0,
    )?;
    field(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        MenuAction::AddAddress,
        usize::MAX,
        MenuField::Address,
        "Address (host:port)",
        &view.address,
        [left + SPACE_LG, top + 120.0],
        form_width - SPACE_LG * 2.0,
    )?;
    let button_width = (form_width - SPACE_LG * 2.0 - SPACE_SM) * 0.5;
    button(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        MenuAction::AddSaveConnect,
        usize::MAX,
        "Save & join",
        left + SPACE_LG,
        top + 224.0,
        button_width,
        TOUCH_CONTROL_HEIGHT,
        SafeArea::ZERO,
    )?;
    button(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        MenuAction::AddSave,
        usize::MAX,
        "Save",
        left + SPACE_LG + button_width + SPACE_SM,
        top + 224.0,
        button_width,
        TOUCH_CONTROL_HEIGHT,
        SafeArea::ZERO,
    )?;
    button(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        MenuAction::AddBack,
        usize::MAX,
        "Back",
        left + SPACE_LG,
        top + 286.0,
        form_width - SPACE_LG * 2.0,
        CONTROL_HEIGHT,
        SafeArea::ZERO,
    )
}

pub(super) fn pause(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    area: ContentArea,
) -> Result<(), UiPresentationError> {
    let width = area.width.min(460.0);
    let height = 314.0;
    let left = (area.width - width) * 0.5;
    let top = (area.height - height) * 0.5;
    panel(nodes, next_id, solid_page, left, top, width, height)?;
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "Game menu",
        [left + SPACE_LG, top + SPACE_LG],
        width - SPACE_LG * 2.0,
        TEXT,
    )?;
    for (index, (action, label)) in [
        (MenuAction::PauseResume, "Back to game"),
        (MenuAction::PauseSettings, "Settings"),
        (MenuAction::PauseDisconnect, "Disconnect"),
    ]
    .into_iter()
    .enumerate()
    {
        button(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            action,
            usize::MAX,
            label,
            left + SPACE_LG,
            top + 82.0 + index as f32 * 62.0,
            width - SPACE_LG * 2.0,
            TOUCH_CONTROL_HEIGHT,
            SafeArea::ZERO,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ui::{DpiScale, TextLayoutCache};

    use super::*;
    use crate::{
        menu::{MenuRuntime, MenuScreen, auth::AuthState},
        ui_runtime::presentation::tests::fixture_font,
    };

    #[test]
    fn profile_presents_signed_out_and_device_code_actions() {
        let mut runtime = MenuRuntime::new(true, 2, "Offline Player".to_owned());
        runtime.activate(MenuAction::Navigate(MenuScreen::Profile));
        let mut signed_out = runtime.view();
        let font = fixture_font();
        let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(1.0).unwrap(), Some(2));
        let area = ContentArea {
            left: 200.0,
            top: 80.0,
            width: 1000.0,
            height: 600.0,
            compact: false,
        };

        let render = |view: &MenuView| {
            let mut nodes = Vec::new();
            let mut hits = Vec::new();
            let mut next_id = 1;
            let mut layouts = TextLayoutCache::new(128, 1024 * 1024);
            profile(
                view,
                &mut nodes,
                &mut hits,
                &mut next_id,
                &mut layouts,
                &font,
                metrics,
                0,
                area,
            )
            .unwrap();
            (nodes, hits)
        };

        let (nodes, hits) = render(&signed_out);
        assert!(!nodes.is_empty());
        assert!(
            hits.iter()
                .any(|(action, _)| *action == MenuAction::StartSignIn)
        );

        signed_out.auth_state = AuthState::AwaitingCode {
            uri: "https://example.test/device".to_owned(),
            code: "ABCD-1234".to_owned(),
        };
        let (nodes, hits) = render(&signed_out);
        assert!(!nodes.is_empty());
        assert!(
            hits.iter()
                .any(|(action, _)| *action == MenuAction::CancelSignIn)
        );
        assert!(
            !hits
                .iter()
                .any(|(action, _)| *action == MenuAction::StartSignIn)
        );
    }
}
