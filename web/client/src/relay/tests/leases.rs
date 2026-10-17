use super::*;

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
    let lease = response.now_or_never().unwrap().unwrap().unwrap();
    assert_eq!(host.opens.get(), 2);
    let hello = requests(&host);
    assert_eq!(hello[1]["channel"], bridge::contract::RELAY_CHANNEL_PLAY);
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
