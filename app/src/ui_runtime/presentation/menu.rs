//! Original Cinnabar launcher shell.
//!
//! The layout borrows the information hierarchy of polished Java clients—one
//! persistent navigation system, compact multiplayer rows, and social data in
//! context—while keeping a restrained, rectangular visual identity built from
//! Cinnabar's own tokens and the server artwork supplied by Bedrock services.

#![allow(clippy::too_many_arguments)]

use ui::{SafeArea, TextLayoutCache, UiNode, UiRect};

use crate::menu::{MenuAction, MenuDialog, MenuScreen, MenuView};

use super::{TextMetrics, UiPresentationError, bounded_visible_text, rect};

mod components;
mod screens;
mod tokens;

use components::{button, solid, text};
use tokens::*;

#[derive(Clone, Copy)]
pub(super) struct ContentArea {
    pub(super) left: f32,
    pub(super) top: f32,
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) compact: bool,
}

pub(super) fn append_menu_nodes(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    width: f32,
    height: f32,
    safe_area: SafeArea,
) -> Result<Vec<(MenuAction, UiRect)>, UiPresentationError> {
    if !view.visible {
        return Ok(Vec::new());
    }
    let mut hits = Vec::new();
    solid(
        nodes,
        next_id,
        solid_page,
        rect(0.0, 0.0, width, height)?,
        CANVAS,
    );

    let content = if view.screen == MenuScreen::Pause {
        ContentArea {
            left: 0.0,
            top: 0.0,
            width,
            height,
            compact: width < COMPACT_BREAKPOINT,
        }
    } else {
        append_shell(
            view, nodes, &mut hits, next_id, layouts, font, metrics, solid_page, width, height,
        )?
    };

    screens::append(
        view, nodes, &mut hits, next_id, layouts, font, metrics, solid_page, content,
    )?;
    if let Some(message) = view.message.as_deref() {
        append_toast(
            nodes, next_id, layouts, font, metrics, solid_page, content, message,
        )?;
    }
    if let Some(dialog) = view.dialog {
        append_dialog(
            view, nodes, &mut hits, next_id, layouts, font, metrics, solid_page, width, height,
            dialog,
        )?;
    }

    Ok(hits
        .into_iter()
        .map(|(action, bounds)| {
            let translated = rect(
                bounds.min().x() + safe_area.left(),
                bounds.min().y() + safe_area.top(),
                bounds.max().x() + safe_area.left(),
                bounds.max().y() + safe_area.top(),
            )
            .expect("menu hit rectangles are finite");
            (action, translated)
        })
        .collect())
}

fn append_shell(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    width: f32,
    height: f32,
) -> Result<ContentArea, UiPresentationError> {
    solid(
        nodes,
        next_id,
        solid_page,
        rect(0.0, 0.0, width, TOP_BAR_HEIGHT)?,
        TOP_BAR,
    );
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        "CINNABAR",
        [SPACE_LG, 21.0],
        210.0,
        ACCENT,
    )?;
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        screen_title(view.screen),
        [SPACE_LG, 48.0],
        260.0,
        MUTED,
    )?;

    let compact = width < COMPACT_BREAKPOINT;
    let account_width = if compact {
        168.0_f32.min((width * 0.28).max(132.0))
    } else {
        224.0_f32.min((width * 0.3).max(150.0))
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
        MenuAction::Navigate(MenuScreen::Profile),
        usize::MAX,
        &view.display_name,
        width - account_width - SPACE_LG,
        19.0,
        account_width,
        CONTROL_HEIGHT,
        SafeArea::ZERO,
    )?;

    if compact {
        let exit_width = 76.0;
        button(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            MenuAction::OpenExitDialog,
            usize::MAX,
            "Exit",
            width - account_width - SPACE_LG - exit_width - SPACE_SM,
            19.0,
            exit_width,
            CONTROL_HEIGHT,
            SafeArea::ZERO,
        )?;
        let nav_top = TOP_BAR_HEIGHT + SPACE_SM;
        let gap = SPACE_XS;
        let available = (width - SPACE_MD * 2.0 - gap * 5.0).max(1.0);
        let nav_width = available / 6.0;
        for (index, (screen, label)) in nav_items().into_iter().enumerate() {
            nav_button(
                view,
                nodes,
                hits,
                next_id,
                layouts,
                font,
                metrics,
                solid_page,
                screen,
                label,
                [SPACE_MD + index as f32 * (nav_width + gap), nav_top],
                nav_width,
                true,
            )?;
        }
        Ok(ContentArea {
            left: SPACE_MD,
            top: nav_top + TOUCH_CONTROL_HEIGHT + SPACE_MD,
            width: (width - SPACE_MD * 2.0).max(1.0),
            height: (height - nav_top - TOUCH_CONTROL_HEIGHT - SPACE_XL).max(1.0),
            compact,
        })
    } else {
        let sidebar_top = TOP_BAR_HEIGHT + SPACE_MD;
        let sidebar_height = (height - sidebar_top - SPACE_MD).max(1.0);
        solid(
            nodes,
            next_id,
            solid_page,
            rect(
                SPACE_MD,
                sidebar_top,
                SPACE_MD + SIDEBAR_WIDTH,
                sidebar_top + sidebar_height,
            )?,
            SIDEBAR,
        );
        for (index, (screen, label)) in nav_items().into_iter().enumerate() {
            nav_button(
                view,
                nodes,
                hits,
                next_id,
                layouts,
                font,
                metrics,
                solid_page,
                screen,
                label,
                [
                    SPACE_MD + SPACE_SM,
                    sidebar_top + SPACE_MD + index as f32 * (CONTROL_HEIGHT + SPACE_SM),
                ],
                SIDEBAR_WIDTH - SPACE_MD * 1.25,
                false,
            )?;
        }
        button(
            view,
            nodes,
            hits,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            MenuAction::OpenExitDialog,
            usize::MAX,
            "Quit Cinnabar",
            SPACE_MD + SPACE_SM,
            height - SPACE_MD - CONTROL_HEIGHT - SPACE_SM,
            SIDEBAR_WIDTH - SPACE_MD * 1.25,
            CONTROL_HEIGHT,
            SafeArea::ZERO,
        )?;
        let left = SPACE_MD + SIDEBAR_WIDTH + SPACE_LG;
        Ok(ContentArea {
            left,
            top: sidebar_top,
            width: (width - left - SPACE_LG).max(1.0),
            height: sidebar_height,
            compact,
        })
    }
}

