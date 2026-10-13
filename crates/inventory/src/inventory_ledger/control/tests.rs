use std::collections::{BTreeMap, BTreeSet};

use super::*;

mod interleavings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Completion {
    Answered,
    TimedOut,
}

/// Produces reproducible interleavings without a clock or an additional test dependency.
fn random_word(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Tracks sent requests by their position in an independent wire transcript.
#[derive(Default)]
struct Transcript {
    requests: Vec<(ControlRequest, u64)>,
    replies: [VecDeque<usize>; 2],
    completed: BTreeMap<usize, Completion>,
    superseded: BTreeSet<usize>,
}

impl Transcript {
    /// Assigns a separate wire receipt to each send, including repeated IDs and ownerless cleanup.
    fn send(&mut self, queue: &mut SentControls, request: ControlRequest, now: u64, stream: usize) {
        let receipt = self.requests.len();
        self.requests.push((request, now));
        self.replies[stream].push_back(receipt);
        queue.sent(request, now);
    }

    /// Requires each reply to consume the oldest receipt of its kind and complete it at most once.
    fn reply(&mut self, queue: &mut SentControls, stream: usize) {
        let kind = if stream == 0 {
            ControlKind::PersonalOpen
        } else {
            ControlKind::Close
        };
        let actual = queue.answer(kind);
        let Some(receipt) = self.replies[stream].pop_front() else {
            assert!(actual.is_none());
            return;
        };
        let actual = actual.expect("every sent receipt keeps its correlation slot");
        let expected = self.requests[receipt].0;
        assert_eq!(
            actual.request, expected,
            "a reply must never acknowledge a newer request"
        );
        assert_eq!(actual.superseded, self.superseded.contains(&receipt));
        if let Some(completion) = self.completed.get(&receipt) {
            assert_eq!(*completion, Completion::TimedOut);
            assert!(
                actual.deadline_millis.is_none(),
                "a late reply cannot complete the request twice"
            );
        } else {
            assert!(actual.deadline_millis.is_some());
            self.completed.insert(receipt, Completion::Answered);
        }
    }

    /// Completes overdue receipts once while leaving their wire reply positions intact.
    fn timeout(&mut self, queue: &mut SentControls, now: u64) {
        let mut expected = 0;
        for (receipt, (_, sent_at)) in self.requests.iter().enumerate() {
            if now.saturating_sub(*sent_at) >= INVENTORY_REQUEST_TIMEOUT_MILLIS
                && !self.completed.contains_key(&receipt)
            {
                self.completed.insert(receipt, Completion::TimedOut);
                expected += 1;
            }
        }
        assert_eq!(queue.expire(now), expected);
        assert_eq!(
            queue.expire(now),
            0,
            "timeouts complete each request only once"
        );
    }
}

#[test]
fn random_control_interleavings_complete_each_send_once_without_reassigning_replies() {
    for seed in 1..=64 {
        let mut random = seed;
        let mut now = 0_u64;
        let mut queue = SentControls::default();
        let mut transcript = Transcript::default();
        for _ in 0..1024 {
            let word = random_word(&mut random);
            match word % 9 {
                stream @ (0 | 1) if queue.has_capacity() => {
                    let generation = u64::try_from(transcript.requests.len()).unwrap() + 1;
                    let request = if stream == 0 {
                        ControlRequest::PersonalOpen(generation)
                    } else {
                        ControlRequest::Close(PendingClose {
                            window_id: i32::try_from((word >> 8) % 4).unwrap() + 1,
                            window_type: super::super::GENERIC_STORAGE_WINDOW_TYPE,
                            owner: match (word >> 16) % 3 {
                                0 => PendingCloseOwner::Personal(generation),
                                1 => PendingCloseOwner::Storage(generation),
                                _ => PendingCloseOwner::Cleanup,
                            },
                        })
                    };
                    transcript.send(&mut queue, request, now, usize::try_from(stream).unwrap());
                }
                2 => transcript.reply(&mut queue, 0),
                3 => transcript.reply(&mut queue, 1),
                4 => {
                    now += (word >> 8) % INVENTORY_REQUEST_TIMEOUT_MILLIS;
                    transcript.timeout(&mut queue, now);
                }
                5 if !transcript.requests.is_empty() => {
                    let generation =
                        (word >> 8) % u64::try_from(transcript.requests.len()).unwrap() + 1;
                    queue.supersede(generation);
                    transcript.superseded.extend(
                        transcript.requests.iter().enumerate().filter_map(
                            |(receipt, (request, _))| {
                                (request.generation() == Some(generation)).then_some(receipt)
                            },
                        ),
                    );
                }
                6 => {
                    queue.supersede_all();
                    transcript.superseded.extend(0..transcript.requests.len());
                }
                7 => interleavings::server_close(&mut queue, word),
                8 => {
                    // Finish the synthetic traffic before exercising a complete screen lifecycle.
                    while transcript.replies.iter().any(|replies| !replies.is_empty()) {
                        transcript.reply(&mut queue, 0);
                        transcript.reply(&mut queue, 1);
                    }
                    interleavings::world_drop_blocked_close(&mut queue, &mut transcript, now, word);
                }
                _ => {}
            }
            assert_eq!(
                queue.entries.len(),
                transcript.replies.iter().map(VecDeque::len).sum::<usize>()
            );
        }
        transcript.timeout(&mut queue, now + INVENTORY_REQUEST_TIMEOUT_MILLIS);
        while transcript.replies.iter().any(|replies| !replies.is_empty()) {
            transcript.reply(&mut queue, 0);
            transcript.reply(&mut queue, 1);
        }
        assert_eq!(
            transcript.completed.len(),
            transcript.requests.len(),
            "every sent request is answered or timed out exactly once"
        );
        assert!(queue.entries.is_empty());
    }
}

#[test]
fn full_fifo_keeps_timed_out_and_superseded_requests_until_their_replies_arrive() {
    let mut queue = SentControls::default();
    for generation in 1..=MAX_SENT_CONTROLS as u64 {
        queue.sent(ControlRequest::PersonalOpen(generation), 0);
        queue.supersede(generation);
    }
    assert!(!queue.has_capacity());
    assert_eq!(
        queue.expire(INVENTORY_REQUEST_TIMEOUT_MILLIS),
        MAX_SENT_CONTROLS
    );
    assert!(
        !queue.has_capacity(),
        "timeout must not discard reply correlation"
    );
    for generation in 1..=MAX_SENT_CONTROLS as u64 {
        let reply = queue.answer(ControlKind::PersonalOpen).unwrap();
        assert_eq!(reply.request, ControlRequest::PersonalOpen(generation));
        assert!(reply.superseded);
        assert!(reply.deadline_millis.is_none());
    }
    assert!(queue.has_capacity());
}
