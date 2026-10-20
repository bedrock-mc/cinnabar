use super::*;

/// Repeated hosted prompts cannot change the displayed page or restart the caller's absolute window.
#[test]
fn repeated_signup_keeps_the_first_prompt_and_absolute_attempt_cutoff() {
    let (sender, receiver) = bounded(super::super::super::EVENT_CAPACITY);
    let mut signup = Signup::default();
    let now = Instant::now();
    assert!(
        signup
            .present("https://example.invalid/first", now, &sender)
            .is_ok()
    );
    let deadline = now + Duration::from_secs(u64::from(bridge::contract::AUTH_XBOX_SIGNUP_SECONDS));
    assert!(
        matches!(receiver.try_recv(), Ok(Message::State(AuthState::AwaitingXboxSignup { uri })) if uri == "https://example.invalid/first")
    );
    assert!(
        signup
            .present(
                "https://example.invalid/replacement",
                now + ACQUISITION_TIMEOUT,
                &sender
            )
            .is_ok()
    );
    assert!(
        receiver.is_empty(),
        "a repeated challenge replaced the original signed page"
    );
    assert_eq!(signup.deadline, Some(deadline));
    assert_eq!(
        signup.acquisition_cutoff(now).ok(),
        Some(now + ACQUISITION_TIMEOUT)
    );
    let near_expiry = deadline - Duration::from_secs(1);
    assert_eq!(signup.acquisition_cutoff(near_expiry).ok(), Some(deadline));
    assert!(
        signup.acquisition_cutoff(deadline).is_err(),
        "expiry admitted another service acquisition"
    );
}

/// Canceling the real pacing owner is immediate, and an unfinished page cannot appear as completed sign-in.
#[tokio::test]
async fn signup_wait_cancels_and_disconnected_presentation_fails_closed() {
    let (sender, receiver) = bounded(super::super::super::EVENT_CAPACITY);
    let mut signup = Signup::default();
    assert!(
        signup
            .present("https://example.invalid/signup", Instant::now(), &sender)
            .is_ok()
    );
    drop(receiver);
    let (cancel, mut cancelled) = oneshot::channel();
    cancel.send(()).unwrap();
    assert!(matches!(
        signup.wait(&mut cancelled).await,
        Err(Failure::Cancelled)
    ));
    let mut owner = NativeAuth::from_state(AuthState::AwaitingXboxSignup {
        uri: "https://example.invalid/signup".into(),
    });
    owner.poll();
    assert!(matches!(owner.state(), AuthState::Failed(_)));
    assert!(owner.take_profile().is_none());
}