fn nav_button(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    screen: MenuScreen,
    label: &str,
    position: [f32; 2],
    width: f32,
    compact: bool,
) -> Result<(), UiPresentationError> {
    let action = MenuAction::Navigate(screen);
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
        position[0],
        position[1],
        width,
        TOUCH_CONTROL_HEIGHT,
        SafeArea::ZERO,
    )?;
    if view.screen == screen {
        let indicator = if compact {
            rect(
                position[0],
                position[1] + TOUCH_CONTROL_HEIGHT - 3.0,
                position[0] + width,
                position[1] + TOUCH_CONTROL_HEIGHT,
            )?
        } else {
            rect(
                position[0],
                position[1],
                position[0] + 3.0,
                position[1] + TOUCH_CONTROL_HEIGHT,
            )?
        };
        solid(nodes, next_id, solid_page, indicator, ACCENT);
    }
    Ok(())
}

fn append_toast(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    content: ContentArea,
    message: &str,
) -> Result<(), UiPresentationError> {
    let width = content.width.min(520.0);
    let left = content.left + content.width - width;
    let top = content.top + content.height - 58.0;
    solid(
        nodes,
        next_id,
        solid_page,
        rect(left, top, left + width, top + 48.0)?,
        PANEL_RAISED,
    );
    solid(
        nodes,
        next_id,
        solid_page,
        rect(left, top, left + 4.0, top + 48.0)?,
        ACCENT,
    );
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        message,
        [left + SPACE_MD, top + 14.0],
        width - SPACE_XL,
        TEXT,
    )
}

fn append_dialog(
    view: &MenuView,
    nodes: &mut Vec<UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next_id: &mut u32,
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    solid_page: u16,
    width: f32,
    height: f32,
    dialog: MenuDialog,
) -> Result<(), UiPresentationError> {
    solid(
        nodes,
        next_id,
        solid_page,
        rect(0.0, 0.0, width, height)?,
        SCRIM,
    );
    let panel_width = width.clamp(300.0, 480.0);
    let panel_height = 214.0;
    let left = (width - panel_width) * 0.5;
    let top = (height - panel_height) * 0.5;
    solid(
        nodes,
        next_id,
        solid_page,
        rect(left, top, left + panel_width, top + panel_height)?,
        DANGER,
    );
    solid(
        nodes,
        next_id,
        solid_page,
        rect(
            left + 2.0,
            top + 2.0,
            left + panel_width - 2.0,
            top + panel_height - 2.0,
        )?,
        PANEL,
    );
    let (title, description, confirm) = match dialog {
        MenuDialog::Exit => (
            "Quit Cinnabar?",
            "Your saved servers and account cache will remain available.",
            MenuAction::ConfirmExit,
        ),
        MenuDialog::RemoveSaved(index) => (
            "Remove saved server?",
            view.servers
                .get(index)
                .map(|server| server.name.as_str())
                .unwrap_or("This server will be removed from your list."),
            MenuAction::ConfirmRemoveSaved(index),
        ),
    };
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        title,
        [left + SPACE_LG, top + SPACE_LG],
        panel_width - SPACE_LG * 2.0,
        TEXT,
    )?;
    text(
        nodes,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        description,
        [left + SPACE_LG, top + 68.0],
        panel_width - SPACE_LG * 2.0,
        MUTED,
    )?;
    let button_width = (panel_width - SPACE_LG * 2.0 - SPACE_SM) * 0.5;
    button(
        view,
        nodes,
        hits,
        next_id,
        layouts,
        font,
        metrics,
        solid_page,
        confirm,
        usize::MAX,
        "Confirm",
        left + SPACE_LG,
        top + 142.0,
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
        MenuAction::DismissDialog,
        usize::MAX,
        "Cancel",
        left + SPACE_LG + button_width + SPACE_SM,
        top + 142.0,
        button_width,
        TOUCH_CONTROL_HEIGHT,
        SafeArea::ZERO,
    )
}

const fn screen_title(screen: MenuScreen) -> &'static str {
    match screen {
        MenuScreen::Home => "Home",
        MenuScreen::Play => "Play",
        MenuScreen::Social => "Friends & Social",
        MenuScreen::Servers => "Servers",
        MenuScreen::Profile => "Profile & Character",
        MenuScreen::Settings => "Settings",
        MenuScreen::AddServer => "Add server",
        MenuScreen::Pause => "Game menu",
    }
}

const fn nav_items() -> [(MenuScreen, &'static str); 6] {
    [
        (MenuScreen::Home, "Home"),
        (MenuScreen::Play, "Play"),
        (MenuScreen::Social, "Social"),
        (MenuScreen::Servers, "Servers"),
        (MenuScreen::Profile, "Profile"),
        (MenuScreen::Settings, "Settings"),
    ]
}
