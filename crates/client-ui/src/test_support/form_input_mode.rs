//! Original form artwork for device-switching tests and offline captures.

use crate::ui_runtime::presentation::forms::ServerUiPack;

/// Creates a form with a cyan image that is visible only during gamepad use.
pub fn pack() -> ServerUiPack {
    ServerUiPack {
        ui_layers: vec![vec![(
            "ui/server_form.json".into(),
            br##"{
                "namespace":"server_form",
                "long_form":{"type":"panel","size":[260,120],"controls":[
                    {"caption":{"type":"label","text":"Controller prompt","size":[240,20],"offset":[0,-35]}},
                    {"hint":{"type":"image","size":[100,30],"color":[0.2,0.8,1.0],
                        "bindings":[{"binding_type":"global","binding_name":"#is_using_gamepad","binding_name_override":"#visible"}]}}
                ]}
            }"##.to_vec(),
        )]],
        ..Default::default()
    }
}
