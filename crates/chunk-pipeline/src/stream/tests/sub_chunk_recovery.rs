use super::*;

/// Times out every attempt at `request`'s sub-chunks from `sent`, through the last retry, and
/// returns when the stream abandons them.
fn abandon(
    stream: &mut WorldStream,
    mut request: super::super::PendingSubChunkRequest,
    mut sent: Instant,
) -> Instant {
    for _ in 0..=super::super::MAX_SUB_CHUNK_RETRIES {
        acknowledge_request_sent(stream, &request, sent);
        sent += super::super::SUB_CHUNK_RESPONSE_TIMEOUT;
        stream.expire_sub_chunk_deadlines(sent);
        match stream.pop_next_request() {
            Some(retry) => request = retry,
            None => break,
        }
    }
    sent
}

/// A server too busy to answer in time once left a permanent hole: the abandoned sub-chunk was
/// never asked for again and its column never had collision, which froze a player beside it.
#[test]
fn an_abandoned_sub_chunk_is_requested_again_until_it_arrives() {
    let started = Instant::now();
    let (mut stream, keys, request) = stream_with_unsent_sub_chunks(1);
    let chunk = keys[0].chunk();
    let abandoned = abandon(&mut stream, request, started);
    assert_eq!(stream.stats().sub_chunk_retry_exhaustions, 1);
    assert!(!stream.loaded_columns.contains(&chunk));

    stream.request_due_sub_chunk_recoveries(abandoned);
    assert!(
        stream.pop_next_request().is_none(),
        "asked again only after a delay"
    );
    let due = abandoned + super::super::SUB_CHUNK_RECOVERY_DELAY;
    stream.request_due_sub_chunk_recoveries(due);
    let again = stream
        .pop_next_request()
        .expect("the abandoned sub-chunk is requested again");
    assert_eq!(
        (again.chunk, again.base_sub_chunk_y, again.count),
        (chunk, keys[0].y, 1)
    );

    acknowledge_request_sent(&mut stream, &again, due);
    apply_sub_chunk_result(
        &mut stream,
        keys[0],
        super::super::PreparedSubChunkResult::AllAir,
    );
    assert!(
        stream.loaded_columns.contains(&chunk),
        "the column now has collision"
    );
    assert!(stream.requests.recoveries.is_empty());
}

/// Each abandonment waits twice as long as the last before the next request.
#[test]
fn repeated_abandonment_backs_off() {
    let started = Instant::now();
    let (mut stream, keys, request) = stream_with_unsent_sub_chunks(1);
    let abandoned = abandon(&mut stream, request, started);
    stream.request_due_sub_chunk_recoveries(abandoned);
    let first = abandoned + super::super::SUB_CHUNK_RECOVERY_DELAY;
    stream.request_due_sub_chunk_recoveries(first);
    let again = stream.pop_next_request().expect("first recovery");
    let abandoned_again = abandon(&mut stream, again, first);
    assert_eq!(
        stream.requests.recoveries[&keys[0].chunk()][&keys[0].y].rounds,
        2
    );

    stream.request_due_sub_chunk_recoveries(abandoned_again);
    stream
        .request_due_sub_chunk_recoveries(abandoned_again + super::super::SUB_CHUNK_RECOVERY_DELAY);
    assert!(
        stream.pop_next_request().is_none(),
        "the second wait is longer"
    );
    stream.request_due_sub_chunk_recoveries(
        abandoned_again + super::super::SUB_CHUNK_RECOVERY_DELAY * 2,
    );
    assert!(stream.pop_next_request().is_some());
}

