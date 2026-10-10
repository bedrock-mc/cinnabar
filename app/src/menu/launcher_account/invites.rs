//! The invite screen's requests, the account's Xbox friends and the invites the player sends,
//! run in order on their own worker so neither waits on the polling workers.

use super::profile_worker::RESPONSE_TIMEOUT;
use super::*;

pub(super) enum Request {
    People,
    Send(Vec<String>),
}

/// Starts the worker; dropping the account link stops it and drops what is still queued.
pub(super) fn start(
    socket_dir: PathBuf,
    artwork_dir: PathBuf,
    shared: Arc<Mutex<Snapshot>>,
    stop: Receiver<()>,
) -> Sender<Request> {
    let (sender, requests) = crossbeam_channel::unbounded();
    thread::spawn(move || {
        let runtime = runtime();
        let images = artwork::people_images(&artwork_dir);
        loop {
            let request = crossbeam_channel::select_biased! {
                recv(stop) -> _ => return,
                recv(requests) -> request => match request {
                    Ok(request) => request,
                    Err(_) => return,
                },
            };
            match (request, runtime.as_ref()) {
                (Request::People, runtime) => {
                    let generation = auth_generation(&shared);
                    let people = runtime
                        .ok_or(())
                        .and_then(|runtime| people(runtime, &socket_dir, &images));
                    publish_account(&shared, generation, |snapshot| {
                        snapshot.people = Some(people)
                    });
                }
                (Request::Send(xuids), Some(runtime)) => send(runtime, &socket_dir, &xuids),
                (Request::Send(_), None) => bevy::log::warn!("game invites not sent: no runtime"),
            }
        }
    });
    sender
}

fn people(
    runtime: &tokio::runtime::Runtime,
    socket_dir: &std::path::Path,
    images: &client_ui::remote_images::ImageDirectory,
) -> Result<Vec<bridge::Person>, ()> {
    let listed = runtime.block_on(async {
        let mut listed =
            tokio::time::timeout(RESPONSE_TIMEOUT, bridge::list_people(socket_dir)).await;
        if let Ok(Ok(people)) = &mut listed {
            artwork::fill(images, artwork::people_slots(people)).await;
        }
        listed
    });
    match listed {
        Ok(Ok(people)) => Ok(people),
        Ok(Err(error)) => {
            bevy::log::warn!(%error, "friends list unavailable");
            Err(())
        }
        Err(_) => {
            bevy::log::warn!("friends list timed out");
            Err(())
        }
    }
}

/// Invites each friend in turn; one failure does not stop the others.
fn send(runtime: &tokio::runtime::Runtime, socket_dir: &std::path::Path, xuids: &[String]) {
    for xuid in xuids {
        let sent = runtime.block_on(async {
            tokio::time::timeout(RESPONSE_TIMEOUT, bridge::invite_to_world(socket_dir, xuid)).await
        });
        match sent {
            Ok(Ok(())) => {}
            Ok(Err(error)) => bevy::log::warn!(%error, "game invite not sent"),
            Err(_) => bevy::log::warn!("game invite timed out"),
        }
    }
}
