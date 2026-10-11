use std::{
    io::{Cursor, Write},
    sync::Arc,
};

use assets::{SoundBankIndex, SoundEventTables};
use resource_pack::{LayeredPackView, validate_handoff};
use serde_json::json;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use super::ServerSoundPack;
use crate::audio::bank::SoundBank;

fn view(files: &[(&str, &[u8])]) -> LayeredPackView {
    let id = "42ee1e2f-5604-47cb-ab4c-7a978f9a83be";
    let manifest = json!({"format_version":2,"header":{"uuid":id,"version":[1,0,0]},"modules":[{"type":"resources"}]}).to_string();
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in
        std::iter::once(("manifest.json", manifest.as_bytes())).chain(files.iter().copied())
    {
        zip.start_file(
            path,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
    }
    let archive = protocol::ResourcePackArchive::unencrypted(
        id.parse().unwrap(),
        "1.0.0".into(),
        String::new(),
        zip.finish().unwrap().into_inner(),
    );
    LayeredPackView::new(validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![archive]),
    ))
}

fn bank(pack: Arc<ServerSoundPack>) -> SoundBank {
    let bytes = assets::encode_sound_bank(b"{}", b"{}", b"{}", &[]).unwrap();
    let mut bank = SoundBank::from_parts(
        SoundBankIndex::decode_prefix(&bytes).unwrap(),
        SoundEventTables::default(),
        None,
    );
    bank.install_server(Some(pack));
    bank
}

#[test]
fn every_sound_remains_playable_when_total_pcm_exceeds_the_cache_budget() {
    let wav = super::tests::wav(1, 8000, &vec![1234; 3 * 1024 * 1024]);
    let paths: Vec<_> = (0..18).map(|i| format!("sounds/tone{i}.wav")).collect();
    let definitions: serde_json::Map<_, _> = (0..18)
        .map(|i| {
            (
                format!("tone{i}"),
                json!({"category":"ui", "sounds":[format!("sounds/tone{i}")]}),
            )
        })
        .collect();
    let json = serde_json::to_vec(&json!({"sound_definitions":definitions})).unwrap();
    let mut files = vec![("sounds/sound_definitions.json", json.as_slice())];
    files.extend(paths.iter().map(|path| (path.as_str(), wav.as_slice())));
    let pack = Arc::new(ServerSoundPack::from_view(&view(&files)).unwrap());
    let mut bank = bank(pack);
    for i in 0..18 {
        let path = format!("sounds/tone{i}");
        let pcm = bank
            .pcm(&path, false)
            .unwrap_or_else(|| panic!("{path} must remain playable"));
        assert_eq!(pcm.samples.first(), Some(&1234));
    }
    assert_eq!(
        bank.pcm("sounds/tone0", false).unwrap().samples.first(),
        Some(&1234)
    );
}

#[test]
fn waveform_only_override_is_playable_without_redeclaring_a_definition() {
    let wav = super::tests::wav(1, 8000, &[1234, -1234]);
    let pack = ServerSoundPack::from_view(&view(&[("sounds/random/click.wav", &wav)]))
        .expect("a waveform override is a sound pack");
    let pcm = bank(Arc::new(pack))
        .pcm("sounds/random/click", false)
        .expect("override PCM");
    assert_eq!(&*pcm.samples, &[1234, -1234]);
}

#[test]
fn waveform_paths_follow_the_archive_case_insensitive_lookup() {
    let wav = super::tests::wav(1, 8000, &[1234, -1234]);
    let pack = ServerSoundPack::from_view(&view(&[("SOUNDS/RANDOM/CLICK.WAV", &wav)]))
        .expect("mixed-case waveform override");
    let mut bank = bank(Arc::new(pack));
    for path in ["sounds/random/click", "SOUNDS/RANDOM/CLICK"] {
        let pcm = bank.pcm(path, false).expect("override PCM");
        assert_eq!(&*pcm.samples, &[1234, -1234]);
    }
}