/// Receiving one staggered recovery makes only that slot authoritative.
#[test]
fn collision_waits_for_every_staggered_recovery_in_a_column() {
    let started = Instant::now();
    let (mut stream, keys, _) = stream_with_unsent_sub_chunks(2);
    let chunk = keys[0].chunk();
    stream.complete_requested_sub_chunk(keys[0], false);
    stream.request_due_sub_chunk_recoveries(started);
    let later = started + super::super::SUB_CHUNK_RECOVERY_DELAY / 2;
    stream.complete_requested_sub_chunk(keys[1], false);
    stream.request_due_sub_chunk_recoveries(later);

    let first_due = started + super::super::SUB_CHUNK_RECOVERY_DELAY;
    stream.request_due_sub_chunk_recoveries(first_due);
    let first = stream.pop_next_request().expect("the first slot is due");
    assert_eq!((first.base_sub_chunk_y, first.count), (keys[0].y, 1));
    assert!(stream.requests.collision_failures.contains(&chunk));
    acknowledge_request_sent(&mut stream, &first, first_due);
    apply_sub_chunk_result(
        &mut stream,
        keys[0],
        super::super::PreparedSubChunkResult::AllAir,
    );

    assert!(stream.requests.collision_failures.contains(&chunk));
    assert!(!stream.loaded_columns.contains(&chunk));
    assert!(!stream.authority.terrain().is_chunk_loaded(chunk));
    assert!(stream.authority.terrain().is_sub_chunk_loaded(keys[0]));
    assert!(!stream.authority.terrain().is_sub_chunk_loaded(keys[1]));

    let second_due = later + super::super::SUB_CHUNK_RECOVERY_DELAY;
    stream.request_due_sub_chunk_recoveries(second_due);
    let second = stream.pop_next_request().expect("the second slot is due");
    assert_eq!((second.base_sub_chunk_y, second.count), (keys[1].y, 1));
    acknowledge_request_sent(&mut stream, &second, second_due);
    apply_sub_chunk_result(
        &mut stream,
        keys[1],
        super::super::PreparedSubChunkResult::AllAir,
    );

    assert!(stream.requests.recoveries.is_empty());
    assert!(!stream.requests.collision_failures.contains(&chunk));
    assert!(stream.loaded_columns.contains(&chunk));
    assert!(stream.authority.terrain().is_chunk_loaded(chunk));
}

/// A new request range cannot make an abandoned slot outside that range authoritative.
#[test]
fn new_request_ranges_preserve_unrecovered_collision_failures() {
    let (mut stream, keys, _) = stream_with_unsent_sub_chunks(2);
    let chunk = keys[0].chunk();
    for &key in &keys {
        stream.complete_requested_sub_chunk(key, false);
    }
    stream.enqueue_request(chunk, keys[0].y, 1, None);
    apply_sub_chunk_result(
        &mut stream,
        keys[0],
        super::super::PreparedSubChunkResult::AllAir,
    );

    assert!(stream.requests.collision_failures.contains(&chunk));
    assert!(!stream.loaded_columns.contains(&chunk));
    assert!(!stream.authority.terrain().is_chunk_loaded(chunk));
    assert!(stream.authority.terrain().is_sub_chunk_loaded(keys[0]));
    assert!(!stream.authority.terrain().is_sub_chunk_loaded(keys[1]));
}

/// An empty resync does not fill an abandoned slot with authoritative air.
#[test]
fn empty_request_ranges_preserve_unrecovered_collision_failures() {
    let (mut stream, keys, _) = stream_with_unsent_sub_chunks(1);
    let key = keys[0];
    stream.complete_requested_sub_chunk(key, false);
    stream.enqueue_request(key.chunk(), key.y, 0, None);

    assert!(stream.requests.collision_failures.contains(&key.chunk()));
    assert!(!stream.loaded_columns.contains(&key.chunk()));
    assert!(!stream.authority.terrain().is_sub_chunk_loaded(key));
}

/// Abandons one requested slot in each of `count` columns, without starting their delays.
fn stream_with_abandoned_columns(count: i32) -> (WorldStream, Vec<SubChunkKey>) {
    let (mut stream, keys, _) = stream_with_unsent_sub_chunks(1);
    let first = keys[0];
    let mut abandoned = Vec::new();
    for x in 0..count {
        let key = SubChunkKey { x, ..first };
        if x != first.x {
            stream.enqueue_request(key.chunk(), key.y, 1, None);
        }
        stream.complete_requested_sub_chunk(key, false);
        abandoned.push(key);
    }
    (stream, abandoned)
}

