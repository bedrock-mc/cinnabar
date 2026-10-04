//! Shared production commit-to-input authority registrations.
use super::*;
use crate::runtime::world::drain_committed_ui_before_authority;

pub(crate) fn configure_client_frame_schedule(app: &mut App) {
    app.configure_sets(
        Update,
        (
            ClientFrameSet::RawInput,
            ClientFrameSet::SemanticSample,
            ClientFrameSet::UiAuthority,
            ClientFrameSet::SemanticFinalize,
            ClientFrameSet::Physics,
            ClientFrameSet::Camera,
            ClientFrameSet::Interaction,
            ClientFrameSet::WorldPublication,
            ClientFrameSet::ActorPreparation,
            ClientFrameSet::UiPreparation,
            ClientFrameSet::NetworkSend,
            ClientFrameSet::ActorPublication,
            ClientFrameSet::UiPublication,
        )
            .chain(),
    );
}

pub(crate) fn configure_client_authority_systems(app: &mut App) {
    app.add_plugins(client_presentation::ClientPresentationPlugin)
        .add_message::<crate::runtime::audio::SequencedAudioEvent>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .init_resource::<WorldStreamFramePoll>()
        .init_resource::<crate::ui_runtime::presentation::PreparedUiPublication>()
        .add_systems(
            Update,
            (drive_gameplay_touch_targets, collect_raw_input)
                .chain()
                .in_set(ClientFrameSet::RawInput),
        )
        .add_systems(
            Update,
            route_semantic_input.in_set(ClientFrameSet::SemanticSample),
        )
        .add_systems(
            Update,
            (
                crate::ui_runtime::scene_stack::close_scenes_on_player_hurt,
                drive_sign_editor.run_if(crate::server_experiences::input::ordinary_input),
                drive_server_form_input.run_if(crate::server_experiences::input::ordinary_input),
                drive_chat_ui_actions.run_if(crate::server_experiences::input::ordinary_input),
                drain_inventory_authority,
                drive_chat_keyboard_input.run_if(crate::server_experiences::input::ordinary_input),
                crate::fullscreen::toggle_fullscreen_hotkey,
                drive_menu_input,
                crate::fullscreen::apply_runtime_fullscreen_setting,
                crate::ui_runtime::presentation::apply_gui_scale_setting,
                crate::menu::persist_video_settings,
                drive_inventory_ui_actions.run_if(crate::server_experiences::input::ordinary_input),
                drive_menu_connection,
                crate::settings_runtime::apply_window_settings,
                crate::settings_runtime::apply_render_distance,
                crate::store::drive_store,
                synchronize_semantic_input_authority,
                drive_world_inventory_keys.run_if(crate::server_experiences::input::ordinary_input),
            )
                .chain()
                .in_set(ClientFrameSet::UiAuthority),
        )
        .add_systems(
            Update,
            finalize_semantic_input_after_ui_authority.in_set(ClientFrameSet::SemanticFinalize),
        )
        .add_systems(
            Update,
            reconcile_world_stream_before_physics
                .after(receive_network_events)
                .before(drain_committed_ui_before_authority)
                .before(ClientFrameSet::UiAuthority)
                .before(ClientFrameSet::Physics),
        )
        .add_systems(
            Update,
            drain_committed_ui_before_authority
                .after(reconcile_world_stream_before_physics)
                .before(ClientFrameSet::UiAuthority),
        );
}
