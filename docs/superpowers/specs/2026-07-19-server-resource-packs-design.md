# Server-Supplied Resource Pack Ingestion Design

**Status:** Proposed on 2026-07-19
**Canonical base:** `fe698f5` (`main`)
**Scope:** Runtime ingestion of server-supplied Bedrock resource packs so that
custom-content servers (Zeqa and similar) render correctly — glyphs first, then
item and block textures. This is a design and decomposition, not an
implementation approval.

## Objective

Let Cinnabar display the custom glyphs, items, and blocks that servers like
Zeqa deliver through their own resource pack, instead of rendering private-use
glyphs as tofu and custom items/blocks as missing textures. The design must do
this **without weakening the build-time provenance discipline** that governs
the vanilla asset pipeline.

## Motivation

Zeqa's entire scoreboard is drawn with custom glyphs from its resource pack —
the scoreboard text arrives over ordinary `SetDisplayObjective` / `SetScore`
packets, but the characters are Unicode private-use-area codepoints
(`U+E000`+) whose art lives only in the server's `font/glyph_*.png` pages.
Without those pages the scoreboard renders as tofu. The same is true of Zeqa's
items and blocks: they reference textures that exist only inside the
server-supplied pack. Every popular custom server has this shape, so this is a
prerequisite for correct rendering on the servers users actually play.

## The core decision: a second, untrusted ingestion path

Every asset the client renders today comes from **build-time, offline,
provenance-pinned** compilation of the *known* vanilla pack — the `.mcbehud`
and `MCBEFONT1` carriers, the SHA256-pinned manifests, and the fail-closed
startup contract in `AGENTS.md`. Server packs are the philosophical inverse:

- **Runtime** — downloaded during login, not available at build time.
- **Per-server and unknown** — content cannot be pinned or pre-hashed.
- **Untrusted** — arbitrary bytes from a third party; a decompression bomb,
  an oversized atlas, or a malformed manifest must fail safely, never panic or
  exhaust memory.

So this feature is not "load more textures." It is standing up a **parallel
runtime ingestion path** next to the offline compiler, with its own trust
boundary. That is why it warrants a design pass and its own phase rather than
being folded into unrelated work. The good news (below) is that the *rendering*
machinery it feeds already exists and is reusable — the new surface is the
ingestion + trust boundary, which is narrower than "resource pack support"
sounds.

## Current-state audit

### Reusable foundations (already built)

- **Glyph atlas format** — `crates/assets/src/font.rs` defines the `MCBEFONT1`
  carrier with `GlyphMetrics { codepoint, page, uv, bearing, advance_64 }` and
  `FontTexturePage { rgba8, width, height, .. }`, plus bounds
  (`MAX_FONT_PAGES = 256`, `MAX_FONT_GLYPHS = 65_536`,
  `MAX_FONT_PAGE_SIDE = 4_096`, `MAX_FONT_SOURCE_BYTES = 64 MiB`). The
  codepoint→page→UV mapping that private-use glyphs need **already exists**.
- **Pack JSON parsing** — `crates/asset-compiler/src/pack/mod.rs` reads
  `blocks.json`, `textures/terrain_texture.json`, and
  `textures/flipbook_textures.json` into `BlockTextureMap` / `TerrainTextureMap`
  / `FlipbookSource`, with `resolve_texture_key`. The block/terrain resolution
  logic is reusable.
- **Item textures** — `crates/assets/src/item.rs` already carries item texture
  data.
- **Download transport** — the Go core is a gophertunnel proxy
  (`core/proxy/proxy.go`). gophertunnel's dialer downloads and decrypts server
  resource packs during login by default, so the wire handshake
  (`ResourcePacksInfo` → `ResourcePackClientResponse` → `ResourcePackDataInfo`
  → chunk request/data, content-key decryption) is **not** ours to build.

### The gap

