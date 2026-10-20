use super::*;

#[test]
fn a_silent_start_stalls_after_the_timeout() {
    let directory = tempfile::tempdir().unwrap();
    let start = Instant::now();
    let mut monitor = Monitor::new(start);
    let before = start + STALL_TIMEOUT - Duration::from_secs(1);
    assert_eq!(monitor.poll(directory.path(), before), Progress::Waiting);
    assert_eq!(
        monitor.poll(directory.path(), start + STALL_TIMEOUT),
        Progress::Stalled
    );
}

#[test]
fn only_changed_setup_progress_restarts_the_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let progress = directory.path().join("status.json");
    fs::write(&progress, "downloading 0").unwrap();
    let start = Instant::now();
    let mut monitor = Monitor::new(start);
    let mut relay = Relay::start(marker(directory.path()), progress.clone(), awaiting);
    let mut now = start + STALL_TIMEOUT - Duration::from_secs(1);
    assert_eq!(monitor.poll(directory.path(), now), Progress::Waiting);
    // A setup that reports nothing new is hung, however often the client polls.
    for _ in 0..3 {
        relay.poll();
    }
    now += STALL_TIMEOUT;
    assert_eq!(monitor.poll(directory.path(), now), Progress::Stalled);
    fs::write(&progress, "downloading 1").unwrap();
    relay.poll();
    assert_eq!(monitor.poll(directory.path(), now), Progress::Waiting);
}

/// Fixture reports name their state directly.
fn awaiting(report: &[u8]) -> bool {
    report == b"consent"
}

#[test]
fn awaiting_the_user_never_stalls_and_the_deadline_counts_from_the_answer() {
    let directory = tempfile::tempdir().unwrap();
    let progress = directory.path().join("status.json");
    let start = Instant::now();
    let mut monitor = Monitor::new(start);
    let mut relay = Relay::start(marker(directory.path()), progress.clone(), awaiting);
    fs::write(&progress, "consent").unwrap();
    relay.poll();
    // The prompt stays open far past the timeout, with repeated identical reports.
    let mut now = start;
    for _ in 0..10 {
        now += STALL_TIMEOUT;
        relay.poll();
        assert_eq!(monitor.poll(directory.path(), now), Progress::Waiting);
    }
    fs::write(&progress, "downloading").unwrap();
    relay.poll();
    let answered = now;
    assert_eq!(monitor.poll(directory.path(), answered), Progress::Waiting);
    let before = answered + STALL_TIMEOUT - Duration::from_secs(1);
    assert_eq!(monitor.poll(directory.path(), before), Progress::Waiting);
    assert_eq!(
        monitor.poll(directory.path(), answered + STALL_TIMEOUT),
        Progress::Stalled
    );
}

#[test]
fn finishing_preparation_leaves_the_awaiting_state_with_a_fresh_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let progress = directory.path().join("status.json");
    let start = Instant::now();
    let mut monitor = Monitor::new(start);
    let mut relay = Relay::start(marker(directory.path()), progress.clone(), awaiting);
    fs::write(&progress, "consent").unwrap();
    relay.poll();
    let ended = start + STALL_TIMEOUT * 3;
    assert_eq!(monitor.poll(directory.path(), ended), Progress::Waiting);
    relay.finish();
    assert_eq!(monitor.poll(directory.path(), ended), Progress::Waiting);
    assert_eq!(
        monitor.poll(directory.path(), ended + STALL_TIMEOUT),
        Progress::Stalled
    );
}

#[test]
fn preparation_heartbeats_restart_the_deadline_until_they_stop() {
    let directory = tempfile::tempdir().unwrap();
    let marker = marker(directory.path());
    let start = Instant::now();
    let mut monitor = Monitor::new(start);
    // Asset preparation lasting several stall timeouts keeps the client alive.
    let mut now = start;
    for count in 1..=4 {
        now += STALL_TIMEOUT - Duration::from_secs(1);
        beat(&marker, &count.to_string()).unwrap();
        assert_eq!(monitor.poll(directory.path(), now), Progress::Waiting);
    }
    // A repeated count is not progress, so a hung client still stalls.
    assert_eq!(
        monitor.poll(directory.path(), now + STALL_TIMEOUT),
        Progress::Stalled
    );
    fs::write(&marker, "").unwrap();
    assert_eq!(
        monitor.poll(directory.path(), now + STALL_TIMEOUT * 2),
        Progress::Started
    );
}

#[cfg(unix)]
#[test]
fn stopping_a_client_also_ends_the_setup_process_it_started() {
    use rustix::process::{Pid, test_kill_process_group};
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 600 & wait"]);
    let tree = Tree::spawn(&mut command).unwrap();
    let group = Pid::from_child(&tree.child);
    tree.stop().unwrap();
    assert!(test_kill_process_group(group).is_err());
}

#[cfg(windows)]
#[test]
fn stopping_a_client_also_ends_the_setup_process_it_started() {
    // `cmd` waits on a long-running grandchild, as the client waits on its setup process.
    let mut command = Command::new("cmd");
    command.args(["/C", "ping -n 600 127.0.0.1 >NUL"]);
    let tree = Tree::spawn(&mut command).unwrap();
    assert!(tree.job.is_some());
    // Returns only once the job, grandchild included, has no live process.
    tree.stop().unwrap();
}
