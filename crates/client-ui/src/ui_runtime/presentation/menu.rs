//! Original Cinnabar launcher shell.
//!
//! The layout borrows the information hierarchy of polished Java clients—one
//! persistent navigation system, compact multiplayer rows, and social data in
//! context—while keeping a restrained, rectangular visual identity built from
//! Cinnabar's own tokens and the server artwork supplied by Bedrock services.

#![allow(clippy::too_many_arguments)]

use ui::{SafeArea, TextError, TextLayout, TextLayoutCache, TextShadow, UiNode, UiRect};

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
    scrolls: &mut super::menu_scroll::MenuScrolls,
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

    let content = if matches!(view.screen, MenuScreen::Pause | MenuScreen::Death) {
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

    let content_start = nodes.len();
    let hit_start = hits.len();
    screens::append(
        view, nodes, &mut hits, next_id, layouts, font, metrics, solid_page, content,
    )?;
    if matches!(
        view.screen,
        MenuScreen::Play
            | MenuScreen::Social
            | MenuScreen::Servers
            | MenuScreen::Friends
            | MenuScreen::Inbox
    ) {
        scroll_content(
            view,
            nodes,
            &mut hits,
            next_id,
            content_start,
            hit_start,
            content,
            scrolls,
            safe_area,
        )?;
    }
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

/// Clips the flattened fallback content while keeping the launcher shell fixed.
fn scroll_content(
    view: &MenuView,
    nodes: &mut Vec<ui::UiNode>,
    hits: &mut Vec<(MenuAction, UiRect)>,
    next: &mut u32,
    start: usize,
    hit_start: usize,
    area: ContentArea,
    scrolls: &mut super::menu_scroll::MenuScrolls,
    safe_area: SafeArea,
) -> Result<(), UiPresentationError> {
    let viewport = rect(
        area.left,
        area.top,
        area.left + area.width,
        area.top + area.height,
    )?;
    let bottom = nodes[start..]
        .iter()
        .map(|node| node.bounds().max().y())
        .fold(viewport.max().y(), f32::max);
    let max = (bottom - viewport.max().y()).max(0.0);
    let focused = hits[hit_start..]
        .iter()
        .find(|(action, _)| Some(*action) == view.focused_action)
        .map(|(_, bounds)| *bounds);
    let offset = scrolls.reveal_focus(
        "fallback_content",
        view.focused_action,
        focused,
        viewport,
        max,
    );
    let id = ui::UiNodeId::new(*next);
    *next = next.saturating_add(1);
    for node in &mut nodes[start..] {
        if node.parent().is_none() {
            let b = node.bounds();
            *node = ui::UiNode::new(
                node.id(),
                Some(id),
                rect(
                    b.min().x() - area.left,
                    b.min().y() - area.top - offset,
                    b.max().x() - area.left,
                    b.max().y() - area.top - offset,
                )?,
            )
            .with_visual(node.visual().clone());
        }
    }
    nodes.insert(
        start,
        ui::UiNode::new(id, None, viewport).with_clip_children(true),
    );
    let content_hits = hits.split_off(hit_start);
    for (action, b) in content_hits {
        let (left, top, right, bottom) = (
            b.min().x().max(viewport.min().x()),
            (b.min().y() - offset).max(viewport.min().y()),
            b.max().x().min(viewport.max().x()),
            (b.max().y() - offset).min(viewport.max().y()),
        );
        if right > left && bottom > top {
            hits.push((action, rect(left, top, right, bottom)?));
        }
    }
    scrolls.set_areas(vec![super::menu_scroll::ScrollArea {
        key: "fallback_content".into(),
        viewport: rect(
            viewport.min().x() + safe_area.left(),
            viewport.min().y() + safe_area.top(),
            viewport.max().x() + safe_area.left(),
            viewport.max().y() + safe_area.top(),
        )?,
        scale: 1.0,
        offset,
        max,
        speed: CONTROL_HEIGHT,
        track: None,
        thumb: None,
        engine: None,
        draggable: false,
    }]);
    Ok(())
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
        156.0_f32.min((width * 0.26).max(128.0))
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
        let exit_width = 96.0;
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
    const MIN_PANEL_HEIGHT: f32 = 48.0;
    const PANEL_BOTTOM_MARGIN: f32 = 10.0;
    const TEXT_TOP_PADDING: f32 = 14.0;
    const TEXT_BOTTOM_PADDING: f32 = 4.0;

    let width = content.width.min(520.0);
    let left = content.left + content.width - width;
    let bottom_margin = PANEL_BOTTOM_MARGIN.min((content.height - 1.0).max(0.0));
    let panel_bottom = content.top + content.height - bottom_margin;
    let maximum_panel_height = (panel_bottom - content.top).max(1.0);
    let maximum_text_extent =
        (maximum_panel_height - TEXT_TOP_PADDING - TEXT_BOTTOM_PADDING).max(0.0);
    let text_width = (width - SPACE_XL).max(1.0);
    let visible_message = bounded_visible_text(message);
    let fitted = fitting_status_layout(
        layouts,
        font,
        metrics,
        visible_message,
        text_width,
        maximum_text_extent,
    )?;
    let text_extent = fitted.as_ref().map_or(0.0, |(_, extent)| *extent);
    let panel_height = MIN_PANEL_HEIGHT
        .max(TEXT_TOP_PADDING + text_extent + TEXT_BOTTOM_PADDING)
        .min(maximum_panel_height);
    let top = panel_bottom - panel_height;
    solid(
        nodes,
        next_id,
        solid_page,
        rect(left, top, left + width, panel_bottom)?,
        PANEL_RAISED,
    );
    solid(
        nodes,
        next_id,
        solid_page,
        rect(left, top, left + 4.0, panel_bottom)?,
        ACCENT,
    );
    if let Some((visible_message, _)) = fitted {
        text(
            nodes,
            next_id,
            layouts,
            font,
            metrics,
            solid_page,
            visible_message,
            [left + SPACE_MD, top + TEXT_TOP_PADDING],
            text_width,
            TEXT,
        )?;
    }
    Ok(())
}

fn fitting_status_layout<'a>(
    layouts: &mut TextLayoutCache,
    font: &assets::RuntimeFontCatalog,
    metrics: TextMetrics,
    message: &'a str,
    width: f32,
    maximum_extent: f32,
) -> Result<Option<(&'a str, f32)>, UiPresentationError> {
    if message.is_empty() || maximum_extent <= 0.0 {
        return Ok(None);
    }
    match layouts.layout(metrics.request(message, (width * 64.0) as u32, font)) {
        Ok(layout) => {
            let extent = text_visual_extent(&layout, metrics.shadow());
            if extent <= maximum_extent {
                return Ok(Some((message, extent)));
            }
        }
        Err(TextError::VisualWidthExceeded { .. } | TextError::WrapLineLimitExceeded { .. }) => {}
        Err(error) => return Err(UiPresentationError::Text(error)),
    }

    let mut boundaries = Vec::with_capacity(message.chars().count() + 1);
    boundaries.push(0);
    boundaries.extend(message.char_indices().skip(1).map(|(index, _)| index));
    boundaries.push(message.len());

    let mut low = 0usize;
    let mut high = boundaries.len() - 1;
    let mut best = None;
    while low < high {
        let middle = low + (high - low) / 2;
        let end = boundaries[middle];
        if end == 0 {
            low = middle + 1;
            continue;
        }
        let value = &message[..end];
        let layout = match layouts.layout(metrics.request(value, (width * 64.0) as u32, font)) {
            Ok(layout) => layout,
            Err(
                TextError::VisualWidthExceeded { .. } | TextError::WrapLineLimitExceeded { .. },
            ) => {
                high = middle;
                continue;
            }
            Err(error) => return Err(UiPresentationError::Text(error)),
        };
        let extent = text_visual_extent(&layout, metrics.shadow());
        if extent <= maximum_extent {
            best = Some((value, extent));
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    Ok(best)
}

fn text_visual_extent(layout: &TextLayout, shadow: TextShadow) -> f32 {
    let shadow_extent = match shadow {
        TextShadow::None => 0.0,
        TextShadow::Offset64(offset) => {
            f32::from(layout.key().scale_1024) / 1_024.0 * offset as f32 / 64.0
        }
    };
    let glyph_extent = layout
        .glyphs()
        .iter()
        .map(|glyph| glyph.bounds_64[3] as f32 / 64.0 + shadow_extent)
        .fold(0.0, f32::max);
    (layout.size_64()[1] as f32 / 64.0).max(glyph_extent)
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
        MenuDialog::Accounts => ("Accounts", "", MenuAction::DismissDialog),
        MenuDialog::SettingsResetGroup(group) => (
            "Reset to Default",
            "Do you really want to reset the settings?",
            MenuAction::SettingsConfirmResetGroup(group),
        ),
        MenuDialog::SettingsResetBindings(gamepad) => (
            "Reset to Default",
            "Reset all bindings to their defaults?",
            MenuAction::SettingsConfirmResetBindings(gamepad),
        ),
        MenuDialog::SettingsSupport(_) => ("Settings", "", MenuAction::DismissDialog),
        MenuDialog::StorageError => (
            "Storage",
            view.storage.error.as_deref().unwrap_or_default(),
            MenuAction::DismissDialog,
        ),
        MenuDialog::StorageDelete => (
            "Delete Cached Data permanently?",
            "Downloaded packs will be downloaded again when required.",
            MenuAction::SettingsStorage(
                crate::menu::settings_storage::StorageAction::ConfirmDelete,
            ),
        ),
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
        MenuScreen::DressingRoom => "Dressing Room",
        MenuScreen::Settings => "Settings",
        MenuScreen::AddServer => "Add server",
        MenuScreen::Pause => "Game menu",
        MenuScreen::Death => "You died!",
        MenuScreen::Inbox => "Inbox",
        MenuScreen::Friends => "Friends",
        MenuScreen::Store => "Marketplace",
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

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::menu::{MenuServerTab, SavedServer};
    #[test]
    fn review_fallback_destinations_can_scroll_to_the_last_saved_server() {
        let mut view = crate::menu::MenuView::new(true, "Test".into());
        view.screen = MenuScreen::Servers;
        view.server_tab = MenuServerTab::Saved;
        view.servers = (0..40)
            .map(|index| SavedServer {
                name: index.to_string(),
                address: "example.test".into(),
                favorite: false,
                last_joined_unix: 0,
            })
            .collect();
        let font = crate::ui_runtime::presentation::tests::fixture_font();
        let metrics =
            TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
        let mut scrolls = super::super::menu_scroll::MenuScrolls::default();
        let render = |scrolls: &mut super::super::menu_scroll::MenuScrolls| {
            let (mut nodes, mut next, mut layouts) =
                (Vec::new(), 1, TextLayoutCache::new(128, 1024 * 1024));
            append_menu_nodes(
                &view,
                &mut nodes,
                &mut next,
                &mut layouts,
                &font,
                metrics,
                0,
                1280.0,
                720.0,
                SafeArea::ZERO,
                scrolls,
            )
            .unwrap()
        };
        render(&mut scrolls);
        assert!(scrolls.wheel(ui::UiPoint::new(600.0, 400.0).unwrap(), -100000.0, true));
        let hits = render(&mut scrolls);
        assert!(
            hits.iter()
                .any(|(action, _)| *action == MenuAction::PlaySaved(39))
        );
    }
}
