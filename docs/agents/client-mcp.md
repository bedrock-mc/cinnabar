# Driving and recording the client (cinnabar-mcp)

`tools/cinnabar-mcp` is a stdio MCP server that launches a client built with
`cargo build -p cinnabar --features developer-control` and drives
it over a loopback, token-authenticated endpoint written under `.local/developer-control/`. The tool
schemas document every argument. Agents drive the client only through this endpoint, headless
(`headless: true`); never with OS-level input or window automation.

- `connect` with `local_server` runs `make local-server`'s binary on a free loopback port. Remote
  joins need `allow_remote`, since they sign in with the configured account; ask the owner first.
  The local server accepts `/give <item> [count] [data]`, `/fly`, `/speed`, and `/tp` for captures.
- `input` sends real key and mouse events, so mods' ability keys (`Digit1`…) work; `F1` hides the HUD.
  To test Retina rendering headlessly, pass `CINNABAR_WINDOW_SCALE_FACTOR=2` in the
  launch environment. Capture sizes stay in physical pixels; pointer input uses
  logical window pixels. The default scale factor is 1.
  Hotkey toggles and the camera path's hidden hand stay in memory and are never saved.
  `pointer: {x, y}` moves in logical window pixels; combine it with `press: ["MouseLeft"]` to click.
  `wheel: {y: -3}` scrolls down three lines; add `unit: "pixel"` for precise scrolling.
- `test_cape` with `enabled: true` installs an original cape on the local player for captures;
  `false` removes it. This presentation fixture does not modify the server or saved skin.
- Sign-in captures launch with `headless: true`, an isolated `CINNABAR_USER_ROOT`, and
  `env: {"CINNABAR_SIGN_IN_FIXTURE": "opened"}`. `sign_in_fixture` switches between
  `waiting`, `opened`, `browser_failed`, `success`, `expired`, and `error`. These fixed states accept no account
  data, skip account services, and never open a browser. Save screenshots in a temporary directory
  outside the checkout.
- `test_accounts` with `enabled: true` shows a signed-in launcher whose Accounts picker lists
  placeholder accounts, so recordings never show real gamertags; it never signs in or saves, and
  needs a signed-out install (an isolated `CINNABAR_USER_ROOT`).
- `global_packs` stages and applies global resource packs as the settings screen does; import
  packs with `launch_client` args `["--import-pack", path]`, then poll it until `pending` clears.
- `log_filter_set` replaces the client's runtime log filter: `spec` takes comma-separated
  `category=level` directives (such as `net=debug`; the tool description lists the categories)
  and raw tracing `EnvFilter` directives, layered over the defaults and `RUST_LOG`.
  `log_filter_get` reports the runtime `spec` and the `effective` filter.
- `state.player_motion` includes the simulation tick, velocity, accepted motion sequence, jump
  eligibility, physical jump state, processed sprint state, active item use and the selected
  item's resolved use profile. `state.local_mods` also reports bounded
  host status and validated panel controls, so tests can verify module toggles and settings
  directly before sending a stimulus. These diagnostics do not grant movement authority.
- `record_start` needs `ffmpeg` on PATH. Its default fixed clock steps game time exactly 1/fps per
  rendered frame, so it suits the local showcase server; record remote servers with
  `fixed_clock: false`. Audio is captured to a WAV and muxed in.

```json
{ "mcpServers": { "cinnabar": { "command": "target/debug/cinnabar-mcp", "args": ["--repo", "."] } } }
```

Keep default features enabled for client MCP builds so acceptance evidence remains available.
`make package-binaries` excludes the acceptance harness and developer control.

## Join timing logs

Default developer builds include the `bridge/join-profile` feature through
`acceptance`; shipping `--no-default-features` builds omit it. `make play-build`
builds the core with `-tags joinprofile`; packaging leaves that tag off. For a
manual core build, use `go build -tags joinprofile` when collecting these timings.

Set `CINNABAR_LOG=join_profile=debug,core=debug` before launching. Rust milestones
and core logs share `join_profile_id`. Rust `elapsed_ms` starts when the accepted
menu request runs, including a wait for sign-in and core readiness. Core
`core_elapsed_ms` starts when it receives Connect; packet `dial_elapsed_ms` starts
at that dial attempt, including automatic pre-login transfers. HTTP records carry
`http_request_id`; background account requests can have no join ID. They log
redacted hosts and timing fields, with no URLs, headers or bodies.

The `terrain_ready` milestone uses the existing loading-screen gate and queues
loading completion. The terminal `first_world_presented` milestone waits until
the render world presents a frame extracted after that gate, with the loading
screen removed. It includes rendering stalls after readiness; a skipped present
does not complete the attempt. The timing includes completion of the submitted GPU
work. Hidden windows render without an OS swap chain, so headless runs instead
end at `first_world_frame_completed` after the same loading-free frame's GPU work.
Neither endpoint claims OS display feedback. `player_spawn` is an earlier
protocol milestone. Independent `duration_ms` sections, transport
setup and authentication can overlap; use cumulative milestones for total join
time. Cancellation, failure and replacement end the old attempt. Detailed frame
and packet traces remain available for attributing work between these milestones.

`devtool diag` includes both processes in `join-timeline.txt` alongside the full
retained logs, without uploading them. Client and core sections retain their
own monotonic origins; correlate them by ID and their recorded timestamps.
