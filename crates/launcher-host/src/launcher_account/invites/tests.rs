use super::*;

#[tokio::test]
async fn missing_native_people_owner_remains_unavailable() {
    assert!(Source(None).fetch().await.is_err());
}

#[tokio::test]
async fn unavailable_native_invitation_never_dispatches() {
    assert!(Destination(None).send("456").await.is_err());
}
