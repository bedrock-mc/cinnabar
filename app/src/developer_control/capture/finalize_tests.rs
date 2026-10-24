use std::{cell::Cell, path::PathBuf};

use super::{Summary, complete_recording};

fn summary() -> Summary {
    Summary {
        path: PathBuf::from("capture.mp4"),
        frames: 3,
        size: Some([2, 2]),
        filled: 0,
    }
}

#[test]
fn failed_audio_still_finishes_video_and_reports_its_path_context() {
    let joined = Cell::new(false);
    let error = complete_recording(
        None,
        Err("finish audio capture.wav: disk failure".into()),
        |wav| {
            joined.set(true);
            assert!(wav.is_none());
            Ok(summary())
        },
    )
    .unwrap_err();
    assert!(joined.get());
    assert!(error.contains("capture.wav"));
    assert!(error.contains("disk failure"));
}

#[test]
fn capture_audio_and_encoder_errors_are_all_retained() {
    let error = complete_recording(
        Some("capture failed".into()),
        Err("audio failed".into()),
        |_| Err("encoder failed".into()),
    )
    .unwrap_err();
    for context in ["capture failed", "audio failed", "encoder failed"] {
        assert!(error.contains(context));
    }
}

#[test]
fn successful_audio_is_muxed_even_when_capture_failed() {
    let path = PathBuf::from("capture.wav");
    let error = complete_recording(
        Some("readback failed".into()),
        Ok(Some(path.clone())),
        |wav| {
            assert_eq!(wav, Some(path.as_path()));
            Ok(summary())
        },
    )
    .unwrap_err();
    assert_eq!(error, "readback failed");
}

#[test]
fn recording_without_audio_finishes_successfully() {
    let expected = summary();
    let (result, wav) = complete_recording(None, Ok(None), |wav| {
        assert!(wav.is_none());
        Ok(expected.clone())
    })
    .unwrap();
    assert_eq!(result, expected);
    assert!(wav.is_none());
}
