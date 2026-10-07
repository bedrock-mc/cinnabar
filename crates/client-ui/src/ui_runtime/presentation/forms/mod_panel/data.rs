use json_ui::{DataSource, Scalar};
use serde_json::json;
use ui::mod_panel::{Control, Panel, Theme};

use super::widgets::Palette;

pub(super) fn control_data(panel: &Panel) -> DataSource {
    let mut data = DataSource::new();
    let palette = Palette::for_panel(panel);
    for (index, control) in panel.controls.iter().enumerate() {
        let value = match control {
            Control::Toggle { value, .. } => if *value { "On" } else { "Off" }.to_owned(),
            Control::Slider { value, .. } => format!("{value:.2}")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_owned(),
            Control::Button { .. } => String::new(),
            Control::Keybind { key, capturing, .. } => {
                if *capturing {
                    "...".to_owned()
                } else {
                    key_label(key)
                }
            }
            Control::Choice { index, options, .. } => options[*index as usize].clone(),
        };
        data.set_global(
            format!("#row_{index}_label"),
            Scalar::Text(control.label().to_owned()),
        );
        data.set_global(format!("#row_{index}_value"), Scalar::Text(value));
        match control {
            Control::Slider {
                value, min, max, ..
            } => data.set_global(
                format!("#row_{index}_fill"),
                Scalar::Num(f64::from((value - min) / (max - min))),
            ),
            Control::Toggle { value, .. } => {
                data.set_global(
                    format!("#row_{index}_toggle_color"),
                    Scalar::Json(json!(if *value {
                        palette.accent
                    } else {
                        palette.raised
                    })),
                );
                data.set_global(
                    format!("#row_{index}_knob_color"),
                    Scalar::Json(json!(if panel.theme == Theme::Monochrome {
                        if *value {
                            palette.background
                        } else {
                            palette.text
                        }
                    } else {
                        [1.; 4]
                    })),
                );
                data.set_global(
                    format!("#row_{index}_knob_offset"),
                    Scalar::Json(json!([if *value { 12.0 } else { 2.0 }, 2.0])),
                );
            }
            _ => {}
        }
    }
    data
}

pub(super) fn key_label(key: &str) -> String {
    let label = match key {
        "ControlLeft" => "LCtrl",
        "ControlRight" => "RCtrl",
        "ShiftLeft" => "LShift",
        "ShiftRight" => "RShift",
        "AltLeft" => "LAlt",
        "AltRight" => "RAlt",
        "SuperLeft" | "MetaLeft" => "LWin",
        "SuperRight" | "MetaRight" => "RWin",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        "ArrowUp" => "Up",
        "ArrowDown" => "Down",
        "Backspace" => "Bksp",
        "CapsLock" => "Caps",
        "Escape" => "Esc",
        "PageUp" => "PgUp",
        "PageDown" => "PgDn",
        "PrintScreen" => "PrtSc",
        "ScrollLock" => "ScrLk",
        "NumLock" => "NumLk",
        "ContextMenu" => "Menu",
        "Delete" => "Del",
        "Insert" => "Ins",
        "Backquote" => "`",
        "Backslash" => "\\",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        "Semicolon" => ";",
        "Quote" => "'",
        "Minus" => "-",
        "Equal" => "=",
        "IntlBackslash" => "Intl\\",
        "NumpadAdd" => "Num+",
        "NumpadSubtract" => "Num-",
        "NumpadMultiply" => "Num*",
        "NumpadDivide" => "Num/",
        "NumpadDecimal" => "Num.",
        "NumpadEnter" => "NumEnt",
        "NumpadEqual" => "Num=",
        "NumpadComma" => "Num,",
        "NumpadHash" => "Num#",
        "NumpadParenLeft" => "Num(",
        "NumpadParenRight" => "Num)",
        "NumpadBackspace" => "NumBk",
        "NumpadMemoryAdd" => "NumM+",
        "NumpadMemorySubtract" => "NumM-",
        "NumpadMemoryStore" => "NumMS",
        "NumpadMemoryRecall" => "NumMR",
        "NumpadMemoryClear" => "NumMC",
        "NumpadClear" => "NumClr",
        "NumpadClearEntry" => "NumCE",
        "AudioVolumeMute" => "Mute",
        "AudioVolumeUp" => "Vol+",
        "AudioVolumeDown" => "Vol-",
        "MediaPlayPause" => "Play",
        "MediaStop" => "Stop",
        "MediaTrackNext" => "Next",
        "MediaTrackPrevious" => "Prev",
        "LaunchMail" => "Mail",
        "LaunchApp1" => "App1",
        "LaunchApp2" => "App2",
        "MediaSelect" => "Media",
        "BrowserBack" => "Back",
        "BrowserForward" => "Fwd",
        "BrowserRefresh" => "Reload",
        "BrowserStop" => "Stop",
        "BrowserSearch" => "Search",
        "BrowserFavorites" => "Fav",
        "BrowserHome" => "Home",
        _ => key
            .strip_prefix("Key")
            .or_else(|| key.strip_prefix("Digit"))
            .unwrap_or(key),
    };
    if let Some(digit) = label.strip_prefix("Numpad") {
        format!("Num{digit}")
    } else {
        label.to_owned()
    }
}
