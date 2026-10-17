use super::*;

/// Keeps native decoding and packing on the existing artwork thread.
pub(super) fn start() -> (Sender<Request>, Receiver<Request>, Receiver<Packed>) {
    let (requests, jobs) = crossbeam_channel::unbounded();
    let (done, results) = crossbeam_channel::unbounded();
    let pending = jobs.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("menu-artwork".to_owned())
        .spawn(move || serve(&jobs, &done))
    {
        tracing::warn!("menu artwork worker unavailable: {error}");
    }
    (requests, pending, results)
}
