# Enhanced startup validation

The reported crash shortly after joining Zeqa remains unconfirmed on this
checkout. No speculative renderer change is included in this validation commit.

Two offline native Metal tests pass:

- `enhanced_from_startup_renders_populated_world_on_native_gpu`: enables HDR,
  Enhanced and Bloom before the first frame, then adds terrain, liquid, actors,
  particles, both nametag modes, world UI, screen UI, an isolated UI model,
  invert blending, an animated hand and a held cube. When the installed world
  carrier is available, it also rebuilds the terrain textures/material table.
  Actor and held-cube completion gates prove those draws executed. GPU readback,
  pipeline errors and Enhanced/vanilla round trips are checked.
- `enhanced_lobby_replay_on_native_gpu`: replays 120 frames from the local
  capture with its entity pack, up to 49 actor instances, the real HUD/menu
  carriers, generated nametags and FXAA. This actor replay excludes terrain
  packets and its local avatar is inactive; the populated test covers those
  missing renderer paths.

Run the first test through `cslot cargo test -p render --lib
enhanced::populated_tests`. Set `CINNABAR_REQUIRE_ENHANCED_GPU=1` to fail instead
of skipping when no native adapter exists. `CINNABAR_ENHANCED_FRAME` optionally
selects an external PNG path. The ignored lobby test needs externally supplied
`CINNABAR_LOBBY_CAPTURE` and `CINNABAR_RENDER_PACK` paths, then `cslot cargo test
-p bedrock-client --lib enhanced_lobby_replay_on_native_gpu -- --ignored`.
Neither test opens a socket. Captures, packs and PNGs stay outside git.

The Bloom, first-person and screen composition rules are recorded in
`enhanced-world-bloom.md`. They identify separate rendering roles, rather than
a cause for this crash.
Native validation is evidence for this Metal backend, not a closed parity gate
or proof that every backend and server payload is safe.
