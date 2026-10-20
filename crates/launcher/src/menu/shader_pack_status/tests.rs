use super::*;

const SLOW: Duration = LOADING_TOAST_DELAY;
const REASON: &str = "composite: composite.fsh:2: parse: Unknown variable: undefined_name\n\
                      final: final.fsh:9: parse: Unknown variable: grade";

fn at(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

fn loading(pack: &str, programs: Option<(u32, u32)>) -> PackLoad<'_> {
    PackLoad::Loading { pack, programs }
}

fn titles(toast: Option<PackToast>) -> Option<(String, String)> {
    match toast? {
        PackToast::Loading { title, message } | PackToast::Outcome { title, message } => {
            Some((title, message))
        }
        PackToast::Withdraw => Some(("withdraw".to_owned(), String::new())),
    }
}

#[test]
fn starting_on_vanilla_raises_nothing() {
    let mut notices = PackNotices::default();
    for millis in [0, 1_000, 60_000] {
        assert_eq!(notices.observe(PackLoad::Idle, at(millis), true), None);
    }
}

#[test]
fn a_slow_load_stands_a_progress_toast_that_its_outcome_replaces() {
    let mut notices = PackNotices::default();
    assert_eq!(
        notices.observe(loading("Dusk.zip", None), at(0), true),
        None
    );
    assert_eq!(
        notices.observe(loading("Dusk.zip", Some((0, 40))), SLOW - at(1), true),
        None,
        "a load that may still settle at once stays quiet"
    );
    assert_eq!(
        titles(notices.observe(loading("Dusk.zip", Some((12, 40))), SLOW, true)),
        Some((
            "Loading shader pack Dusk…".to_owned(),
            "Translated 12 of 40 programs".to_owned()
        ))
    );
    assert_eq!(
        notices.observe(loading("Dusk.zip", Some((12, 40))), SLOW + at(16), true),
        None,
        "unchanged progress leaves the toast alone"
    );
    assert_eq!(
        notices.observe(loading("Dusk.zip", Some((40, 40))), SLOW + at(32), true),
        Some(PackToast::Loading {
            title: "Loading shader pack Dusk…".to_owned(),
            message: "Building pipelines".to_owned(),
        })
    );
    assert_eq!(
        notices.observe(PackLoad::Ready { pack: "Dusk.zip" }, SLOW + at(900), true),
        Some(PackToast::Outcome {
            title: "Shader pack Dusk loaded".to_owned(),
            message: String::new(),
        })
    );
    for millis in [1_000, 5_000] {
        let quiet = notices.observe(
            PackLoad::Ready { pack: "Dusk.zip" },
            SLOW + at(millis),
            true,
        );
        assert_eq!(quiet, None, "one toast per transition");
    }
}

#[test]
fn a_quick_load_reports_only_its_outcome() {
    let mut notices = PackNotices::default();
    assert_eq!(
        notices.observe(loading("Lumen", Some((3, 9))), at(0), true),
        None
    );
    assert_eq!(
        titles(notices.observe(PackLoad::Ready { pack: "Lumen" }, at(120), true)),
        Some(("Shader pack Lumen loaded".to_owned(), String::new()))
    );
}

#[test]
fn a_failure_names_the_pack_and_summarises_its_first_problem_once() {
    let mut notices = PackNotices::default();
    notices.observe(loading("Dusk.zip", None), at(0), true);
    let failed = PackLoad::Failed {
        pack: "Dusk.zip",
        reason: REASON,
    };
    assert_eq!(
        titles(notices.observe(failed, at(2_000), true)),
        Some((
            "Shader pack Dusk failed to load".to_owned(),
            "composite: composite.fsh:2: parse: Unknown variable: undefined_name (+1 more)"
                .to_owned()
        ))
    );
    assert_eq!(notices.observe(failed, at(2_016), true), None);
    // Loading it again reports again.
    notices.observe(loading("Dusk.zip", None), at(3_000), true);
    assert!(matches!(
        notices.observe(failed, at(3_100), true),
        Some(PackToast::Outcome { .. })
    ));
}

#[test]
fn turning_the_pack_off_mid_load_withdraws_a_standing_toast_only() {
    let mut notices = PackNotices::default();
    notices.observe(loading("Dusk", None), at(0), true);
    assert_eq!(
        notices.observe(PackLoad::Idle, at(100), true),
        None,
        "never stood"
    );
    notices.observe(loading("Dusk", None), at(1_000), true);
    assert!(
        notices
            .observe(loading("Dusk", None), at(1_000) + SLOW, true)
            .is_some()
    );
    assert_eq!(
        notices.observe(PackLoad::Idle, at(2_000), true),
        Some(PackToast::Withdraw)
    );
    assert_eq!(notices.observe(PackLoad::Idle, at(3_000), true), None);
}

#[test]
fn picking_another_pack_mid_load_retitles_the_standing_toast_at_once() {
    let mut notices = PackNotices::default();
    notices.observe(loading("Dusk", None), at(0), true);
    assert!(notices.observe(loading("Dusk", None), SLOW, true).is_some());
    assert_eq!(
        titles(notices.observe(loading("Lumen", None), SLOW + at(10), true)),
        Some(("Loading shader pack Lumen…".to_owned(), String::new()))
    );
}

#[test]
fn long_failures_shorten_for_the_toast_and_count_lines_for_the_row() {
    let long = "x".repeat(SUMMARY_CHARS + 10);
    let summary = failure_summary(&long);
    assert_eq!(summary.chars().count(), SUMMARY_CHARS + 1);
    assert!(summary.ends_with('…'));
    let reason: String = (0..DESCRIBED_LINES + 3)
        .map(|index| format!("program{index}: broken\n"))
        .collect();
    let description = failure_description(&reason);
    assert!(description.contains("program0: broken"));
    assert!(description.contains(&format!("program{}: broken", DESCRIBED_LINES - 1)));
    assert!(!description.contains(&format!("program{}: broken", DESCRIBED_LINES)));
    assert!(description.ends_with("…and 3 more in the client log."));
}

#[test]
fn pack_labels_drop_only_a_zip_extension() {
    assert_eq!(pack_label("Dusk.zip"), "Dusk");
    assert_eq!(pack_label("Dusk.ZIP"), "Dusk");
    assert_eq!(pack_label("Lumen"), "Lumen");
    assert_eq!(pack_label(".zip"), ".zip");
    assert_eq!(pack_label("Lumen v2.zipped"), "Lumen v2.zipped");
}
