//! Optional finite PCM sibling, linked to the ACTUAL startup catalog identity.
use super::{AssetStartupError, LoadedAudioAssets};
use assets::RuntimeAudioPcm;
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
    sync::Arc,
};

pub(crate) fn load_audio_pcm_assets(
    world_path: &Path,
    catalog: Option<&LoadedAudioAssets>,
) -> Result<Option<Arc<RuntimeAudioPcm>>, AssetStartupError> {
    let path = world_path.with_file_name(assets::carriers::AUDIO_PCM.output);
    let fail = |detail: String| AssetStartupError::AudioPcm {
        path: path.clone(),
        detail,
    };
    let mut file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            diagnostics::log_stderr!(
                "optional finite PCM carrier absent at {}; named playback inactive; make audio-pcm-assets",
                path.display()
            );
            return Ok(None);
        }
        Err(error) => return Err(fail(error.to_string())),
    };
    let Some(catalog) = catalog else {
        return Err(fail(
            "present PCM requires its validated sound catalog".into(),
        ));
    };
    let metadata = file.metadata().map_err(|error| fail(error.to_string()))?;
    let limit = assets::MAX_AUDIO_PCM_CARRIER_BYTES;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(fail("carrier byte bound or file type".into()));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(metadata.len() as usize)
        .map_err(|_| fail("bounded allocation failed".into()))?;
    file.by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| fail(error.to_string()))?;
    if bytes.len() > limit || bytes.len() as u64 != metadata.len() {
        return Err(fail("carrier changed or exceeds bound".into()));
    }
    let expected = assets::reviewed_audio_pcm_identity();
    let runtime = RuntimeAudioPcm::decode(&bytes, catalog.runtime(), catalog.identity(), &expected)
        .map_err(|error| fail(error.to_string()))?;
    diagnostics::log_stderr!(
        "loaded finite no-loop PCM: {} frames at {}Hz; preview sample identity, playback capability excludes streaming/spatial/loop parity",
        runtime.frames(),
        runtime.sample_rate()
    );
    Ok(Some(Arc::new(runtime)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn absent_is_optional_but_present_without_actual_catalog_is_fatal() {
        let root = std::env::temp_dir().join(format!("audio-pcm-startup-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let world = root.join("world.mcbea");
        assert!(load_audio_pcm_assets(&world, None).unwrap().is_none());
        let pcm = root.join("vanilla-v1.mcbepcm");
        fs::write(&pcm, b"invalid synthetic PCM").unwrap();
        assert!(matches!(
            load_audio_pcm_assets(&world, None),
            Err(AssetStartupError::AudioPcm { .. })
        ));
        fs::remove_file(pcm).unwrap();
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn loaded_catalog_identity_is_same_bytes_and_pcm_profile_cannot_self_authorize() {
        use sha2::{Digest, Sha256};
        let root = std::env::temp_dir().join(format!("audio-pcm-catalog-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let world = root.join("world.mcbea");
        let expected = assets::reviewed_audio_pcm_identity();
        let definition = assets::AudioDefinition {
            identifier: expected.identifier().into(),
            category: None,
            subtitle: None,
            min_distance: None,
            max_distance: None,
            volume: None,
            pitch: None,
            use_legacy_max_distance: None,
            alternatives: vec![assets::AudioAlternative {
                object_form: true,
                name: expected.source_path().strip_suffix(".fsb").unwrap().into(),
                weight: 1,
                volume: None,
                pitch: None,
                is_3d: Some(false),
                stream: Some(true),
                load_on_low_memory: None,
            }]
            .into_boxed_slice(),
        };
        let bytes = assets::encode_audio_catalog(
            expected.source_manifest_sha256(),
            expected.sound_definitions_sha256(),
            &[definition],
        )
        .unwrap();
        let catalog_path = root.join("vanilla-v1.mcbeaud");
        fs::write(&catalog_path, &bytes).unwrap();
        let catalog = super::super::audio_carrier::load_audio_assets(&world)
            .unwrap()
            .unwrap();
        assert_eq!(catalog.identity(), <[u8; 32]>::from(Sha256::digest(&bytes)));
        assert_ne!(catalog.identity(), expected.catalog_sha256());
        let pcm = root.join("vanilla-v1.mcbepcm");
        fs::write(&pcm, b"MCBEPCM1 synthetic invalid").unwrap();
        assert!(load_audio_pcm_assets(&world, Some(&catalog)).is_err());
        fs::write(&pcm, vec![0; assets::MAX_AUDIO_PCM_CARRIER_BYTES + 1]).unwrap();
        assert!(load_audio_pcm_assets(&world, Some(&catalog)).is_err());
        fs::remove_file(pcm).unwrap();
        fs::remove_file(catalog_path).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
