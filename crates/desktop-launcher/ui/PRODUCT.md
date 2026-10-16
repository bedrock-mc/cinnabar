# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

Svelte 5 + TypeScript + Vite inside a Tauri 2 webview (WKWebView on macOS, WebView2 on Windows, WebKitGTK on Linux). The Rust app in `crates/desktop-launcher` owns behavior; the UI reaches it only through `src/api/`.

## Users

Desktop Bedrock players on macOS, Windows and Linux, mostly multiplayer: PvP, survival and minigame servers. Many arrive from Java launchers such as Lunar or Feather and expect that class of polish. They open the launcher for seconds at a time, usually in the evening, on their way into the game.

## Product Purpose

Cinnabar is an independent Bedrock client with its own engine. The launcher is its front door: it shows what is installed, gets the player into the game in one click, installs updates only when the player asks, and offers servers to join directly. Success is a player who is playing within seconds and always knows which version and channel they are on.

## Positioning

The launcher for a client whose engine is its own: signed builds on published channels (Beta and Nightly today), updates that never install without a click, and offline play of the installed build. Neighboring Java launchers can't claim a Bedrock engine; Bedrock's official launcher has no release channels.

## Operating Context

- Opened before play and returned to after the game closes or crashes.
- Runs beside a full-screen game, so it must idle cheaply and stop rendering while the game runs.
- Network can be absent; the installed build still plays.

## Capabilities and Constraints

- Contract types: install state, update state (checking, available, required, downloading, verifying, installing, failed), game state (starting, running, stopped or crashed), cached news, featured servers. See `src/api/types.ts`.
- Proposed but not yet in the contract: account identity (gamertag, skin), server MOTD, tags and ping. The UI renders them when present and degrades without them.
- Only published channels may be shown. Stable stays hidden until it publishes.
- No store, mods or friends exist yet. They must not appear as destinations until they work.
- The window is frameless with a custom title bar. On macOS the traffic lights sit at the top left.

## Brand Commitments

- Name: Cinnabar, the red mercury-sulfide ore. Mark: a three-faced isometric block in cinnabar reds (`packaging/icons/cinnabar.svg`).
- Shipped art is original. No Mojang or Minecraft assets, names or trademarks in shipped files.
- Fonts must be original or OFL-licensed.

## Evidence on Hand

- Brand mark SVG and the Cinnangles typefaces in `assets/fonts`.
- No real news, server listings or player skins yet. Mock data is synthetic and labeled as such in `src/api/mock.ts`.

## Product Principles

1. Play is one click and never waits on the network.
2. State is always legible: installed version, channel, and what the launcher is doing right now.
3. Nothing installs without an explicit click, and failure never strands the installed build.
4. Spend spectacle on one scene; keep everything else fast, quiet and exact.
5. Only ship destinations that work.

## Accessibility & Inclusion

WCAG 2.2 AA contrast for text and controls. Full keyboard operation with visible focus. `prefers-reduced-motion` replaces animation with a static equivalent rather than removing feedback. The 3D scene is decorative and hidden from assistive tech.
