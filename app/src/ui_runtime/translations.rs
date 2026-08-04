use std::{collections::BTreeMap, sync::Arc};

use protocol::{RawTextComponent, RawTextDocument};

pub(super) fn resolve_translation(
    translations: &BTreeMap<Box<str>, Box<str>>,
    message: Arc<str>,
    parameters: &[Arc<str>],
    should_translate: bool,
) -> Arc<str> {
    if !should_translate {
        return message;
    }
    let Some(template) = translations.get(message.as_ref()) else {
        return message;
    };
    Arc::from(format_translation(template, parameters))
}

pub(super) fn resolve_raw_text(
    translations: &BTreeMap<Box<str>, Box<str>>,
    document: &RawTextDocument,
) -> Option<Arc<str>> {
    let mut output = String::new();
    for component in document.components() {
        append_raw_text_component(translations, component, &mut output)?;
    }
    Some(Arc::from(output))
}

fn format_translation(template: &str, parameters: &[Arc<str>]) -> String {
    format_translation_bounded(template, parameters, protocol::MAX_UI_TEXT_BYTES)
}

fn format_translation_bounded(
    template: &str,
    parameters: &[Arc<str>],
    maximum_bytes: usize,
) -> String {
    let mut output = String::with_capacity(template.len());
    let mut characters = template.chars();
    let mut next_parameter = 0usize;
    while let Some(character) = characters.next() {
        if character != '%' {
            append_bounded_text(&mut output, &character.to_string(), maximum_bytes);
            continue;
        }
        let Some(specifier) = characters.next() else {
            append_bounded_text(&mut output, "%", maximum_bytes);
            break;
        };
        match specifier {
            '%' => append_bounded_text(&mut output, "%", maximum_bytes),
            's' => {
                if let Some(parameter) = parameters.get(next_parameter) {
                    append_bounded_text(&mut output, parameter, maximum_bytes);
                }
                next_parameter = next_parameter.saturating_add(1);
            }
            '1'..='9' => {
                let index = usize::from(specifier as u8 - b'1');
                if let Some(parameter) = parameters.get(index) {
                    append_bounded_text(&mut output, parameter, maximum_bytes);
                }
            }
            other => {
                let unknown = format!("%{other}");
                append_bounded_text(&mut output, &unknown, maximum_bytes);
            }
        }
    }
    output
}

fn append_raw_text_component(
    translations: &BTreeMap<Box<str>, Box<str>>,
    component: &RawTextComponent,
    output: &mut String,
) -> Option<()> {
    match component {
        RawTextComponent::Text(text) => {
            append_bounded_text(output, text, protocol::MAX_RAW_TEXT_OUTPUT_BYTES);
            Some(())
        }
        RawTextComponent::Translate { key, with } => {
            let mut parameters = Vec::with_capacity(with.len());
            for component in with.iter() {
                let mut parameter = String::new();
                append_raw_text_component(translations, component, &mut parameter)?;
                parameters.push(Arc::<str>::from(parameter));
            }
            let template = translations
                .get(key.as_ref())
                .map_or(key.as_ref(), |value| value.as_ref());
            let translated = format_translation_bounded(
                template,
                &parameters,
                protocol::MAX_RAW_TEXT_OUTPUT_BYTES,
            );
            append_bounded_text(output, &translated, protocol::MAX_RAW_TEXT_OUTPUT_BYTES);
            Some(())
        }
        RawTextComponent::Sequence(components) => {
            for component in components.iter() {
                append_raw_text_component(translations, component, output)?;
            }
            Some(())
        }
        RawTextComponent::Score { .. } | RawTextComponent::Selector(_) => None,
    }
}

fn append_bounded_text(output: &mut String, value: &str, maximum_bytes: usize) {
    if output.len() >= maximum_bytes {
        return;
    }
    let remaining = maximum_bytes - output.len();
    if value.len() <= remaining {
        output.push_str(value);
        return;
    }
    let mut end = remaining;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    output.push_str(&value[..end]);
}
