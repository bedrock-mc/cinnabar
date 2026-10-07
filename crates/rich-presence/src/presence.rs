//! The Discord client: queued activity updates, join subscriptions and invite delivery.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};
use discord_presence::{Client, Event, event_handler::EventCallbackHandle, models::EventData};

use crate::{Publication, State, Target, join, launch, unix_seconds};

/// Queues only changes; the library worker coalesces updates and applies Discord's rate limit.
pub struct Presence {
    client: Option<Client>,
    handlers: Vec<EventCallbackHandle>,
    connection: Arc<AtomicU64>,
    /// Wakes the subscriber after each connection; dropping it ends that thread.
    resubscribe: Option<Sender<()>>,
    subscriber: Option<JoinHandle<()>>,
    joins: Receiver<String>,
    publication: Publication,
    /// Launch time, shown in the menus and while joining.
    started_at: u64,
}

impl Presence {
    pub fn start(application_id: u64) -> Self {
        let connection = Arc::new(AtomicU64::new(0));
        let mut client = Client::new(application_id);
        let (resubscribe, connected) = crossbeam_channel::unbounded();
        let (accepted, joins) = crossbeam_channel::unbounded();
        let epoch = Arc::clone(&connection);
        let wake = resubscribe.clone();
        let handlers = vec![
            client.on_connected(move |_| {
                epoch.fetch_add(1, Ordering::Relaxed);
                let _ = wake.send(());
            }),
            client.on_activity_join(move |context| {
                if let EventData::ActivityJoin(event) = context.event
                    && let Some(address) = event.secret.as_deref().and_then(join::join_address)
                {
                    let _ = accepted.send(address.to_owned());
                }
            }),
        ];
        client.start();
        // Commands block until Discord answers, so they run off the frame; the clone is
        // released before shutdown, which needs the only handle to the worker.
        let subscriber_client = client.clone();
        let subscriber = std::thread::Builder::new()
            .name("discord-subscribe".into())
            .spawn(move || subscribe(application_id, subscriber_client, &connected))
            .ok();
        Self {
            client: Some(client),
            handlers,
            connection,
            resubscribe: Some(resubscribe),
            subscriber,
            joins,
            publication: Publication::default(),
            started_at: unix_seconds(),
        }
    }

    pub fn update(&mut self, state: State, target: Option<&Target>) {
        if self.publication.changed(
            state,
            self.connection.load(Ordering::Relaxed),
            target,
            unix_seconds(),
        ) && let Some(client) = self.client.as_mut()
        {
            let started_at = self.publication.playing_since.unwrap_or(self.started_at);
            client.queue_activity(|_| state.activity(started_at, target));
        }
    }

    /// The address of the most recent invite the player accepted in Discord.
    pub fn take_join(&self) -> Option<String> {
        self.joins.try_iter().last()
    }
}

fn subscribe(application_id: u64, mut client: Client, connected: &Receiver<()>) {
    if let Err(error) = launch::register(application_id) {
        tracing::warn!(%error, "discord invites cannot start the game while it is closed");
    }
    while connected.recv().is_ok() {
        if let Err(error) = client.subscribe(Event::ActivityJoin, |args| args) {
            tracing::debug!(%error, "discord join subscription failed");
        }
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        self.handlers.clear();
        drop(self.resubscribe.take());
        let subscriber = self.subscriber.take();
        if let Some(client) = self.client.take() {
            // Discord retries can sleep past the app's shutdown deadline.
            let _ = std::thread::Builder::new()
                .name("discord-shutdown".into())
                .spawn(move || {
                    if let Some(subscriber) = subscriber {
                        let _ = subscriber.join();
                    }
                    let _ = client.shutdown();
                });
        }
    }
}
