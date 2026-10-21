use super::*;

#[test]
fn fresh_session_blocks_play_until_the_replacement_welcome() {
    let host = Rc::new(Observations::default());
    host.live.set(true);
    let refresh = Rc::new(Intent {
        ready: Rc::clone(&host.ready),
        ..Intent::default()
    });
    let (_commands, pending) = mpsc::channel(4);
    let (mut joins, join_requests) = mpsc::channel(1);
    let (mut lifecycle, changed) = mpsc::channel(1);
    let (_cancel, cancelled) = oneshot::channel();
    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&refresh),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(refresh.ready.get());
    host.hold_welcome.set(true);
    refresh.request(Refresh::Fresh);
    assert!(
        !refresh.ready.get(),
        "fresh intent must immediately gate joins"
    );
    lifecycle.try_send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(!refresh.ready.get());
    assert_eq!(host.opens.get(), 2);
    assert!(requests(&host)[1].get("resume").is_none());
    host.hold_welcome.set(false);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(refresh.ready.get());
    let (reply, response) = oneshot::channel();
    joins.try_send(play::Request { reply }).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let lease = response.now_or_never().unwrap().unwrap().unwrap();
    assert_eq!(requests(&host)[2]["generation"], 2);
    drop(lease);
}

#[test]
fn sign_out_retries_busy_refusals_until_relay_play_teardown_finishes() {
    let refresh = Rc::new(Intent::default());
    let host = Rc::new(Observations::default());
    host.live.set(true);
    let (mut commands, pending) = mpsc::channel(4);
    let (mut joins, join_requests) = mpsc::channel(1);
    let (_lifecycle, changed) = mpsc::channel(1);
    let (_cancel, cancelled) = oneshot::channel();
    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&refresh),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    deliver(
        &host,
        json!({"type":"account", "account":{"state":"signed_in", "gamertag":"Fixture"}}),
    );
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let (reply, response) = oneshot::channel();
    joins.try_send(play::Request { reply }).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let (_, mut guard) = response
        .now_or_never()
        .unwrap()
        .unwrap()
        .unwrap()
        .into_parts();
    commands
        .try_send(refresh.account(AccountOperation::SignOut))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(guard.cancelled().now_or_never().is_some());
    drop(guard);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(requests(&host).last().unwrap()["op"], "sign_out");
    for attempt in 1..=2 {
        deliver(&host, json!({"type":"error", "error":"session_busy"}));
        assert!(task.as_mut().poll(&mut cx).is_pending());
        let sent = requests(&host).len();
        assert!(task.as_mut().poll(&mut cx).is_pending());
        assert_eq!(requests(&host).len(), sent, "busy retries must yield");
        host.now.set(Duration::from_secs(attempt));
        assert!(task.as_mut().poll(&mut cx).is_pending());
        assert_eq!(requests(&host).len(), sent + 1);
        assert_eq!(requests(&host).last().unwrap()["op"], "sign_out");
        assert_eq!(host.closes.get(), 0);
    }
    deliver(
        &host,
        json!({"type":"account", "account":{"state":"signed_out"}}),
    );
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        host.account.borrow().state(),
        ::launcher::menu::auth::AuthState::SignedOut
    );
    assert_eq!(host.closes.get(), 1);
    host.now.set(CONTROL_DEADLINE + Duration::from_secs(1));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        host.opens.get(),
        2,
        "confirmed sign-out must stop reconnects"
    );
}

#[test]
fn busy_sign_out_retries_keep_the_original_deadline() {
    let refresh = Rc::new(Intent::default());
    let host = Rc::new(Observations::default());
    host.live.set(true);
    let (mut commands, pending) = mpsc::channel(4);
    let (_joins, join_requests) = mpsc::channel(1);
    let (_lifecycle, changed) = mpsc::channel(1);
    let (_cancel, cancelled) = oneshot::channel();
    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&refresh),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    commands
        .try_send(refresh.account(AccountOperation::SignOut))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    deliver(&host, json!({"type":"error", "error":"session_busy"}));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    host.now.set(CONTROL_DEADLINE - Duration::from_millis(1));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(requests(&host).len(), 3);
    deliver(&host, json!({"type":"error", "error":"session_busy"}));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    host.now.set(CONTROL_DEADLINE);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.closes.get(), 1);
    let sent = requests(&host).len();
    host.now.set(CONTROL_DEADLINE * 2);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(requests(&host).len(), sent);
    assert_eq!(host.opens.get(), 1, "expired sign-out must not reconnect");
}

