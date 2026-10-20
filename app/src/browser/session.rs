//! Browser host admission and lifetime around the shared protocol login and session pump.
use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc};

use bevy::prelude::{AppExit, MessageWriter, NonSend, ResMut};
use client_runtime::network::NetworkHandle;
use simulation_clock::identity::SessionGeneration;

use crate::{
    game_session::SessionWorld,
    runtime::network::{resource_packs::PackApplication, session::Preparation},
};

/// A terminal session observation for the page's connection controls.
#[derive(Debug)]
pub enum Event {
    Disconnected,
    Failed(String),
    Transfer { host: String, port: u16 },
}

/// One pending host operation; replacing it drops its uninstalled session queues.
enum Command {
    Join {
        generation: u64,
        network: NetworkHandle<PackApplication>,
    },
    Disconnect,
    Stop,
}

/// A single bounded command and terminal observation shared with the browser event loop.
#[derive(Default)]
struct Mailbox {
    command: Option<Command>,
    event: Option<Event>,
    generation: u64,
    connected: bool,
}

/// Page-owned entry to the game, independent of browser transport implementation.
pub struct Host {
    mailbox: Rc<RefCell<Mailbox>>,
    preparation: Preparation,
    skin: protocol::ClientSkin,
    identity: protocol::ClientRandomId,
}

/// Game-side ownership of the page's bounded command slot.
pub(super) struct HostCommands(Rc<RefCell<Mailbox>>);

impl Host {
    /// Shares verified immutable join inputs with the game-side command consumer.
    pub(super) fn new(
        preparation: Preparation,
        skin: protocol::ClientSkin,
        identity: protocol::ClientRandomId,
    ) -> (Self, HostCommands) {
        let mailbox = Rc::new(RefCell::new(Mailbox::default()));
        let commands = HostCommands(Rc::clone(&mailbox));
        (
            Self {
                mailbox,
                preparation,
                skin,
                identity,
            },
            commands,
        )
    }

    /// Creates one shared login and pump; the page must run it alongside the I/O writer.
    pub fn connect<R, E, J, A>(
        &mut self,
        io: protocol::SessionIo<R>,
        join: u64,
        packs: impl Future<Output = Result<Option<A>, protocol::ProtocolError>> + 'static,
        address: String,
        executor: E,
        jobs: J,
    ) -> Result<Pin<Box<dyn Future<Output = ()>>>, String>
    where
        R: client_session::SessionRead + 'static,
        E: client_session::NetworkExecutor + 'static,
        J: protocol::JoinPreparation + 'static,
        A: tokio::io::AsyncRead + Unpin + 'static,
    {
        let mut mailbox = self.mailbox.borrow_mut();
        let generation = mailbox
            .generation
            .checked_add(1)
            .ok_or("session generation exhausted")?;
        let mut preparation = self.preparation.clone();
        preparation.session_generation = SessionGeneration::new(generation);
        let memory_tier = protocol::login_memory_tier(preparation.physical_memory_bytes);
        let skin = self.skin.clone();
        let identity = self.identity;
        let (network, work) = client_session::prepare_network(
            preparation.session_generation,
            move |cancelled| async move {
                let settings = protocol::LoginSettings {
                    memory_tier,
                    ..Default::default()
                };
                protocol::LoginSequence::connect_web_prepared(
                    io,
                    protocol::SessionLogin {
                        display_name: launcher::PRODUCT_NAME,
                        client_random_id: identity,
                        cache: None,
                        skin: Some(skin),
                        settings: &settings,
                        tap: None,
                        #[cfg(not(target_arch = "wasm32"))]
                        relay: None,
                        target: Some(bridge::ConnectTarget::RakNet(address)),
                    },
                    join,
                    packs,
                    &jobs,
                    move |handoff, data| {
                        let prepared = preparation.run(handoff, &data, &|| *cancelled.borrow());
                        (data, prepared)
                    },
                )
                .await
            },
            executor,
            client_session::SessionTrace::default(),
        );
        mailbox.generation = generation;
        mailbox.event = None;
        mailbox.command = Some(Command::Join {
            generation,
            network: network.into(),
        });
        Ok(Box::pin(work))
    }

    /// Retires the current join or world at the next game authority boundary.
    pub fn disconnect(&mut self) {
        let mut mailbox = self.mailbox.borrow_mut();
        mailbox.command = Some(Command::Disconnect);
        mailbox.event = None;
    }

    /// Takes the current terminal observation once, without retaining an unbounded history.
    pub fn take_event(&mut self) -> Option<Event> {
        self.mailbox.borrow_mut().event.take()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.mailbox.borrow_mut().command = Some(Command::Stop);
    }
}

/// Applies page intent before network ingress can publish a replacement world.
pub(super) fn apply_commands(
    commands: NonSend<HostCommands>,
    mut network: ResMut<NetworkHandle<PackApplication>>,
    mut state: SessionWorld,
    mut exit: MessageWriter<AppExit>,
) {
    let mut mailbox = commands.0.borrow_mut();
    let Some(command) = mailbox.command.take() else {
        return;
    };
    network.shutdown();
    match command {
        Command::Join {
            generation,
            network: next,
        } => {
            debug_assert_eq!(generation, mailbox.generation);
            state.retire(generation);
            *state.movement = client_interaction::movement::MovementTicker::with_epoch_publisher(
                next.physics_epoch_publisher(),
            );
            *network = next;
            mailbox.connected = true;
        }
        Command::Disconnect | Command::Stop => {
            state.retire(0);
            *network = NetworkHandle::disconnected();
            mailbox.connected = false;
            mailbox.event = Some(Event::Disconnected);
            if matches!(command, Command::Stop) {
                exit.write(AppExit::Success);
            }
        }
    }
}

/// Moves terminal controls to the page after the shared network owner applies them.
pub(super) fn observe_terminal(
    commands: NonSend<HostCommands>,
    mut network: ResMut<NetworkHandle<PackApplication>>,
    mut state: SessionWorld,
) {
    let mut mailbox = commands.0.borrow_mut();
    if !mailbox.connected {
        return;
    }
    let event = if let Some(transfer) = state.client_world.transfer_notice.take() {
        Some(Event::Transfer {
            host: transfer.host,
            port: transfer.port,
        })
    } else {
        state.fatal_error.0.take().map(Event::Failed)
    };
    if let Some(event) = event {
        network.shutdown();
        state.retire(0);
        *network = NetworkHandle::disconnected();
        mailbox.connected = false;
        mailbox.event = Some(event);
    }
}
