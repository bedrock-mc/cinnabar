//! Tool names, descriptions and input schemas.

use serde_json::{Value, json};

pub fn definitions() -> Value {
    let strings = json!({ "type": "array", "items": { "type": "string" } });
    let controls = json!({
        "type": "array", "items": { "type": "string" },
        "description": "Vanilla binding names (key.jump, key.attack, key.hotbar.1, key.togglePerspective), Bevy key names as local mods bind them (Digit1..Digit9, KeyF, F1, F5, Escape, Enter), or MouseLeft/MouseRight/MouseMiddle."
    });
    let keyframe = json!({
        "type": "object",
        "properties": {
            "t": { "type": "number", "description": "Seconds of game time after the path starts" },
            "position": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 },
            "yaw": { "type": "number", "description": "Bedrock yaw in degrees (0 faces +Z/south, 90 faces -X/west)" },
            "pitch": { "type": "number", "description": "Degrees; positive looks down" },
            "fov": { "type": "number", "description": "Field of view in degrees; omitted keeps the player's" },
            "easing": { "enum": ["linear", "ease_in", "ease_out", "ease_in_out", "step"] }
        },
        "required": ["t", "position", "yaw", "pitch"]
    });
    let condition = json!({
        "type": "object",
        "description": "Exactly one of: {\"in_world\": null}, {\"chunks_loaded\": {\"radius\": 4}}, {\"screen\": {\"name\": \"Gameplay\"}}, {\"entity\": {\"matches\": \"cinnabar:hollow_warden\", \"radius\": 32}}, {\"camera_finished\": null}. Unit conditions may also be given as the bare strings \"in_world\" / \"camera_finished\"."
    });
    json!([
        {
            "name": "launch_client",
            "description": "Start a client built with `cargo build -p bedrock-client --features developer-control` (add `local-mods` to load mods via env CINNABAR_MOD_COMPONENT) and attach to its control endpoint. headless hides the window; size is physical pixels at scale 1.",
            "inputSchema": { "type": "object", "properties": {
                "binary": { "type": "string", "description": "Client binary (default target/debug/bedrock-client under the checkout)" },
                "size": { "type": "array", "items": { "type": "integer" }, "description": "[width, height], default [1920, 1080]" },
                "headless": { "type": "boolean", "description": "Create the window hidden (default false)" },
                "args": strings,
                "env": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Extra environment, e.g. CINNABAR_MOD_COMPONENT" },
                "timeout_ms": { "type": "integer" }
            } }
        },
        {
            "name": "connect",
            "description": "Join a server. local_server starts the checkout's bedrock-local-server (make local-server) on a free loopback port first; pass showcase flags in local_server.args. Non-loopback addresses are refused unless allow_remote is set, since remote joins use the configured account.",
            "inputSchema": { "type": "object", "properties": {
                "address": { "type": "string", "description": "host:port" },
                "local_server": { "type": "object", "properties": {
                    "binary": { "type": "string" },
                    "world_dir": { "type": "string" },
                    "args": strings
                } },
                "allow_remote": { "type": "boolean" }
            } }
        },
        {
            "name": "input",
            "description": "Synthetic input delivered as real key and mouse events, so bindings, UI, menus and local mods (including showcase ability keys like Digit1..Digit4) see them. Held controls stay down until released. While driven, the window counts as focused and captured without grabbing the OS cursor; release_control hands it back.",
            "inputSchema": { "type": "object", "properties": {
                "hold": controls, "release": controls, "press": controls,
                "press_frames": { "type": "integer", "description": "Frames a press stays down (default 1)" },
                "move": { "type": "object", "properties": { "forward": { "type": "number" }, "strafe": { "type": "number" } }, "description": "Sign of each axis holds key.forward/back and key.right/left; 0 releases" },
                "jump": { "type": "boolean" }, "sneak": { "type": "boolean" }, "sprint": { "type": "boolean" },
                "pointer": { "oneOf": [{ "type": "object", "properties": { "x": { "type": "number" }, "y": { "type": "number" } }, "required": ["x", "y"], "additionalProperties": false }, { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 }], "description": "Absolute logical window pixels from top left, as {x,y} or [x,y], applied before button edges; combine with press:[MouseLeft] to click." },
                "wheel": { "type": "object", "properties": { "x": { "type": "number" }, "y": { "type": "number" }, "unit": { "enum": ["line", "pixel"] } }, "additionalProperties": false, "description": "One scroll event; axes default 0, unit defaults line, positive y scrolls up." },
                "hotbar": { "type": "integer", "minimum": 1, "maximum": 9 },
                "scroll": { "type": "object", "properties": { "x": { "type": "number" }, "y": { "type": "number" }, "pixels": { "type": "boolean" } }, "required": ["y"], "additionalProperties": false, "description": "One wheel delta in lines, or window-logical pixels when pixels is true. Positive y scrolls up." },
                "cursor": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2, "description": "Logical window coordinates for menu clicks and dragging; does not move the OS pointer." },
                "text": { "type": "string", "description": "Insert text into the focused editor through keyboard messages." },
                "look": { "type": "object", "properties": {
                    "yaw": { "type": "number" }, "pitch": { "type": "number" },
                    "relative": { "type": "boolean" }, "frames": { "type": "integer", "description": "Turn over this many frames (0 snaps)" }
                }, "required": ["yaw", "pitch"] },
                "release_all": { "type": "boolean" },
                "release_control": { "type": "boolean" }
            } }
        },
        {
            "name": "chat",
            "description": "Send a chat line as the player; a leading / sends a command (e.g. /showcase souls).",
            "inputSchema": { "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] }
        },
        {
            "name": "camera_path",
            "description": "Take over the rendered camera with eased keyframes on game time (a single keyframe is a fixed free camera); the player's own view, movement and server state are untouched. release: true returns the camera to the player. Press F1 to hide the HUD, F5 for third person.",
            "inputSchema": { "type": "object", "properties": {
                "keyframes": { "type": "array", "items": keyframe },
                "easing": { "enum": ["linear", "ease_in", "ease_out", "ease_in_out", "step"], "description": "Default ease_in_out" },
                "loop": { "type": "boolean" },
                "hide_hand": { "type": "boolean", "description": "Hide the first-person hand while scripted (default true; restored on release)" },
                "release": { "type": "boolean" }
            } }
        },
        {
            "name": "test_cape",
            "description": "Install or remove an original local cape for animation captures. Changes only the local player's presentation.",
            "inputSchema": { "type": "object", "properties": { "enabled": { "type": "boolean" } }, "required": ["enabled"], "additionalProperties": false }
        },
        {
            "name": "sign_in_fixture",
            "description": "Show a fixed placeholder sign-in state in a hidden client launched with CINNABAR_SIGN_IN_FIXTURE. Never starts authentication or opens a browser.",
            "inputSchema": { "type": "object", "properties": {
                "state": { "enum": ["waiting", "opened", "browser_failed", "success", "expired", "error"] }
            }, "required": ["state"], "additionalProperties": false }
        },
        {
            "name": "state",
            "description": "Position, rotation (Bedrock degrees), health, dimension, loaded chunk columns, nearby actors (nearest first), screen stack, menu, camera and recording status, and game time.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "wait_for",
            "description": "Block until a condition holds or timeout_ms (default 60000) passes.",
            "inputSchema": { "type": "object", "properties": {
                "condition": condition,
                "timeout_ms": { "type": "integer" }
            }, "required": ["condition"] }
        },
        {
            "name": "screenshot",
            "description": "Save the next rendered frame as a PNG (default under .local/developer-control/screenshots) and return it inline, downscaled to max_width (default 1280).",
            "inputSchema": { "type": "object", "properties": {
                "path": { "type": "string" },
                "inline": { "type": "boolean" },
                "max_width": { "type": "integer" }
            } }
        },
        {
            "name": "record_start",
            "description": "Record the window to MP4 through ffmpeg on PATH. fixed_clock (default true) steps game time exactly 1/fps per rendered frame, so the video is smooth however slow rendering is; use it with the local showcase server, since a remote server keeps wall-clock time (set fixed_clock false there for real-time capture). Game audio is captured and muxed in unless audio is false.",
            "inputSchema": { "type": "object", "properties": {
                "path": { "type": "string", "description": "Output .mp4 (default under .local/developer-control/recordings)" },
                "fps": { "type": "integer", "description": "Default 60" },
                "codec": { "enum": ["h264", "hevc"] },
                "fixed_clock": { "type": "boolean" },
                "audio": { "type": "boolean" }
            } }
        },
        {
            "name": "record_stop",
            "description": "Stop recording and wait for the encode (and audio mux) to finish; returns the path and frame count.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "quit",
            "description": "Ask the client to exit (killing it after a grace period) and stop any local server this session started.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}