#[test]
fn play_lease_uses_the_account_session_and_retires_before_the_next_join() {
    let refresh = Rc::new(Intent::default());
    let host = Rc::new(Observations::default());
    host.live.set(true);
    let (_commands, pending) = mpsc::channel(4);
    let (mut joins, join_requests) = mpsc::channel(1);
    let (_lifecycle, changed) = mpsc::channel(1);
    let (_cancel, cancelled) = oneshot::channel();

    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&refresh),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let (reply, response) = oneshot::channel();
    joins.try_send(play::Request { reply }).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let mut lease = response.now_or_never().unwrap().unwrap().unwrap();
    assert_eq!(host.opens.get(), 2);
    let hello = requests(&host);
    assert_eq!(hello[1]["channel"], bridge::contract::RELAY_CHANNEL_PLAY);
    assert_eq!(hello[1]["join"], lease.io.join);
    assert!(matches!(
        lease.io.packs.as_mut().now_or_never(),
        Some(Ok(None))
    ));
    assert_eq!(hello[1]["resume"], "retained");
    assert_eq!(hello[1]["generation"], 1);
    let (reply, response) = oneshot::channel();
    joins.try_send(play::Request { reply }).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(matches!(
        response.now_or_never(),
        Some(Ok(Err(CloseReason::Protocol)))
    ));
    assert_eq!(host.opens.get(), 2);
    drop(lease);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    host.now.set(Duration::from_millis(1250));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.opens.get(), 3);
    let hello = requests(&host);
    assert_eq!(hello[2]["resume"], "retained");
}

#[test]
fn navigation_cancels_the_game_task_without_waiting_for_play_io() {
    let refresh = Rc::new(Intent::default());
    let host = Rc::new(Observations::default());
    host.live.set(true);
    let (_commands, pending) = mpsc::channel(4);
    let (mut joins, join_requests) = mpsc::channel(1);
    let (mut lifecycle, changed) = mpsc::channel(1);
    let paused = Rc::new(Cell::new(false));
    let (_cancel, cancelled) = oneshot::channel();

    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::clone(&paused),
        Rc::clone(&refresh),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let (reply, response) = oneshot::channel();
    joins.try_send(play::Request { reply }).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let (_, mut guard) = response
        .now_or_never()
        .unwrap()
        .unwrap()
        .unwrap()
        .into_parts();
    assert!(guard.cancelled().now_or_never().is_none());
    paused.set(true);
    lifecycle.try_send(()).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(guard.cancelled().now_or_never().is_some());
    assert_eq!(host.opens.get(), 2);
}

#[test]
fn sign_out_keeps_control_until_confirmation_after_the_game_releases_play() {
    let refresh = Rc::new(Intent::default());
    let host = Rc::new(Observations::default());
    host.live.set(true);
    let (mut commands, pending) = mpsc::channel(4);
    let (mut joins, join_requests) = mpsc::channel(1);
    let (_lifecycle, changed) = mpsc::channel(1);
    let (_cancel, cancelled) = oneshot::channel();

    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&refresh),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let (reply, response) = oneshot::channel();
    joins.try_send(play::Request { reply }).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let (_, mut guard) = response
        .now_or_never()
        .unwrap()
        .unwrap()
        .unwrap()
        .into_parts();
    commands
        .try_send(refresh.account(AccountOperation::SignOut))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(guard.cancelled().now_or_never().is_some());
    drop(guard);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        host.closes.get(),
        0,
        "play retirement must retain the sign-out exchange"
    );
    let event =
        serde_json::to_vec(&json!({"type":"account", "account":{"state":"signed_out"}})).unwrap();
    host.incoming
        .borrow_mut()
        .extend((event.len() as u32).to_be_bytes());
    host.incoming.borrow_mut().extend(event);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(host.closes.get(), 1);
    assert_eq!(host.opens.get(), 2, "sign-out must not retry control");
}

#[test]
fn play_readiness_is_gated_while_opening_and_retiring_a_cancelled_request() {
    let host = Rc::new(Observations::default());
    host.live.set(true);
    let intent = Rc::new(Intent {
        ready: Rc::clone(&host.ready),
        ..Intent::default()
    });
    let (_commands, pending) = mpsc::channel(4);
    let (mut joins, join_requests) = mpsc::channel(1);
    let (_lifecycle, changed) = mpsc::channel(1);
    let (_cancel, cancelled) = oneshot::channel();
    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&intent),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(intent.ready.get());
    host.hold_writes.set(true);
    let (reply, response) = oneshot::channel();
    joins.try_send(play::Request { reply }).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(
        !intent.ready.get(),
        "opening play must gate replacement joins"
    );
    drop(response);
    host.hold_writes.set(false);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(
        !intent.ready.get(),
        "cancelled play must retire before joining again"
    );
    host.now.set(Duration::from_millis(1250));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(intent.ready.get());
    let (reply, response) = oneshot::channel();
    joins.try_send(play::Request { reply }).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(matches!(response.now_or_never(), Some(Ok(Ok(_)))));
}

