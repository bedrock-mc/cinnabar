use std::path::{Path, PathBuf};

use assets::AssetError;

use super::{Command, validate_output_bundle};

/// Rejects aliases among every explicit input and output, including generated sidecars.
pub(super) fn validate_command_outputs(command: &Command) -> Result<(), AssetError> {
    use Command::*;
    let mut inputs: Vec<&Path> = Vec::new();
    let mut outputs: Vec<&Path> = Vec::new();
    let sidecar;
    match command {
        Atmosphere {
            source_manifest,
            out,
            report,
            ..
        }
        | EntityAssets {
            source_manifest,
            out,
            report,
            ..
        }
        | EquipmentAssets {
            source_manifest,
            out,
            report,
            ..
        }
        | FontAssets {
            source_manifest,
            out,
            report,
            ..
        }
        | HudAssets {
            source_manifest,
            out,
            report,
            ..
        }
        | BlockEntityAssets {
            source_manifest,
            out,
            report,
            ..
        }
        | UiAssets {
            source_manifest,
            out,
            report,
            ..
        }
        | ParticleAssets {
            source_manifest,
            out,
            report,
            ..
        }
        | IconAssets {
            source_manifest,
            out,
            report,
            ..
        }
        | ActorAssets {
            source_manifest,
            out,
            report,
            ..
        }
        | LangAssets {
            source_manifest,
            out,
            report,
            ..
        }
        | AudioAssets {
            source_manifest,
            out,
            report,
            ..
        }
        | AudioPcmAssets {
            source_manifest,
            out,
            report,
            ..
        }
        | OutlineFontAssets {
            source_manifest,
            out,
            report,
            ..
        } => {
            inputs.push(source_manifest);
            outputs.extend([out.as_path(), report.as_path()]);
        }
        Compile {
            source_manifest,
            registry,
            light_registry,
            biome_registry,
            out,
            ..
        } => {
            inputs.extend([
                source_manifest.as_path(),
                registry,
                light_registry,
                biome_registry,
            ]);
            outputs.push(out);
        }
        AnimationInventory {
            source_manifest,
            out,
            ..
        } => {
            inputs.push(source_manifest);
            outputs.push(out);
        }
        WeatherAssets { out, .. } | HudExtrasAssets { out, .. } => outputs.push(out),
        AudioBank { out, report, .. } => outputs.extend([out.as_path(), report.as_path()]),
        LanguageAssets { .. } | VanillaPack { .. } | Prepare { .. } => return Ok(()),
    }
    match command {
        EntityAssets { out, .. } => {
            sidecar = entity_refs_output(out);
            outputs.push(&sidecar);
        }
        Compile { out, .. } => {
            sidecar = material_keys_output(out);
            outputs.push(&sidecar);
        }
        Atmosphere {
            clouds_override, ..
        } => inputs.extend(clouds_override.as_deref()),
        IconAssets { block_assets, .. } => inputs.extend(block_assets.as_deref()),
        AudioPcmAssets { catalog, .. } => inputs.push(catalog),
        FontAssets { font, .. } => inputs.extend(font.as_deref()),
        OutlineFontAssets {
            font,
            fallback_font,
            ..
        } => {
            inputs.push(font);
            inputs.extend(fallback_font.as_deref());
        }
        _ => {}
    }
    for (index, output) in outputs.iter().enumerate() {
        for other in outputs.iter().skip(index + 1).chain(inputs.iter()) {
            validate_output_bundle(output, other)?;
        }
    }
    Ok(())
}

/// Returns the entity catalog's companion reference table destination.
pub(super) fn entity_refs_output(out: &Path) -> PathBuf {
    out.with_extension("vanillarefs.json")
}

/// Returns the world catalog's companion material key destination.
pub(super) fn material_keys_output(out: &Path) -> PathBuf {
    out.with_extension("matkeys.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_outputs_reject_derived_sidecars_and_source_inputs() {
        let out = PathBuf::from("/tmp/result.bin");
        let refs = entity_refs_output(&out);
        let command = Command::EntityAssets {
            pack: "/tmp/pack".into(),
            source_manifest: "/tmp/source.json".into(),
            out,
            report: refs,
        };
        let error = crate::run(command).unwrap_err().to_string();
        assert!(error.contains("distinct files"), "{error}");
        let out = PathBuf::from("/tmp/world.bin");
        let command = Command::Compile {
            pack: "/tmp/pack".into(),
            source_manifest: "/tmp/source.json".into(),
            registry: material_keys_output(&out),
            light_registry: "/tmp/light.bin".into(),
            biome_registry: "/tmp/biomes.bin".into(),
            out,
        };
        let error = crate::run(command).unwrap_err().to_string();
        assert!(error.contains("distinct files"), "{error}");
        let command = Command::IconAssets {
            pack: "/tmp/pack".into(),
            source_manifest: "/tmp/source.json".into(),
            block_assets: Some("/tmp/world.bin".into()),
            out: "/tmp/world.bin".into(),
            report: "/tmp/report.json".into(),
        };
        let error = crate::run(command).unwrap_err().to_string();
        assert!(error.contains("distinct files"), "{error}");
        let command = Command::ActorAssets {
            pack: "/tmp/pack".into(),
            source_manifest: "/tmp/source.json".into(),
            out: "/tmp/source.json".into(),
            report: "/tmp/report.json".into(),
        };
        let error = crate::run(command).unwrap_err().to_string();
        assert!(error.contains("distinct files"), "{error}");
    }
}
