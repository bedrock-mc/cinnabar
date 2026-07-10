#![doc = include_str!("../README.md")]

pub mod auth;
pub mod batch;
pub mod config;
pub mod error;
pub mod gamedata;
#[cfg(feature = "server")]
pub mod listener;
pub mod raw;
pub mod stream;
pub mod valentine;
pub mod world;

pub use config::BedrockListenerConfig;
pub use error::JolyneError;
pub use gamedata::GameData;

#[cfg(feature = "server")]
pub use listener::{BedrockListener, RawListener};

#[cfg(all(feature = "server", feature = "raknet"))]
pub use listener::RakNetBuilder;

#[cfg(all(feature = "server", feature = "nethernet"))]
pub use listener::NetherNetBuilder;

// Client marker is always available when client feature is enabled
#[cfg(feature = "client")]
pub use stream::Client;

// Client type aliases require a transport
#[cfg(all(feature = "client", any(feature = "raknet", feature = "nethernet")))]
pub use stream::{ClientLogin, ClientPlay};

// Server marker and type aliases require server feature + transport
#[cfg(all(feature = "server", any(feature = "raknet", feature = "nethernet")))]
pub use stream::{Server, ServerLogin, ServerPlay};

pub use stream::{BedrockStream, Login, Play};
#[cfg(feature = "raknet")]
pub use tokio_raknet::protocol::reliability::Reliability;

pub use world::WorldTemplate;

pub use raw::{RawPacket, decode_packet_raw};
pub use valentine::{GAME_VERSION, PROTOCOL_VERSION};