/// Spent polls time one column at a time and resume rather than rescanning the first one.
#[test]
fn recovery_delay_initialization_respects_the_poll_budget() {
    let started = Instant::now();
    let (mut stream, keys) = stream_with_abandoned_columns(4);
    stream.poll_deadline = Some(started);

    for initialized in 1..=keys.len() {
        stream.request_due_sub_chunk_recoveries(started);
        assert_eq!(
            stream
                .requests
                .recoveries
                .values()
                .flat_map(|column| column.values())
                .filter(|recovery| recovery.due.is_some())
                .count(),
            initialized,
        );
        assert_eq!(stream.pending_request_count(), 0);
    }
}

/// A full outbound queue cannot turn a spent poll into a drain of every due recovery.
#[test]
fn due_recoveries_resume_under_budget_with_a_full_outbound_queue() {
    let started = Instant::now();
    let (mut stream, keys) = stream_with_abandoned_columns(4);
    stream.request_due_sub_chunk_recoveries(started);
    for index in 0..super::super::OUTBOUND_REQUEST_CAPACITY {
        let chunk = ChunkKey::new(0, 100 + i32::try_from(index).unwrap(), 0);
        stream.requests.queue.push_ready(
            super::super::PendingSubChunkRequest {
                demand: 0,
                chunk,
                base_sub_chunk_y: keys[0].y,
                count: 1,
            },
            false,
        );
    }
    stream.poll_deadline = Some(started);
    let due = started + super::super::SUB_CHUNK_RECOVERY_DELAY;
    for (index, &key) in keys.iter().enumerate() {
        stream.request_due_sub_chunk_recoveries(due);
        assert!(stream.requests.is_expected(key));
        assert_eq!(stream.outstanding_sub_chunk_count(), index + 1);
        assert_eq!(stream.requests.deferred_recovery.len(), index + 1);
        assert_eq!(
            stream.pending_request_count(),
            super::super::OUTBOUND_REQUEST_CAPACITY
        );
    }
    stream.request_due_sub_chunk_recoveries(due);
    assert_eq!(stream.requests.deferred_recovery.len(), keys.len());
}

/// A raised yield flag stops recovery work after one column and leaves the rest resumable.
#[test]
fn recovery_requests_honor_service_yield() {
    let started = Instant::now();
    let (mut stream, keys) = stream_with_abandoned_columns(4);
    stream.request_due_sub_chunk_recoveries(started);
    let yield_request = Arc::new(AtomicBool::new(true));
    stream.service_yield = Some(Arc::clone(&yield_request));
    let due = started + super::super::SUB_CHUNK_RECOVERY_DELAY;
    stream.request_due_sub_chunk_recoveries(due);

    assert_eq!(stream.outstanding_sub_chunk_count(), 1);
    assert_eq!(stream.pending_request_count(), 1);
    yield_request.store(false, Ordering::Release);
    stream.request_due_sub_chunk_recoveries(due);
    assert_eq!(stream.outstanding_sub_chunk_count(), keys.len());
    assert_eq!(stream.pending_request_count(), keys.len());
}

/// Eviction removes recovery slots without invalidating the next poll's cursor.
#[test]
fn recovery_cursor_resumes_after_its_column_is_evicted() {
    let started = Instant::now();
    let (mut stream, keys) = stream_with_abandoned_columns(4);
    stream.request_due_sub_chunk_recoveries(started);
    stream.poll_deadline = Some(started);
    let due = started + super::super::SUB_CHUNK_RECOVERY_DELAY;
    stream.request_due_sub_chunk_recoveries(due);
    assert!(stream.requests.is_expected(keys[0]));
    stream.evict_column(keys[0].chunk());

    assert!(!stream.requests.recoveries.contains_key(&keys[0].chunk()));
    for (index, &key) in keys.iter().enumerate().skip(1) {
        stream.request_due_sub_chunk_recoveries(due);
        assert!(stream.requests.is_expected(key));
        assert_eq!(stream.outstanding_sub_chunk_count(), index);
    }
}
