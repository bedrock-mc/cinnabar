//! Load feedback: translation reports how many programs are done out of how many.

use std::sync::mpsc;

use shaderpack::{PackSource, ShaderPack, TranslateConfig};

const VERTEX: &str = "#version 120\nvoid main() { gl_Position = ftransform(); }\n";
const FRAGMENT: &str = "#version 120\nvoid main() { gl_FragData[0] = vec4(1.0); }\n";

#[test]
fn translation_reports_the_program_count_then_each_finished_program() {
    let pack = ShaderPack::new(PackSource::from_files([
        ("composite.vsh", VERTEX),
        ("composite.fsh", FRAGMENT),
        ("composite1.vsh", VERTEX),
        ("composite1.fsh", FRAGMENT),
        ("final.vsh", VERTEX),
        ("final.fsh", FRAGMENT),
    ]));
    let (send, receive) = mpsc::channel();
    let translation = pack.translate_reporting(&TranslateConfig::default(), |done, total| {
        send.send((done, total)).unwrap();
    });
    let total = translation.programs.len();
    assert_eq!(total, 3);
    let mut reports: Vec<(usize, usize)> = receive.try_iter().collect();
    assert_eq!(reports.remove(0), (0, total), "the total is known first");
    reports.sort_unstable();
    assert_eq!(reports, [(1, total), (2, total), (3, total)]);
}

#[test]
fn cancelling_before_programs_start_returns_no_partial_translation() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let pack = ShaderPack::new(PackSource::from_files([
        ("final.vsh", VERTEX),
        ("final.fsh", FRAGMENT),
    ]));
    let cancelled = AtomicBool::new(false);
    let result = pack.translate_cancellable(
        &TranslateConfig::default(),
        |done, total| {
            assert_eq!((done, total), (0, 1));
            cancelled.store(true, Ordering::Relaxed);
        },
        || cancelled.load(Ordering::Relaxed),
    );
    assert!(result.is_none());
    assert!(
        pack.translate_cancellable(
            &TranslateConfig::default(),
            |_, _| {
                panic!("an already cancelled load must not read or translate the pack");
            },
            || true
        )
        .is_none()
    );
}
