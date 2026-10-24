use super::*;

#[tokio::test]
async fn missing_native_home_owner_cannot_publish_or_report() {
    let shared = Mutex::new(Snapshot {
        home_source: Source(None),
        ..Default::default()
    });
    let (_wake, mut changed) = watch::channel(Wake::Account);
    let mut feed = Feed::default();
    let (home, _) = feed.round(None, &shared, &mut changed).await;
    assert!(home.is_none());
    assert!(shared.lock().unwrap().home.is_none());
    let report = Report::capture(&shared.lock().unwrap(), MessageEvent::default());
    assert!(matches!(
        report.send(&shared, &mut changed).await,
        ReportOutcome::Retired
    ));
}
