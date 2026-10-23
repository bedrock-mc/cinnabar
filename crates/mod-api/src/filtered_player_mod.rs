//! Explicit opt-in player-mod bindings for return-value-only action filtering.
wit_bindgen::generate!({
    path: [
        "../experience-sdk/wit/client/deps/server-experience",
        "../experience-sdk/wit/session",
        "wit",
    ],
    world: "cinnabar:extension/filtered-player-mod@0.1.0",
    generate_all,
    pub_export_macro: true,
    export_macro_name: "export_filtered_player_mod",
});
