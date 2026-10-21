//! Device limits for a view's exact pack resource requests.

use super::PackGpu;
use shaderpack::ImageSize;
use wgpu::{Limits, TextureDimension};

/// Resolves a custom image without changing its requested absolute dimensions.
pub(in crate::shaderpack) fn image_extent(
    size: ImageSize,
    screen: [u32; 2],
) -> (TextureDimension, [u32; 3]) {
    match size {
        ImageSize::Relative { width, height } => (
            TextureDimension::D2,
            [
                ((screen[0] as f32 * width) as u32).max(1),
                ((screen[1] as f32 * height) as u32).max(1),
                1,
            ],
        ),
        ImageSize::Absolute {
            width,
            height: Some(height),
            depth: Some(depth),
        } => (TextureDimension::D3, [width, height, depth]),
        ImageSize::Absolute {
            width,
            height: Some(height),
            depth: None,
        } => (TextureDimension::D2, [width, height, 1]),
        ImageSize::Absolute {
            width,
            height: None,
            ..
        } => (TextureDimension::D1, [width, 1, 1]),
    }
}

/// Rejects empty extents or dimensions beyond this device's limit for the texture kind.
pub(in crate::shaderpack) fn texture_extent(
    dimension: TextureDimension,
    size: [u32; 3],
    limits: &Limits,
) -> Result<(), String> {
    let limit = match dimension {
        TextureDimension::D1 => limits.max_texture_dimension_1d,
        TextureDimension::D2 => limits.max_texture_dimension_2d,
        TextureDimension::D3 => limits.max_texture_dimension_3d,
    };
    if size.iter().any(|&value| value == 0 || value > limit) {
        return Err(format!(
            "{dimension:?} texture extent {size:?} exceeds device limit {limit}"
        ));
    }
    Ok(())
}

/// Rejects storage buffers that cannot be allocated and bound whole on this device.
fn storage_size(size: u64, limits: &Limits) -> Result<(), String> {
    let limit = limits
        .max_buffer_size
        .min(limits.max_storage_buffer_binding_size);
    if size == 0 || size > limit {
        return Err(format!(
            "storage buffer size {size} exceeds device binding limit {limit}"
        ));
    }
    Ok(())
}

/// Rejects repeated active buffer attachments before a render pass can use them.
fn validate_attachments(
    program: &str,
    kind: &str,
    buffers: impl IntoIterator<Item = u32>,
) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for buffer in buffers {
        if !seen.insert(buffer) {
            return Err(format!("{program} attaches {kind}{buffer} more than once"));
        }
    }
    Ok(())
}

/// Checks every view-sized request before any resources are created.
pub(super) fn validate(pack: &PackGpu, screen: [u32; 2], limits: &Limits) -> Result<(), String> {
    texture_extent(TextureDimension::D2, [screen[0], screen[1], 1], limits)?;
    for program in &pack.programs {
        validate_attachments(
            &program.name,
            "colortex",
            program
                .reflection
                .color_outputs
                .iter()
                .filter_map(|output| {
                    program
                        .color_targets
                        .get(output.location as usize)
                        .copied()
                        .unwrap_or(false)
                        .then_some(output.buffer)
                        .flatten()
                }),
        )?;
        if let Some(dispatch) = &program.reflection.compute {
            let size = super::super::dispatch::extent(
                program.group,
                screen,
                pack.shadow.as_ref().map(|shadow| shadow.slots.resolution),
            );
            super::super::dispatch::validate(dispatch, size, limits)
                .map_err(|error| format!("{}: {error}", program.name))?;
        }
    }
    for draw in &pack.draws {
        let program = &pack.programs[draw.program];
        validate_attachments(
            &program.name,
            if draw.kind.shadow() {
                "shadowcolor"
            } else {
                "colortex"
            },
            draw.outputs.iter().flatten().copied(),
        )?;
        if draw.kind.shadow() {
            continue;
        }
        let read_alt = super::draw_reads(pack, draw).alt;
        let write_alt = if draw.kind.after_deferred() {
            pack.frame.translucent_alt
        } else {
            pack.frame.scene_alt
        };
        for source in &program.images {
            if let crate::shaderpack::resources::ImageSource::Buffer(buffer) = source
                && draw.outputs.contains(&Some(*buffer))
                && (read_alt ^ write_alt) & (1 << buffer) == 0
            {
                return Err(format!(
                    "{} binds colortex{buffer} as both storage image and attachment",
                    program.name
                ));
            }
        }
    }
    for buffer in &pack.buffers {
        let [width, height] = buffer.extent.resolve(screen);
        texture_extent(TextureDimension::D2, [width, height, 1], limits)?;
    }
    for image in &pack.custom_images {
        let (dimension, size) = image_extent(image.size, screen);
        texture_extent(dimension, size, limits)
            .map_err(|error| format!("{}: {error}", image.name))?;
    }
    for &(binding, size) in &pack.shader_storage {
        storage_size(size, limits)
            .map_err(|error| format!("storage binding {binding}: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod attachment_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_dimensional_limits_apply_to_every_axis_without_clamping() {
        let limits = Limits {
            max_texture_dimension_2d: 8192,
            max_texture_dimension_3d: 2048,
            ..Default::default()
        };
        let (dimension, size) = image_extent(
            ImageSize::Absolute {
                width: 4096,
                height: Some(1),
                depth: Some(1),
            },
            [1280, 720],
        );
        assert_eq!(size, [4096, 1, 1]);
        assert!(texture_extent(dimension, size, &limits).is_err());
        assert!(texture_extent(TextureDimension::D3, [2048, 1, 1], &limits).is_ok());
        assert!(texture_extent(TextureDimension::D2, [4096, 1, 1], &limits).is_ok());
    }

    #[test]
    fn scaled_targets_and_storage_bindings_respect_device_limits() {
        let limits = Limits {
            max_texture_dimension_2d: 2048,
            max_buffer_size: 8192,
            max_storage_buffer_binding_size: 1024,
            ..Default::default()
        };
        let size = super::super::super::plan::Extent::Relative([2.0, 1.0]).resolve([1280, 720]);
        assert!(texture_extent(TextureDimension::D2, [size[0], size[1], 1], &limits).is_err());
        assert!(storage_size(1024, &limits).is_ok());
        assert!(storage_size(1040, &limits).is_err());
    }
}