#[test]
fn replacing_a_pack_drops_old_failures_cached_pcm_and_pending_decodes() {
    let old = super::tests::wav(1, 8000, &[1234; 4000]);
    let new = super::tests::wav(1, 8000, &[-2345; 4000]);
    let old = Arc::new(
        ServerSoundPack::from_view(&view(&[
            ("sounds/shared.wav", &old),
            ("sounds/pending.wav", &old),
        ]))
        .unwrap(),
    );
    let new = Arc::new(
        ServerSoundPack::from_view(&view(&[
            ("sounds/shared.wav", &new),
            ("sounds/pending.wav", &new),
            ("sounds/missing.wav", &new),
        ]))
        .unwrap(),
    );
    let mut bank = bank(old);
    assert_eq!(
        bank.pcm("sounds/shared", false).unwrap().samples.first(),
        Some(&1234)
    );
    assert!(matches!(
        bank.lookup("sounds/pending", false),
        crate::audio::bank::PcmLookup::Pending
    ));
    assert!(bank.pcm("sounds/missing", false).is_none());
    bank.install_server(Some(new));
    for path in ["sounds/shared", "sounds/pending", "sounds/missing"] {
        assert_eq!(bank.pcm(path, false).unwrap().samples.first(), Some(&-2345));
    }
    bank.poll();
    assert_eq!(
        bank.pcm("sounds/pending", false).unwrap().samples.first(),
        Some(&-2345)
    );
}

#[test]
fn admitted_custom_sound_reaches_the_output_mixer() {
    let Some(path) = std::env::var_os("CINNABAR_TEST_SOUND_PACK") else {
        eprintln!(
            "skipping admitted_custom_sound_reaches_the_output_mixer: missing CINNABAR_TEST_SOUND_PACK fixture"
        );
        return;
    };
    let Some(event) = std::env::var_os("CINNABAR_TEST_SOUND_EVENT") else {
        eprintln!(
            "skipping admitted_custom_sound_reaches_the_output_mixer: missing CINNABAR_TEST_SOUND_EVENT fixture"
        );
        return;
    };
    let bytes = std::fs::read(&path).expect("admitted sound archive");
    let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_reader(zip.by_name("manifest.json").unwrap()).unwrap();
    let id = manifest["header"]["uuid"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let version = manifest["header"]["version"]
        .as_array()
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(".");
    drop(zip);
    let archive = protocol::ResourcePackArchive::unencrypted(id, version, String::new(), bytes);
    let view = LayeredPackView::new(validate_handoff(
        protocol::ResourcePackHandoff::from_archives(vec![archive]),
    ));
    let pack = Arc::new(ServerSoundPack::from_view(&view).expect("admitted sounds"));
    let event = event.to_str().unwrap();
    let alternatives = pack.definitions[event].alternatives.clone();
    let mut bank = bank(pack);
    for alternative in &alternatives {
        let pcm = bank
            .pcm(&alternative.name, false)
            .expect("custom sound decoded");
        assert!(pcm.samples.iter().any(|sample| *sample != 0));
    }
    let mut engine = crate::audio::AudioEngine::new(Some(bank));
    engine.enqueue(crate::audio::engine::SoundRequest::new(event));
    let sources = engine.pump(None, 0.0, &crate::audio::AudioSettings::default());
    assert!(!sources.is_empty());
    let (mut device, mixer) = crate::named_audio::AudioDevice::memory_mixer();
    for source in sources {
        assert!(device.play_source(source));
    }
    let samples: Vec<i16> = mixer
        .take(crate::audio::voice::OUTPUT_RATE as usize * 2)
        .map(|sample| (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
        .collect();
    assert!(samples.iter().any(|sample| *sample != 0));
    if let Some(output) = std::env::var_os("CINNABAR_TEST_SOUND_CAPTURE") {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options.open(output).unwrap();
        output
            .write_all(&super::tests::wav(
                2,
                crate::audio::voice::OUTPUT_RATE,
                &samples,
            ))
            .unwrap();
    }
}
