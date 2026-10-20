//! Typed parsing of the recognised `shaders.properties` keys.

use super::{
    BufferSize, CustomImage, CustomTexture, CustomUniform, CustomUniformKind, FlipRule, ImageSize,
    ProgramRule, PropertiesDiagnostic, ScaleRule, ShaderProperties, SizeValue, TextureSource,
};
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
    } else if let Some(buffer) = key.strip_prefix("size.buffer.") {
        let (width, height) = first_of_two(value)?;
        properties.buffer_sizes.push(BufferSize {
            buffer: name(buffer, "buffer")?,
            width: size_value(width)?,
            height: size_value(height)?,
        });
    } else if let Some(target) = key.strip_prefix("flip.") {
        let (program, buffer) = split_target(target, "program", "buffer")?;
        properties.flips.push(FlipRule {
            program,
            buffer,
            flip: parse_bool(value)?,
        });
    } else if let Some(program) = key.strip_prefix("scale.") {
        properties.scales.push(scale_rule(program, value)?);
    } else if let Some(target) = key.strip_prefix("texture.") {
        let (stage, sampler) = if target == "noise" {
            ("noise".to_owned(), "noisetex".to_owned())
        } else {
            split_target(target, "stage", "sampler")?
        };
        properties.custom_textures.push(CustomTexture {
            stage,
            sampler,
            source: texture_source(value)?,
        });
    } else if let Some(image) = key.strip_prefix("image.") {
        properties.images.push(custom_image(image, value)?);
    } else if let Some(target) = key.strip_prefix("uniform.") {
        let uniform = custom_uniform(CustomUniformKind::Uniform, target, value, line)?;
        properties.custom_uniforms.push(uniform);
    } else if let Some(target) = key.strip_prefix("variable.") {
        let variable = custom_uniform(CustomUniformKind::Variable, target, value, line)?;
        properties.custom_uniforms.push(variable);
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

/// Splits `<first>.<rest>` at the first `.`; both parts must be non-empty.
fn split_target(target: &str, first: &str, rest: &str) -> Result<(String, String), String> {
    let Some((head, tail)) = target.split_once('.') else {
        return Err(format!("expected `<{first}>.<{rest}>`"));
    };
    Ok((name(head, first)?, name(tail, rest)?))
}

/// The two whitespace-separated tokens of `value`.
fn first_of_two(value: &str) -> Result<(&str, &str), String> {
    let mut tokens = value.split_whitespace();
    match (tokens.next(), tokens.next(), tokens.next()) {
        (Some(first), Some(second), None) => Ok((first, second)),
        _ => Err(format!("expected two values, found `{value}`")),
    }
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

/// Iris rule: a value containing '.' is a screen-relative fraction, else pixels.
fn size_value(text: &str) -> Result<SizeValue, String> {
    if text.contains('.') {
        positive_f32(text).map(SizeValue::Relative)
    } else {
        positive_u32(text).map(SizeValue::Absolute)
    }
}

/// Parses a positive viewport scale and its optional offset.
fn scale_rule(program: &str, value: &str) -> Result<ScaleRule, String> {
    let numbers = value
        .split_whitespace()
        .map(parse_f32)
        .collect::<Result<Vec<_>, _>>()?;
    let (scale, offset) = match *numbers.as_slice() {
        [scale] => (scale, [0.0, 0.0]),
        [scale, x, y] => (scale, [x, y]),
        _ => {
            return Err(format!(
                "expected `<scale> [<offsetX> <offsetY>]`, found `{value}`"
            ));
        }
    };
    if scale <= 0.0 {
        return Err(format!("scale `{scale}` must be positive"));
    }
    Ok(ScaleRule {
        program: name(program, "program")?,
        scale,
        offset,
    })
}

/// Classifies a texture value: extra tokens make a raw texture, a `:` a resource location,
/// anything else a pack file. Leading `/` is stripped from pack paths.
fn texture_source(value: &str) -> Result<TextureSource, String> {
    let mut tokens = value.split_whitespace();
    let Some(first) = tokens.next() else {
        return Err("missing texture path".to_owned());
    };
    let spec: Vec<String> = tokens.map(str::to_owned).collect();
    let path = first.trim_start_matches('/');
    if !spec.is_empty() {
        return Ok(TextureSource::Raw {
            path: name(path, "texture path")?,
            spec,
        });
    }
    if first.contains(':') {
        return Ok(TextureSource::Resource(first.to_owned()));
    }
    Ok(TextureSource::File(name(path, "texture path")?))
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

/// Parses a `<type>.<name>` custom uniform or variable target and its expression.
fn custom_uniform(
    kind: CustomUniformKind,
    target: &str,
    value: &str,
    line: u32,
) -> Result<CustomUniform, String> {
    let (ty, name) = split_target(target, "type", "name")?;
    let expr = Expr::parse(value).map_err(|err| format!("invalid expression: {err}"))?;
    Ok(CustomUniform {
        kind,
        ty,
        name,
        expr,
        line,
    })
}