#[test]
fn busy_sign_out_waits_for_the_control_request_refill() {
    let intent = Rc::new(Intent::default());
    let host = Rc::new(Observations::default());
    host.live.set(true);
    let (mut commands, pending) = mpsc::channel(4);
    let (_joins, join_requests) = mpsc::channel(1);
    let (_lifecycle, changed) = mpsc::channel(1);
    let (_cancel, cancelled) = oneshot::channel();
    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&intent),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    commands
        .try_send(intent.account(AccountOperation::SignOut))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let refill = Duration::from_millis(bridge::contract::RELAY_CONTROL_REFILL_MILLIS as u64);
    for attempt in 0..8 {
        deliver(&host, json!({"type":"error", "error":"session_busy"}));
        assert!(task.as_mut().poll(&mut cx).is_pending());
        let sent = requests(&host).len();
        host.now
            .set(refill * (attempt + 1) - Duration::from_millis(1));
        assert!(task.as_mut().poll(&mut cx).is_pending());
        assert_eq!(
            requests(&host).len(),
            sent,
            "retry spent an unrefilled token"
        );
        host.now.set(refill * (attempt + 1));
        assert!(task.as_mut().poll(&mut cx).is_pending());
        assert_eq!(requests(&host).len(), sent + 1);
    }
    deliver(
        &host,
        json!({"type":"account", "account":{"state":"signed_out"}}),
    );
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        host.account.borrow().state(),
        ::launcher::menu::auth::AuthState::SignedOut
    );
    assert_eq!(host.closes.get(), 0);
}

#[test]
fn sign_out_rejects_queued_and_new_joins_until_confirmation() {
    let intent = Rc::new(Intent::default());
    let host = Rc::new(Observations::default());
    host.live.set(true);
    let (mut commands, pending) = mpsc::channel(4);
    let (mut joins, join_requests) = mpsc::channel(1);
    let (_lifecycle, changed) = mpsc::channel(1);
    let (_cancel, cancelled) = oneshot::channel();
    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&intent),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let (reply, response) = oneshot::channel();
    joins.try_send(play::Request { reply }).unwrap();
    commands
        .try_send(intent.account(AccountOperation::SignOut))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(matches!(
        response.now_or_never(),
        Some(Ok(Err(CloseReason::Protocol)))
    ));
    assert!(!intent.ready.get());
    for busy in [false, true] {
        if busy {
            deliver(&host, json!({"type":"error", "error":"session_busy"}));
            assert!(task.as_mut().poll(&mut cx).is_pending());
        }
        let (reply, response) = oneshot::channel();
        joins.try_send(play::Request { reply }).unwrap();
        assert!(task.as_mut().poll(&mut cx).is_pending());
        assert!(matches!(
            response.now_or_never(),
            Some(Ok(Err(CloseReason::Protocol)))
        ));
        assert_eq!(host.opens.get(), 1, "sign-out must not start gameplay");
    }
    deliver(
        &host,
        json!({"type":"account", "account":{"state":"signed_out"}}),
    );
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(intent.ready.get());
    let (reply, response) = oneshot::channel();
    joins.try_send(play::Request { reply }).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(matches!(response.now_or_never(), Some(Ok(Ok(_)))));
}

#[test]
fn browser_sign_out_confirmation_restores_offline_play_readiness() {
    let intent = Rc::new(Intent::default());
    let host = Rc::new(Observations::default());
    host.live.set(true);
    let (mut commands, pending) = mpsc::channel(4);
    let (mut joins, join_requests) = mpsc::channel(1);
    let (_lifecycle, changed) = mpsc::channel(1);
    let (_cancel, cancelled) = oneshot::channel();
    let mut task = Box::pin(run(
        FakeHost(Rc::clone(&host)),
        pending,
        join_requests,
        changed,
        Rc::new(Cell::new(false)),
        Rc::clone(&intent),
        cancelled,
    ));
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(task.as_mut().poll(&mut cx).is_pending());
    commands
        .try_send(intent.account(AccountOperation::SignOut))
        .unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(!intent.ready.get(), "pending sign-out must gate joins");
    deliver(
        &host,
        json!({"type":"account", "account":{"state":"signed_out"}}),
    );
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(
        intent.ready.get(),
        "confirmed sign-out must admit offline play"
    );
    let (reply, response) = oneshot::channel();
    joins.try_send(play::Request { reply }).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    let lease = response.now_or_never().unwrap().unwrap().unwrap();
    let first_play = requests(&host).last().unwrap().clone();
    drop(lease);
    assert!(task.as_mut().poll(&mut cx).is_pending());
    host.now.set(Duration::from_millis(1250));
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(
        intent.ready.get(),
        "offline play must reconnect after ending"
    );
    let (reply, response) = oneshot::channel();
    joins.try_send(play::Request { reply }).unwrap();
    assert!(task.as_mut().poll(&mut cx).is_pending());
    assert!(matches!(response.now_or_never(), Some(Ok(Ok(_)))));
    assert_ne!(
        requests(&host).last().unwrap()["generation"],
        first_play["generation"],
        "the next join needs a fresh control generation"
    );
}
