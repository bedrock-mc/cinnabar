//! Camera, actor, equipment and audio presentation over borrowed client observations.
pub mod actor_clock;
pub mod actor_publication;
pub mod actor_sampling;
pub mod dropped_items;
pub mod prepared_actor_artwork;
pub mod presentation;
pub mod seat_defaults;
pub mod session_assets;

pub mod audio;
pub mod audio_ingress;
pub mod camera;
pub mod local_player;
pub mod local_player_camera_receipt;
pub mod named_audio;
pub mod observations;
pub mod server_camera;
pub mod session_audio;

pub mod actor_feed;

mod plugin;
pub use plugin::ClientPresentationPlugin;
