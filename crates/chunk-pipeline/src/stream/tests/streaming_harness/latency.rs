use super::*;

pub(super) const NETWORK_ROUND_TRIPS: [Duration; 2] =
    [Duration::from_millis(50), Duration::from_millis(120)];

/// A reply uses either the original frame delay or an explicit elapsed network delay.
pub(super) enum ReplyDeadline {
    Frame(u64),
    Time(Instant),
}

impl ReplyDeadline {
    /// Starts the complete request-to-reply delay when the request is dispatched.
    pub(super) fn after_request(frame: u64, now: Instant, rtt: Option<Duration>) -> Self {
        match rtt {
            Some(rtt) => Self::Time(now + rtt),
            None => Self::Frame(frame + REPLY_LATENCY_FRAMES),
        }
    }

    /// Reports readiness using the caller's clock, without waiting or reading wall time.
    pub(super) fn is_ready(&self, frame: u64, now: Instant) -> bool {
        match self {
            Self::Frame(due) => frame >= *due,
            Self::Time(due) => now >= *due,
        }
    }
}

#[test]
fn replies_wait_for_the_full_round_trip_from_each_dispatch() {
    for rtt in NETWORK_ROUND_TRIPS {
        let mut harness = Harness::new(0, 1);
        harness.reply_rtt = Some(rtt);
        let first = SubChunkKey::new(0, 0, 4, 0);
        let second = SubChunkKey::new(0, 1, 4, 0);
        let sent = Instant::now();
        let gap = rtt / 2;
        harness
            .stream
            .enqueue_request(first.chunk(), first.y, 1, None);
        harness.answer_requests(sent);
        harness
            .stream
            .enqueue_request(second.chunk(), second.y, 1, None);
        harness.answer_requests(sent + gap);

        harness.frame = REPLY_LATENCY_FRAMES + 1;
        harness.deliver(sent + rtt - Duration::from_nanos(1));
        assert_eq!(harness.sequence, 1, "frames cannot shorten the RTT");
        harness.deliver(sent + rtt);
        assert_eq!(harness.sequence, 2);
        assert!(
            harness
                .stream
                .requests
                .admitted_replies
                .contains_key(&first)
        );
        assert!(
            !harness
                .stream
                .requests
                .admitted_replies
                .contains_key(&second)
        );
        harness.deliver(sent + gap + rtt);
        assert_eq!(harness.sequence, 3);
        assert!(
            harness
                .stream
                .requests
                .admitted_replies
                .contains_key(&second)
        );
    }
}

#[test]
fn released_replies_keep_their_deadline_and_pass_later_pending_replies() {
    let mut harness = Harness::new(0, 1);
    let rtt = NETWORK_ROUND_TRIPS[0];
    harness.reply_rtt = Some(rtt);
    let held = SubChunkKey::new(0, 0, 4, 0);
    let later = SubChunkKey::new(0, 1, 4, 0);
    let sent = Instant::now();
    harness.withheld.insert(held.chunk());
    harness
        .stream
        .enqueue_request(held.chunk(), held.y, 1, None);
    harness.answer_requests(sent);
    harness
        .stream
        .enqueue_request(later.chunk(), later.y, 1, None);
    harness.answer_requests(sent + rtt / 2);
    harness.withheld.clear();

    harness.deliver(sent + rtt / 2);
    assert_eq!(harness.sequence, 1, "release cannot bypass the RTT");
    harness.deliver(sent + rtt);
    assert_eq!(
        harness.sequence, 2,
        "a later deadline cannot block a ready reply"
    );
    assert!(harness.stream.requests.admitted_replies.contains_key(&held));
    assert!(
        !harness
            .stream
            .requests
            .admitted_replies
            .contains_key(&later)
    );
}

#[test]
fn default_replies_keep_the_original_frame_delay() {
    let mut harness = Harness::new(0, 1);
    let key = SubChunkKey::new(0, 0, 4, 0);
    let sent = Instant::now();
    harness.stream.enqueue_request(key.chunk(), key.y, 1, None);
    harness.answer_requests(sent);
    harness.frame = REPLY_LATENCY_FRAMES - 1;
    harness.deliver(sent + COMPLETION_TIMEOUT);
    assert_eq!(harness.sequence, 1);
    harness.frame = REPLY_LATENCY_FRAMES;
    harness.deliver(sent + COMPLETION_TIMEOUT);
    assert_eq!(harness.sequence, 2);
    assert!(harness.stream.requests.admitted_replies.contains_key(&key));
}

#[test]
fn default_withheld_replies_are_ready_on_release() {
    let mut harness = Harness::new(0, 1);
    let key = SubChunkKey::new(0, 0, 4, 0);
    let sent = Instant::now();
    harness.withheld.insert(key.chunk());
    harness.stream.enqueue_request(key.chunk(), key.y, 1, None);
    harness.answer_requests(sent);
    harness.withheld.clear();
    harness.deliver(sent);
    assert_eq!(harness.sequence, 2);
    assert!(harness.stream.requests.admitted_replies.contains_key(&key));
}
