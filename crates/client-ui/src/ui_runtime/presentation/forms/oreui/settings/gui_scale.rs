//! GUI-scale choices carry viewport-derived percentages and commit signed modifiers.

use super::super::super::super::UiPresentationError;
use super::super::super::menu_screens::Translate;
use super::super::paint::Canvas;
use super::super::theme::{self, BODY};
use super::super::widgets;
use super::{Content, picker, sections};
use launcher::menu::{MenuAction, MenuView};

fn label(percentage: u16, translate: Translate<'_>) -> String {
    translate("options.percent.format").map_or_else(
        || format!("{percentage}%"),
        |format| {
            format
                .replace("%s", &percentage.to_string())
                .replace("%%", "%")
        },
    )
}

pub(super) fn draw(content: &mut Content<'_, '_>) -> Result<(), UiPresentationError> {
    let title = content.word("options.guiScale.optionName.name");
    let enabled = content.view.gui_scale_choices.len() > 1;
    let description = content.word(if enabled {
        "options.guiScale.optionName.description"
    } else {
        "options.guiScale.disabled"
    });
    let labels: Vec<_> = content
        .view
        .gui_scale_choices
        .iter()
        .map(|choice| label(choice.percentage, content.translate))
        .collect();
    let [left, right] = content.inset();
    let use_picker = content.column_width < content.canvas.r(15.0 * labels.len() as f32);
    let cell_width = (right - left) / labels.len().max(1) as f32;
    let height = if use_picker {
        content.canvas.r(picker::SELECT_HEIGHT)
    } else {
        labels
            .iter()
            .try_fold(content.canvas.r(6.0), |height, label| {
                widgets::choice_height(content.canvas, label, cell_width)
                    .map(|choice| height.max(choice))
            })?
    };
    let extra = height + content.canvas.r(if use_picker { 0.8 } else { 0.4 });
    let bounds = content.row(&title, &description, right - left, None, extra)?;
    content.texts(bounds, &title, &description, right - left, extra)?;
    let control = [
        left,
        bounds[3] - height - content.canvas.r(1.2),
        right,
        bounds[3] - content.canvas.r(1.2),
    ];
    let selected = content
        .view
        .gui_scale_choices
        .iter()
        .position(|choice| choice.offset == content.view.gui_scale_offset)
        .unwrap_or(0);
    if !enabled {
        let outer = [
            control[0],
            control[1] + content.canvas.r(0.4),
            control[2],
            control[3],
        ];
        content.canvas.fill(outer, theme::DISABLED.fill)?;
        content
            .canvas
            .frame(outer, theme::EDGE, theme::DISABLED.border)?;
        if let Some(label) = labels.first() {
            content.canvas.text_centred(
                label,
                outer,
                BODY,
                content.canvas.role(theme::DISABLED).text,
                false,
            )?;
        }
        let centre = (outer[0] + outer[2]) * 0.5;
        let half = content.canvas.r(2.4).min((outer[2] - outer[0]) * 0.5);
        content.canvas.fill(
            [
                centre - half,
                outer[3] - content.canvas.r(0.4),
                centre + half,
                outer[3] - content.canvas.r(0.2),
            ],
            content.canvas.role(theme::DISABLED).text,
        )?;
    } else if use_picker {
        picker::select(
            content.canvas,
            content.view,
            control,
            &labels[selected],
            MenuAction::SettingsScalePicker,
        )?;
    } else {
        for focus in [false, true] {
            for (index, choice) in content.view.gui_scale_choices.iter().enumerate() {
                let bounds = [
                    left + index as f32 * cell_width - content.canvas.r(0.2),
                    control[1],
                    left + (index + 1) as f32 * cell_width,
                    control[3],
                ];
                let action = MenuAction::SettingsScale(choice.offset);
                if focus {
                    widgets::choice_focus(
                        content.canvas,
                        content.view,
                        bounds,
                        index == selected,
                        action,
                    )?;
                } else {
                    widgets::choice(
                        content.canvas,
                        content.view,
                        bounds,
                        &labels[index],
                        index == selected,
                        action,
                    )?;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn draw_picker(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    translate: Translate<'_>,
) -> Result<(), UiPresentationError> {
    let key = "options.guiScale.optionName.name";
    let title = translate(key).map_or_else(
        || sections::fallback(key).to_owned(),
        |text| text.to_string(),
    );
    picker::draw_choices(
        canvas,
        view,
        size,
        picker::Picker {
            title,
            labels: view
                .gui_scale_choices
                .iter()
                .map(|choice| label(choice.percentage, translate))
                .collect(),
            actions: view
                .gui_scale_choices
                .iter()
                .map(|choice| MenuAction::SettingsScale(choice.offset))
                .collect(),
            selected: view
                .gui_scale_choices
                .iter()
                .position(|choice| choice.offset == view.gui_scale_offset)
                .unwrap_or(0),
            close: MenuAction::SettingsScalePicker,
            scroll_key: "oreui_settings_picker/gui_scale".to_owned(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gui_scale_percentage_uses_the_active_translation_format() {
        let translated: Translate<'_> =
            &|key| (key == "options.percent.format").then(|| std::sync::Arc::from("%s %%"));
        assert_eq!(label(67, translated), "67 %");
        assert_eq!(label(50, &|_| None), "50%");
    }
}
