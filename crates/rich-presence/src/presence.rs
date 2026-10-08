//! The Discord client: queued activity updates, join subscriptions, invites and join requests.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};
use discord_presence::{Client, Event, event_handler::EventCallbackHandle, models::EventData};

use crate::{Publication, State, Target, join, launch, unix_seconds};

/// A Discord user asking to join the player's game from their card.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JoinRequest {
    pub user_id: u64,
    /// The requester's Discord username.
    pub name: String,
}

/// Work for the command thread; Discord commands block until Discord answers.
enum Command {
    Subscribe,
    Reply { user_id: u64, accept: bool },
}

/// Queues only changes; the library worker coalesces updates and applies Discord's rate limit.
pub struct Presence {
    client: Option<Client>,
    handlers: Vec<EventCallbackHandle>,
    connection: Arc<AtomicU64>,
    /// Feeds the command thread; dropping it ends that thread.
    commands: Option<Sender<Command>>,
    commander: Option<JoinHandle<()>>,
    joins: Receiver<String>,
    requests: Receiver<JoinRequest>,
    publication: Publication,
    /// Launch time, shown in the menus and while joining.
    started_at: u64,
}

impl Presence {
    pub fn start(application_id: u64) -> Self {
        let connection = Arc::new(AtomicU64::new(0));
        let mut client = Client::new(application_id);
        let (commands, queued) = crossbeam_channel::unbounded();
        let (accepted, joins) = crossbeam_channel::unbounded();
        let (asked, requests) = crossbeam_channel::unbounded();
        let epoch = Arc::clone(&connection);
        let resubscribe = commands.clone();
        let handlers = vec![
            client.on_connected(move |_| {
                epoch.fetch_add(1, Ordering::Relaxed);
                let _ = resubscribe.send(Command::Subscribe);
            }),
            client.on_activity_join(move |context| {
                if let EventData::ActivityJoin(event) = context.event
                    && let Some(address) = event.secret.as_deref().and_then(join::join_address)
                {
                    let _ = accepted.send(address.to_owned());
                }
            }),
            client.on_activity_join_request(move |context| {
                if let EventData::ActivityJoinRequest(event) = context.event
                    && let Some(request) = event.user.and_then(|user| {
                        Some(JoinRequest {
                            user_id: user.id?.parse().ok()?,
                            name: user.username.unwrap_or_default(),
                        })
                    })
                {
                    let _ = asked.send(request);
                }
            }),
        ];
        client.start();
        // The clone is released before shutdown, which needs the only handle to the worker.
        let commander_client = client.clone();
        let commander = std::thread::Builder::new()
            .name("discord-commands".into())
            .spawn(move || run_commands(application_id, commander_client, &queued))
            .ok();
        Self {
            client: Some(client),
            handlers,
            connection,
            commands: Some(commands),
            commander,
            joins,
            requests,
            publication: Publication::default(),
            started_at: unix_seconds(),
        }
    }

    /// `players` is how many are in the session now, for the party size.
    pub fn update(&mut self, state: State, target: Option<&Target>, players: Option<u32>) {
        if self.publication.changed(
            state,
            self.connection.load(Ordering::Relaxed),
            target,
            players,
            unix_seconds(),
        ) && let Some(client) = self.client.as_mut()
        {
            let started_at = self.publication.playing_since.unwrap_or(self.started_at);
            client.queue_activity(|_| state.activity(started_at, target, players));
        }
    }

    /// The address of the most recent invite the player accepted in Discord.
    pub fn take_join(&self) -> Option<String> {
        self.joins.try_iter().last()
    }

    /// The next Discord user asking to join, in arrival order.
    pub fn take_join_request(&self) -> Option<JoinRequest> {
        self.requests.try_recv().ok()
    }

    /// Answers a join request: accepting sends the requester an invite to the current game.
    pub fn reply(&self, user_id: u64, accept: bool) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(Command::Reply { user_id, accept });
        }
    }
}

fn run_commands(application_id: u64, mut client: Client, queued: &Receiver<Command>) {
    if let Err(error) = launch::register(application_id) {
        tracing::warn!(%error, "discord invites cannot start the game while it is closed");
    }
    while let Ok(command) = queued.recv() {
        let result = match command {
            Command::Subscribe => client
                .subscribe(Event::ActivityJoin, |args| args)
                .and_then(|_| client.subscribe(Event::ActivityJoinRequest, |args| args))
                .map(drop),
            Command::Reply {
                user_id,
                accept: true,
            } => client.send_activity_join_invite(user_id).map(drop),
            Command::Reply {
                user_id,
                accept: false,
            } => client.close_activity_request(user_id).map(drop),
        };
        if let Err(error) = result {
            tracing::debug!(%error, "discord command failed");
        }
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        self.handlers.clear();
        drop(self.commands.take());
        let commander = self.commander.take();
        if let Some(client) = self.client.take() {
            // Discord retries can sleep past the app's shutdown deadline.
            let _ = std::thread::Builder::new()
                .name("discord-shutdown".into())
                .spawn(move || {
                    if let Some(commander) = commander {
                        let _ = commander.join();
                    }
                    let _ = client.shutdown();
                });
        }
    }
}