- **The Go boundary drops the pack.** The upstream `Dialer`
  (`newUpstreamDialer`) downloads Zeqa's pack, but the downstream
  `minecraft.ListenConfig` (`proxy.go:57`) serves **no** packs to the Rust
  client and completes its own empty resource-pack handshake. The pack is
  downloaded and then discarded at the Go/Rust seam. *(First design task is to
  confirm this precisely and choose the forwarding mechanism — see Open
  Questions.)*
- **Every Rust ingestion path is offline and pinned.** `font.rs`, `pack/mod.rs`,
  and `item.rs` all read from trusted local build inputs. There is no runtime
  path that accepts arbitrary pack bytes, applies bounds/guards, and registers
  the result into the live atlases.

## Design decomposition

Ordered by dependency; annotated with new-vs-reuse. Each step is independently
shippable, and step 3 alone fixes the Zeqa scoreboard.

1. **Go: forward the pack across the seam.** Wire the upstream-downloaded packs
   through to the Rust client — either by serving them from the downstream
   listener (gophertunnel's standard proxy pack-forwarding) or by handing the
   decrypted pack bytes/paths to the Rust side over the existing channel.
   *Mostly gophertunnel wiring; confirm current relay behavior first.*
2. **Rust: runtime pack ingestion + trust boundary.** Accept pack bytes, read
   the manifest, and extract the relevant subtrees, applying strict bounds
   (reuse the `MAX_FONT_*` philosophy: size caps, page/glyph counts,
   decompression-bomb guard, per-server memory ceiling). Fail safe on malformed
   input; never panic. *New, but parallels the existing bounded parsers.*
3. **Rust: glyph pages → existing atlas.** Load the pack's `font/glyph_*.png`
   pages into `FontTexturePage` / `GlyphMetrics` and register the private-use
   codepoints into the live text pipeline. **Cheapest, highest-leverage slice —
   this is what makes Zeqa's scoreboard render** — because the atlas format
   already exists. *Reuse-heavy.*
4. **Rust: item textures.** Parse `textures/item_texture.json` and register
   custom item icons into the hotbar/inventory. *Medium; reuses `item.rs`.*
5. **Rust: block/terrain textures.** Feed the pack's `blocks.json` /
   `terrain_texture.json` / textures into the 3D block pipeline. *Largest lift;
   reuses `pack/mod.rs` resolution.*

## Open questions (resolve during design, before code)

1. **Relay behavior** — Does the current proxy already forward any resource-pack
   packets, or does the listener fully terminate the handshake? Pick the
   forwarding mechanism (serve-from-listener vs. side-channel bytes) accordingly.
2. **Caching** — Cache decrypted packs per server (keyed by pack UUID + version)
   to avoid re-downloading, or ingest fresh each session? Where does the cache
   live relative to the gitignored `.local/` dev inputs?
3. **Trust bounds** — Concrete ceilings for pack size, atlas dimensions, glyph
   count, and total registered bytes, and the fail-safe behavior when exceeded
   (skip the offending asset vs. reject the pack vs. disconnect).
4. **Provenance story** — How runtime, unpinned assets coexist with the
   fail-closed, SHA256-pinned vanilla contract in `AGENTS.md` without
   contradicting it. Likely: vanilla stays pinned/fail-closed; server packs are
   a separate, explicitly-untrusted, best-effort tier.
5. **Vanilla fallback** — When a server pack omits an asset, fall through to the
   pinned vanilla carrier.

## Non-goals

- Behavioral/gameplay pack content (scripts, entity behavior) — textures, glyphs,
  and UI atlases only.
- Client-authored or user-installed packs — server-supplied packs only.
- Weakening or bypassing the vanilla pinned-asset contract.

## Verification gates

- A live capture against a custom-content server (Zeqa) showing the scoreboard
  rendering real glyphs instead of tofu (step 3 acceptance).
- Bounded-input tests: oversized/malformed/decompression-bomb packs fail safe
  with no panic and no unbounded allocation.
- Vanilla-only servers regress in no way — no server pack means the existing
  pinned pipeline is used unchanged.
