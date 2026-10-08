# Driving and recording the client (cinnabar-mcp)

`tools/cinnabar-mcp` is a stdio MCP server that launches a client built with
`cargo build -p bedrock-client --features developer-control` (plus `local-mods` for mods) and drives
it over a loopback, token-authenticated endpoint written under `.local/developer-control/`. The tool
schemas document every argument. Agents drive the client only through this endpoint, headless
(`headless: true`); never with OS-level input or window automation.

- `connect` with `local_server` runs `make local-server`'s binary on a free loopback port. Remote
  joins need `allow_remote`, since they sign in with the configured account; ask the owner first.
  The local server accepts `/give <item> [count] [data]`, `/fly`, `/speed`, and `/tp` for captures.
- `input` sends real key and mouse events, so mods' ability keys (`Digit1`…) work; `F1` hides the HUD.
  Hotkey toggles and the camera path's hidden hand stay in memory and are never saved.
  `pointer: {x, y}` moves in logical window pixels; combine it with `press: ["MouseLeft"]` to click.
  `wheel: {y: -3}` scrolls down three lines; add `unit: "pixel"` for precise scrolling.
- `test_cape` with `enabled: true` installs an original cape on the local player for captures;
  `false` removes it. This presentation fixture does not modify the server or saved skin.
- Sign-in captures launch with `headless: true`, an isolated `CINNABAR_USER_ROOT`, and
  `env: {"CINNABAR_SIGN_IN_FIXTURE": "opened"}`. `sign_in_fixture` switches between
  `waiting`, `opened`, `browser_failed`, `success`, `expired`, and `error`. These fixed states accept no account
  data, skip account services, and never open a browser. Save screenshots beneath `/private/tmp`.
- `test_accounts` with `enabled: true` shows a signed-in launcher whose Accounts picker lists
  placeholder accounts, so recordings never show real gamertags; it never signs in or saves, and
  needs a signed-out install (an isolated `CINNABAR_USER_ROOT`).
- `record_start` needs `ffmpeg` on PATH. Its default fixed clock steps game time exactly 1/fps per
  rendered frame, so it suits the local showcase server; record remote servers with
  `fixed_clock: false`. Audio is captured to a WAV and muxed in.

```json
{ "mcpServers": { "cinnabar": { "command": "target/debug/cinnabar-mcp", "args": ["--repo", "."] } } }
```
