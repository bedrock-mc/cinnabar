use super::super::super::UiPresentationError;
use super::super::{
    join_progress,
    loading_screen::LoadingStage,
    menu_screens::{Translate, translated},
};
use super::{
    paint::{Bounds, Canvas},
    theme::{self, CAPTION, HEADER5, TEXT, TEXT_DIMMER},
    widgets::{self, Variant},
};
use crate::menu::{MenuAction, MenuView};

#[cfg(test)]
mod destination_tests;
#[cfg(test)]
mod motion_tests;
#[cfg(test)]
mod tests;
mod world;

pub(super) struct Progress<'a> {
    pub(super) title: &'a str,
    pub(super) detail: &'a str,
    pub(super) fraction: Option<f32>,
    pub(super) cancel: Option<MenuAction>,
    pub(super) indicator: bool,
    pub(super) stage: LoadingStage,
    pub(super) destination: Option<i32>,
}

pub(super) fn join(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    translate: Translate<'_>,
) -> Result<(), UiPresentationError> {
    widgets::screen_overlay(canvas, size)?;
    let tr = |key: &str, fallback: &str| translated(translate, key, fallback);
    let (title, detail, fraction, cancel) = if let Some(local) = &view.local.progress {
        use launcher::local_worlds::Stage;
        (
            tr("progressScreen.title.connectingLocal", "Starting World"),
            if local.detail.is_empty() {
                local.stage.title().to_owned()
            } else {
                format!("{}\n{}", local.stage.title(), local.detail)
            },
            local.fraction,
            (local.stage != Stage::Connecting)
                .then_some(MenuAction::LocalWorld(crate::menu::LocalWorldAction::Back)),
        )
    } else {
        let shown = join_progress::shown(&view.feeds.join, &tr);
        (
            shown.title,
            shown.message,
            shown.clipped.map(|clipped| (1.0 - clipped) as f32),
            shown.cancel.then_some(MenuAction::AddBack),
        )
    };
    draw(
        canvas,
        Some(view),
        size,
        &Progress {
            title: &title,
            detail: &detail,
            fraction,
            cancel,
            indicator: true,
            stage: LoadingStage::Connecting,
            destination: None,
        },
    )
}

/// Branding and status share one centered group through every loading stage.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: Option<&MenuView>,
    size: [f32; 2],
    progress: &Progress<'_>,
) -> Result<(), UiPresentationError> {
    world::backdrop(canvas, size, progress.stage, progress.destination)?;
    let entrance = canvas.begin_entrance(super::motion::Surface::Loading);
    let margin = canvas.r(2.0).min(size[0] * 0.05).min(size[1] * 0.05);
    let width = canvas.r(theme::LOADING_WIDTH).min(size[0] - margin * 2.0);
    let pad = canvas.r(theme::LOADING_PAD).min(width * 0.07);
    let inner = width - pad * 2.0;
    let title = progress.title.to_uppercase();
    let mut title_height = canvas.measure_height(&title, inner, HEADER5)?;
    let mut detail_height = if progress.detail.is_empty() {
        0.0
    } else {
        canvas.r(0.8) + canvas.measure_height(progress.detail, inner, CAPTION)?
    };
    let fraction = progress.fraction.filter(|value| value.is_finite());
    let destination_art = progress
        .destination
        .filter(|dimension| world::has_destination_art(canvas, *dimension));
    let loader_height = if destination_art.is_some() || progress.indicator && fraction.is_none() {
        canvas.r(6.4)
    } else {
        0.0
    };
    let mut body_height = loader_height + title_height + detail_height;
    let progress_height = if progress.indicator && fraction.is_some() {
        canvas.r(theme::LOADING_PROGRESS_AREA)
    } else {
        0.0
    };
    let footer_height = if progress.cancel.is_some() {
        canvas.r(theme::LOADING_FOOTER_AREA)
    } else {
        0.0
    };
    let logo = canvas.title_artwork.filter(|_| size[1] >= canvas.r(32.0));
    let logo_size = logo
        .map(|icon| {
            let ratio =
                f32::from(icon.uv[2] - icon.uv[0]) / f32::from((icon.uv[3] - icon.uv[1]).max(1));
            let w = canvas.r(36.0).min(width * 0.82).min(size[1] * 0.16 * ratio);
            [w, w / ratio]
        })
        .unwrap_or([0.0; 2]);
    let logo_height = if logo.is_some() {
        logo_size[1] + canvas.r(2.4)
    } else {
        0.0
    };
    let height = (pad * 2.0 + body_height + progress_height + footer_height)
        .min(size[1] - margin * 2.0 - logo_height);
    let group_top = (size[1] - height - logo_height) * 0.5;
    if let Some(icon) = logo {
        let x = (size[0] - logo_size[0]) * 0.5;
        canvas.icon_ref(
            icon,
            [x, group_top, x + logo_size[0], group_top + logo_size[1]],
        )?;
    }
    let x = (size[0] - width) * 0.5;
    let y = group_top + logo_height;
    let bounds = [x, y, x + width, y + height];
    let status = canvas.begin_entrance(status_surface(progress, view));
    world::panel(canvas, bounds, progress.stage)?;
    let left = x + pad;
    let right = x + width - pad;
    let body_bottom = bounds[3] - pad - progress_height - footer_height;
    let viewport = [left, y + pad, right, body_bottom];
    let content_width = inner
        - if body_height > viewport[3] - viewport[1] {
            canvas.r(1.6)
        } else {
            0.0
        };
    if content_width < inner {
        title_height = canvas.measure_height(&title, content_width, HEADER5)?;
        detail_height = if progress.detail.is_empty() {
            0.0
        } else {
            canvas.r(0.8) + canvas.measure_height(progress.detail, content_width, CAPTION)?
        };
        body_height = loader_height + title_height + detail_height;
    }
    let max = (body_height - (viewport[3] - viewport[1])).max(0.0);
    if let Some(offset) = canvas.offsets.get_mut("loading_status") {
        *offset = offset.clamp(0.0, max);
    }
    let scroll = canvas.begin_scroll("loading_status", viewport)?;
    let mut at = viewport[1] - scroll.offset;
    if loader_height > 0.0 {
        let side = canvas.r(4.8);
        let cx = (left + right) * 0.5;
        let bounds = [cx - side * 0.5, at, cx + side * 0.5, at + side];
        if let Some(dimension) = destination_art {
            let half_width = canvas.r(4.4).min(content_width * 0.5);
            world::destination(
                canvas,
                [cx - half_width, at, cx + half_width, at + side],
                dimension,
            )?;
        } else {
            loader(canvas, bounds)?;
        }
        at += loader_height;
    }
    canvas.centered_wrapped_text(&title, [left, at], content_width, HEADER5, TEXT)?;
    at += title_height;
    if detail_height > 0.0 {
        canvas.centered_wrapped_text(
            progress.detail,
            [left, at + canvas.r(theme::SPACE[1])],
            content_width,
            CAPTION,
            TEXT_DIMMER,
        )?;
    }
    canvas.end_scroll(scroll, body_height)?;
    if progress_height > 0.0 {
        let at = body_bottom + canvas.r(theme::SPACE[3]);
        track(
            canvas,
            [left, at, right, at + canvas.r(theme::PROGRESS_HEIGHT)],
            fraction,
        )?;
    }
    if let (Some(action), Some(view)) = (progress.cancel, view) {
        let button_width = canvas.r(theme::CANCEL_WIDTH).min(inner);
        let mid = (left + right) * 0.5;
        widgets::button(
            canvas,
            view,
            [
                mid - button_width * 0.5,
                bounds[3] - pad - canvas.r(theme::BUTTON_HEIGHT),
                mid + button_width * 0.5,
                bounds[3] - pad,
            ],
            Variant::Neutral,
            "Cancel",
            Some(action),
        )?;
    }
    canvas.end_entrance(status, size)?;
    canvas.end_entrance(entrance, size)
}

