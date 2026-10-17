use super::*;

/// Keeps native decoding and packing on the existing artwork thread.
pub(super) fn start() -> (Sender<Request>, Receiver<Packed>) {
    start_with_spawn(|jobs, done| {
        std::thread::Builder::new()
            .name("menu-artwork".to_owned())
            .spawn(move || serve(&jobs, &done))
            .map(|_| ())
    })
}

/// Creates channels and hands the only request receiver to the worker launcher.
fn start_with_spawn(
    spawn: impl FnOnce(Receiver<Request>, Sender<Packed>) -> std::io::Result<()>,
) -> (Sender<Request>, Receiver<Packed>) {
    let (requests, jobs) = crossbeam_channel::unbounded();
    let (done, results) = crossbeam_channel::unbounded();
    if let Err(error) = spawn(jobs, done) {
        tracing::warn!("menu artwork worker unavailable: {error}");
    }
    (requests, results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_worker_start_rejects_requests_and_releases_payloads() {
        let channels = start_with_spawn(|_, _| Err(std::io::Error::other("worker unavailable")));
        let bytes: Arc<[u8]> = Arc::from([1, 2, 3]);
        for id in 1..=3 {
            let result = channels.0.try_send(Request {
                id,
                set: ArtworkSet {
                    images: vec![("fixture".into(), Arc::clone(&bytes))],
                    ..Default::default()
                },
            });
            assert!(matches!(
                result,
                Err(crossbeam_channel::TrySendError::Disconnected(_))
            ));
            drop(result);
            assert_eq!(Arc::strong_count(&bytes), 1);
        }
    }
}
