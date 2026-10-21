//! Mod update status uses the pack downloader's measurements, units and progress widgets.

use super::{Content, caption, theme};
use launcher::{
    download::{file_size, format_eta},
    mods::{UpdatePhase, UpdateProgress},
};
use ui_engine::engine::screen::PaintError;

/// Shows the actual phase, with a determinate byte bar only while the total is known.
pub(super) fn draw(
    content: &mut Content<'_, '_>,
    progress: &UpdateProgress,
) -> Result<(), PaintError> {
    let downloading = progress.phase == UpdatePhase::Downloading;
    let title = if downloading && progress.total_bytes > 0 {
        let percent = progress
            .received_bytes
            .min(progress.total_bytes)
            .saturating_mul(100)
            / progress.total_bytes;
        format!("{} {percent}%", progress.phase.label())
    } else {
        progress.phase.label().into()
    };
    caption(content, &title)?;
    if downloading && progress.total_bytes > 0 {
        let tr = |key: &str, fallback: &str| {
            (content.translate)(key).map_or_else(|| fallback.to_owned(), |text| text.to_string())
        };
        let mut text = format!(
            "{} / {}",
            file_size(progress.received_bytes, &tr),
            file_size(progress.total_bytes, &tr)
        );
        if let Some(rate) = progress.bytes_per_sec {
            text.push_str(&format!(" · {}/s", file_size(rate, &tr)));
        }
        if let Some(eta) = progress.eta_secs {
            text.push_str(&format!(" · {} left", format_eta(eta)));
        }
        caption(content, &text)?;
        let [left, right] = content.inset();
        let height = content.canvas.r(theme::PROGRESS_HEIGHT);
        let fraction =
            progress.received_bytes.min(progress.total_bytes) as f32 / progress.total_bytes as f32;
        crate::progress::track(
            content.canvas,
            [left, content.y, right, content.y + height],
            Some(fraction),
        )?;
        content.y += height + content.canvas.r(theme::SPACE[2]);
    } else {
        let [left, _] = content.inset();
        let side = content.canvas.r(2.4);
        crate::progress::loader(
            content.canvas,
            [left, content.y, left + side, content.y + side],
        )?;
        content.y += side + content.canvas.r(theme::SPACE[2]);
    }
    Ok(())
}
