use protocol::{TitleAction, TitleEvent};
use ui::TitleDurations;

use super::{UiRuntime, UiRuntimeError};

pub(super) fn apply_title(
    runtime: &mut UiRuntime,
    event: TitleEvent,
    fifo_sequence: u64,
    event_millis: u64,
) -> Result<(), UiRuntimeError> {
    match event.action {
        TitleAction::Clear => runtime.hud.clear_titles(),
        TitleAction::Reset => runtime.hud.reset_titles(),
        TitleAction::SetTitle | TitleAction::SetTitleJson => {
            runtime
                .hud
                .set_title(event.text, fifo_sequence, event_millis);
        }
        TitleAction::SetSubtitle | TitleAction::SetSubtitleJson => {
            runtime
                .hud
                .set_subtitle(event.text, fifo_sequence, event_millis);
        }
        TitleAction::ActionBar | TitleAction::ActionBarJson => {
            runtime
                .hud
                .set_actionbar(event.text, fifo_sequence, event_millis);
        }
        TitleAction::SetDurations => {
            let durations = TitleDurations::from_wire(
                event.fade_in_ticks,
                event.stay_ticks,
                event.fade_out_ticks,
            )
            .ok_or(UiRuntimeError::InvalidTitleDurations)?;
            runtime.hud.set_durations(durations);
        }
    }
    Ok(())
}
