//! Typed parsing of the recognised `shaders.properties` keys.

use super::{CustomImage, ImageSize, ProgramRule, PropertiesDiagnostic, ShaderProperties};
use crate::expr::Expr;

/// Records the typed rule for a recognised `key`; a malformed value adds a diagnostic
/// instead. Unknown keys are ignored here (they stay in `entries`).
pub(super) fn apply(properties: &mut ShaderProperties, line: u32, key: &str, value: &str) {
    if let Err(message) = apply_rule(properties, line, key, value) {
        properties.diagnostics.push(PropertiesDiagnostic {
            line,
            message: format!("`{key}`: {message}"),
        });
    }
}

/// Parses a recognised property into its typed collection, returning malformed-value errors.
fn apply_rule(
    properties: &mut ShaderProperties,
    line: u32,
    key: &str,
    value: &str,
) -> Result<(), String> {
    if let Some(target) = key
        .strip_prefix("program.")
        .and_then(|rest| rest.strip_suffix(".enabled"))
    {
        properties
            .program_rules
            .push(program_rule(target, value, line)?);
    } else if let Some(image) = key.strip_prefix("image.") {
        properties.images.push(custom_image(image, value)?);
    }
    Ok(())
}

/// A non-empty name segment of a key, owned.
fn name(segment: &str, what: &str) -> Result<String, String> {
    if segment.is_empty() {
        return Err(format!("missing {what} name"));
    }
    Ok(segment.to_owned())
}

/// Parses true or false without regard to case, rejecting other values.
fn parse_bool(text: &str) -> Result<bool, String> {
    if text.eq_ignore_ascii_case("true") {
        Ok(true)
    } else if text.eq_ignore_ascii_case("false") {
        Ok(false)
    } else {
        Err(format!("expected `true` or `false`, found `{text}`"))
    }
}

/// Parses a finite floating-point value.
fn parse_f32(text: &str) -> Result<f32, String> {
    text.parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("`{text}` is not a number"))
}

/// Parses a finite, strictly positive floating-point value.
fn positive_f32(text: &str) -> Result<f32, String> {
    let value = parse_f32(text)?;
    if value > 0.0 {
        Ok(value)
    } else {
        Err(format!("`{text}` must be positive"))
    }
}

/// Parses a non-zero unsigned integer.
fn positive_u32(text: &str) -> Result<u32, String> {
    match text.parse::<u32>() {
        Ok(0) => Err(format!("`{text}` must be positive")),
        Ok(value) => Ok(value),
        Err(_) => Err(format!("`{text}` is not a whole number")),
    }
}

/// Parses a program enable expression and its optional dimension prefix.
fn program_rule(target: &str, value: &str, line: u32) -> Result<ProgramRule, String> {
    // Mapped dimension folders may be nested, so the program name follows the last slash.
    let (dimension, program) = match target.rsplit_once('/') {
        Some((dimension, program)) => (Some(name(dimension, "dimension")?), program),
        None => (None, target),
    };
    let expr = Expr::parse(value).map_err(|err| format!("invalid expression: {err}"))?;
    Ok(ProgramRule {
        dimension,
        program: name(program, "program")?,
        expr,
        line,
    })
}

/// Parses an image declaration with positive absolute or relative dimensions.
fn custom_image(image: &str, value: &str) -> Result<CustomImage, String> {
    let tokens: Vec<&str> = value.split_whitespace().collect();
    let &[
        sampler,
        format,
        internal_format,
        pixel_type,
        clear,
        relative,
        ref size @ ..,
    ] = tokens.as_slice()
    else {
        return Err(image_usage(value));
    };
    let size = match (parse_bool(relative)?, size) {
        (true, &[width, height]) => ImageSize::Relative {
            width: positive_f32(width)?,
            height: positive_f32(height)?,
        },
        (false, &[width]) => ImageSize::Absolute {
            width: positive_u32(width)?,
            height: None,
            depth: None,
        },
        (false, &[width, height]) => ImageSize::Absolute {
            width: positive_u32(width)?,
            height: Some(positive_u32(height)?),
            depth: None,
        },
        (false, &[width, height, depth]) => ImageSize::Absolute {
            width: positive_u32(width)?,
            height: Some(positive_u32(height)?),
            depth: Some(positive_u32(depth)?),
        },
        _ => return Err(image_usage(value)),
    };
    Ok(CustomImage {
        name: name(image, "image")?,
        sampler: sampler.to_owned(),
        format: format.to_owned(),
        internal_format: internal_format.to_owned(),
        pixel_type: pixel_type.to_owned(),
        clear: parse_bool(clear)?,
        size,
    })
}

/// Describes the expected image declaration alongside the rejected value.
fn image_usage(value: &str) -> String {
    format!(
        "expected `<sampler> <format> <internalFormat> <pixelType> <clear> <relative> \
         <width> [<height> [<depth>]]`, found `{value}`"
    )
}