fn status_surface(progress: &Progress<'_>, view: Option<&MenuView>) -> super::motion::Surface {
    use std::hash::{Hash, Hasher};
    let mut key = std::collections::hash_map::DefaultHasher::new();
    std::mem::discriminant(&progress.stage).hash(&mut key);
    progress.destination.hash(&mut key);
    if let Some(view) = view {
        if let Some(local) = &view.local.progress {
            std::mem::discriminant(&local.stage).hash(&mut key);
        } else {
            std::mem::discriminant(&view.feeds.join.kind).hash(&mut key);
            std::mem::discriminant(&view.feeds.join.stage).hash(&mut key);
            if let crate::menu::JoinStage::Packs { total_bytes, .. } = view.feeds.join.stage {
                (total_bytes > 0).hash(&mut key);
            }
        }
    }
    super::motion::Surface::LoadingStatus(key.finish())
}

/// The installed cube loader supplies unknown progress; fallback dots never imply a percentage.
pub(super) fn loader(canvas: &mut Canvas<'_>, bounds: Bounds) -> Result<(), UiPresentationError> {
    if canvas.loading_sprite(bounds)? {
        return Ok(());
    }
    let side = canvas.r(0.8);
    let center = [(bounds[0] + bounds[2]) * 0.5, (bounds[1] + bounds[3]) * 0.5];
    let active = ((canvas.seconds.max(0.0) * 5.0) as usize) % 3;
    for index in 0..3 {
        let x = center[0] + (index as f32 - 1.0) * canvas.r(1.6);
        canvas.rotated_fill(
            [
                x - side * 0.5,
                center[1] - side * 0.5,
                x + side * 0.5,
                center[1] + side * 0.5,
            ],
            if index == active {
                theme::SUCCESS_TINT
            } else {
                theme::NEUTRAL.fill
            },
            std::f32::consts::FRAC_PI_4,
        )?;
    }
    Ok(())
}

fn track(
    canvas: &mut Canvas<'_>,
    bounds: Bounds,
    fraction: Option<f32>,
) -> Result<(), UiPresentationError> {
    canvas.fill(bounds, theme::NEUTRAL100)?;
    let edge = canvas.r(theme::EDGE);
    let inner = [
        bounds[0] + edge,
        bounds[1] + edge,
        bounds[2] - edge,
        bounds[3] - edge,
    ];
    canvas.fill(inner, theme::NEUTRAL.fill)?;
    let fraction = fraction
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let fraction = canvas
        .transitions
        .as_deref_mut()
        .map_or(fraction, |transitions| {
            transitions.progress.sample(
                canvas.surface,
                fraction,
                canvas.seconds,
                transitions.motion.enabled(),
            )
        });
    let right = inner[0] + (inner[2] - inner[0]) * fraction;
    canvas.fill(
        [inner[0], inner[1], right, inner[3]],
        theme::PRIMARY_ROLE.fill,
    )?;
    canvas.fill(
        [inner[0], inner[1], right, inner[1] + edge],
        theme::PRIMARY_ROLE.specular[0],
    )
}
