.DEFAULT_GOAL := help

CARGO ?= cargo
# Cargo profile for `make play`/`make client`; PROFILE=release gives the shipped build.
PROFILE ?= play
# Cargo names the dev profile output directory debug.
PROFILE_DIR = $(if $(filter dev,$(PROFILE)),debug,$(PROFILE))
EXE = $(if $(filter Windows_NT,$(OS)),.exe)
# Reuse compiled dependencies across worktrees when sccache is installed.
ifneq ($(shell command -v sccache 2>/dev/null),)
export RUSTC_WRAPPER ?= sccache
endif
GO ?= go
POWERSHELL ?= powershell

SOCKET_DIR ?= .local/run-zeqa
AUTH_CACHE ?= .local/auth/microsoft-token.json
NO_VSYNC ?= 0
RUST_MCBE_BUILD_COMMIT ?= $(shell git rev-parse HEAD)
DIST_PLATFORM ?= $(if $(filter Windows_NT,$(OS)),windows,$(if $(findstring Darwin,$(shell uname -s)),macos,linux))
DIST_CLIENT ?= target/release/$(if $(filter windows,$(DIST_PLATFORM)),bedrock-client.exe,bedrock-client)
DIST_CORE ?= target/release/$(if $(filter windows,$(DIST_PLATFORM)),bedrock-core.exe,bedrock-core)
DIST_OUT ?= .local/dist/$(DIST_PLATFORM)
DIST_TARGET ?= $(shell rustc --print host-tuple)
DIST_GIT_COMMIT ?= $(shell git rev-parse HEAD)
DIST_NOTICES ?= THIRD_PARTY_NOTICES.md

VANILLA_SOURCE_MANIFEST ?= assets/vanilla-source.json
# The pinned pack's extraction directory comes from the manifest, its one definition.
ifeq ($(OS),Windows_NT)
VANILLA_CACHE_DIR := $(shell $(POWERSHELL) -NoProfile -Command "(Get-Content -Raw '$(VANILLA_SOURCE_MANIFEST)' | ConvertFrom-Json).cache_dir")
else
VANILLA_CACHE_DIR := $(shell sed -n 's/^[[:space:]]*"cache_dir"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' $(VANILLA_SOURCE_MANIFEST))
endif
PACK_DIR ?= $(VANILLA_CACHE_DIR)/resource_pack
BEHAVIOR_PACK_DIR ?= $(patsubst %/resource_pack,%/behavior_pack,$(PACK_DIR))
PACK_SENTINEL ?= $(PACK_DIR)/blocks.json
FONT_PACK_DIR ?= .local/assets/font-source
HUD_PACK_DIR ?= $(PACK_DIR)
UI_FONT_SOURCE_MANIFEST ?= assets/ui-font-source.json
UI_FONT_DIR ?= .local/assets/ui-font/e498bf70aeb25b4bdcff1e44d878fb2cb4f7c2a9
UI_FONT_SOURCE ?= $(UI_FONT_DIR)/Monocraft.ttf
UI_FONT_FALLBACK_DIR ?= .local/assets/ui-font/f8d157532fbfaeda587e826d4cd5b21a49186f7c
UI_FONT_FALLBACK_SOURCE ?= $(UI_FONT_FALLBACK_DIR)/NotoSansCJKsc-Regular.otf
FONT_ASSET_NOTICES ?= $(dir $(FONT_ASSET_BLOB))ui-font-notices.txt
BEDROCK_TARGET_MANIFEST ?= assets/bedrock-target.json
BLOCK_REGISTRY ?= crates/assets/data/block-registry-v2193.bin
LIGHT_REGISTRY ?= crates/assets/data/block-light-registry-v2193.bin
BIOME_REGISTRY ?= crates/assets/data/biome-registry-v2193.bin
REGISTRY_FOUNDATION_MANIFEST ?= assets/registry-foundation-v2193.json
PHYSICS_REGISTRY ?= .local/assets/block-physics-v2193.bin
PHYSICS_REGISTRY_SOURCE ?= crates/assets/data/block-physics-v2193.bin
PHYSICS_REGISTRY_SHA256 ?= crates/assets/data/block-physics-v2193.sha256
ASSET_BLOB ?= .local/assets/compiled/vanilla-v2193.mcbea
ATMOSPHERE_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeatm
ATMOSPHERE_REPORT ?= .local/assets/compiled/atmosphere-assets.json
ENTITY_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeent
ENTITY_ASSET_REPORT ?= .local/assets/compiled/entity-assets.json
FONT_ASSET_BLOB ?= .local/assets/compiled/ui-monocraft-v1.mcbefont
FONT_ASSET_REPORT ?= .local/assets/compiled/ui-monocraft-font-assets.json
LOCAL_FONT_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbefont
LOCAL_FONT_ASSET_REPORT ?= .local/assets/compiled/font-assets.json
HUD_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbehud
HUD_ASSET_REPORT ?= .local/assets/compiled/hud-assets.json
HUD_SOURCE_MANIFEST ?= assets/hud-source-v2193.json
LANG_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbelang
LANG_ASSET_REPORT ?= .local/assets/compiled/lang-assets.json
LANGUAGE_ASSET_DIR ?= .local/assets/compiled/lang
LANGUAGE_ASSET_STAMP = $(LANGUAGE_ASSET_DIR)/.compiled
AUDIO_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeaud
AUDIO_ASSET_REPORT ?= .local/assets/compiled/audio-assets.json
AUDIO_BANK_BLOB ?= .local/assets/compiled/vanilla-v1.mcbesnd
AUDIO_BANK_REPORT ?= .local/assets/compiled/audio-bank.json
AUDIO_PCM_BLOB ?= .local/assets/compiled/vanilla-v1.mcbepcm
AUDIO_PCM_REPORT ?= .local/assets/compiled/audio-pcm-assets.json
ICON_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeico
ICON_ASSET_REPORT ?= .local/assets/compiled/icon-assets.json
ACTOR_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeact
ACTOR_ASSET_REPORT ?= .local/assets/compiled/actor-assets.json
EQUIPMENT_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeeqp
EQUIPMENT_ASSET_REPORT ?= .local/assets/compiled/equipment-assets.json
WEATHER_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbewth
HUD_EXTRAS_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbehxt
UI_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeui
UI_ASSET_REPORT ?= .local/assets/compiled/ui-assets.json
PARTICLE_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbept
PARTICLE_ASSET_REPORT ?= .local/assets/compiled/particle-assets.json
BLOCK_ENTITY_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeben
BLOCK_ENTITY_ASSET_REPORT ?= .local/assets/compiled/block-entity-assets.json
CINNABAR_CLOUDS_PNG ?=
CLOUDS_OVERRIDE_PREREQUISITE = FORCE_CINNABAR_CLOUDS_OVERRIDE
ASSET_COMPILER_INPUTS := Cargo.toml Cargo.lock $(BEDROCK_TARGET_MANIFEST) crates/assets/Cargo.toml crates/asset-compiler/Cargo.toml crates/pack-compiler/Cargo.toml Makefile $(wildcard crates/assets/data/*.json) $(wildcard crates/assets/src/*.rs) $(wildcard crates/assets/src/*/*.rs) $(wildcard crates/asset-compiler/src/*.rs) $(wildcard crates/asset-compiler/src/*/*.rs) $(wildcard crates/asset-compiler/src/*/*/*.rs) $(wildcard crates/pack-compiler/src/*.rs) $(wildcard crates/pack-compiler/src/*/*.rs) $(wildcard crates/pack-compiler/src/*/*/*.rs)
VANILLA_FETCH_INPUTS := scripts/fetch-vanilla-assets.ps1 scripts/fetch-vanilla-assets.sh
PHYSICS_REGISTRY_CHECK = $(GO) -C tools/registrygen run ./cmd/hashcheck -file "$(abspath $(PHYSICS_REGISTRY))" -sha256-file "$(abspath $(PHYSICS_REGISTRY_SHA256))"
REGISTRY_FOUNDATION_CHECK = $(GO) -C tools/registrygen run ./cmd/foundationcheck -manifest "$(abspath $(REGISTRY_FOUNDATION_MANIFEST))"
WORLD_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- compile --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --registry "$(BLOCK_REGISTRY)" --light-registry "$(LIGHT_REGISTRY)" --biome-registry "$(BIOME_REGISTRY)" --out "$(ASSET_BLOB)"
ATMOSPHERE_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- atmosphere --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" $(if $(strip $(CINNABAR_CLOUDS_PNG)),--clouds-override "$(CINNABAR_CLOUDS_PNG)") --out "$(ATMOSPHERE_BLOB)" --report "$(ATMOSPHERE_REPORT)"
ENTITY_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- entity-assets --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(ENTITY_ASSET_BLOB)" --report "$(ENTITY_ASSET_REPORT)"
FONT_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- outline-font-assets --font "$(UI_FONT_SOURCE)" --fallback-font "$(UI_FONT_FALLBACK_SOURCE)" --source-manifest "$(UI_FONT_SOURCE_MANIFEST)" --out "$(FONT_ASSET_BLOB)" --report "$(FONT_ASSET_REPORT)"
LOCAL_FONT_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- font-assets --pack "$(FONT_PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(LOCAL_FONT_ASSET_BLOB)" --report "$(LOCAL_FONT_ASSET_REPORT)"
HUD_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- hud-assets --pack "$(HUD_PACK_DIR)" --source-manifest "$(HUD_SOURCE_MANIFEST)" --out "$(HUD_ASSET_BLOB)" --report "$(HUD_ASSET_REPORT)"
LANG_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- lang-assets --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(LANG_ASSET_BLOB)" --report "$(LANG_ASSET_REPORT)"
LANGUAGE_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- language-assets --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out-dir "$(LANGUAGE_ASSET_DIR)"
AUDIO_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- audio-assets --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(AUDIO_ASSET_BLOB)" --report "$(AUDIO_ASSET_REPORT)"
AUDIO_BANK_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- audio-bank --pack "$(PACK_DIR)" --out "$(AUDIO_BANK_BLOB)" --report "$(AUDIO_BANK_REPORT)"
AUDIO_PCM_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- audio-pcm-assets --pack "$(PACK_DIR)" --catalog "$(AUDIO_ASSET_BLOB)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(AUDIO_PCM_BLOB)" --report "$(AUDIO_PCM_REPORT)"
ICON_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- icon-assets --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(ICON_ASSET_BLOB)" --report "$(ICON_ASSET_REPORT)" --block-assets "$(ASSET_BLOB)"
ACTOR_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- actor-assets --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(ACTOR_ASSET_BLOB)" --report "$(ACTOR_ASSET_REPORT)"
EQUIPMENT_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- equipment-assets --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(EQUIPMENT_ASSET_BLOB)" --report "$(EQUIPMENT_ASSET_REPORT)" $(if $(wildcard $(BEHAVIOR_PACK_DIR)/items),--behavior-pack "$(BEHAVIOR_PACK_DIR)")
BLOCK_ENTITY_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- block-entity-assets --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(BLOCK_ENTITY_ASSET_BLOB)" --report "$(BLOCK_ENTITY_ASSET_REPORT)"
UI_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- ui-assets --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(UI_ASSET_BLOB)" --report "$(UI_ASSET_REPORT)"
WEATHER_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- weather-assets --pack "$(PACK_DIR)" --out "$(WEATHER_ASSET_BLOB)"
HUD_EXTRAS_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- hud-extras-assets --pack "$(PACK_DIR)" --out "$(HUD_EXTRAS_ASSET_BLOB)"
PARTICLE_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- particle-assets --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(PARTICLE_ASSET_BLOB)" --report "$(PARTICLE_ASSET_REPORT)"
CLIENT_RUN = RUST_MCBE_BUILD_COMMIT="$(RUST_MCBE_BUILD_COMMIT)" $(CARGO) run --profile $(PROFILE) -p bedrock-client --locked -- --socket-dir "$(SOCKET_DIR)" $(if $(filter 1,$(NO_VSYNC)),--no-vsync)

ifeq ($(OS),Windows_NT)
VANILLA_ASSET_FETCH = $(POWERSHELL) -NoProfile -ExecutionPolicy Bypass -File scripts/fetch-vanilla-assets.ps1 -AcceptEula
RUN_IF_ASSET_REPORT_STALE = $(POWERSHELL) -NoProfile -ExecutionPolicy Bypass -File scripts/run-if-asset-report-stale.ps1 "$@" "$<"
else
VANILLA_ASSET_FETCH = bash scripts/fetch-vanilla-assets.sh --accept-eula
RUN_IF_ASSET_REPORT_STALE = bash scripts/run-if-asset-report-stale.sh "$@" "$<"
endif

ifeq ($(OS),Windows_NT)
# PowerShell single-quoted literals escape an embedded apostrophe only by
# doubling it, so every path is quoted through this helper.
ps_literal = '$(subst ','',$(1))'
PHYSICS_REGISTRY_INSTALL = $(POWERSHELL) -NoProfile -Command "New-Item -ItemType Directory -Force -Path $(call ps_literal,$(dir $(abspath $(PHYSICS_REGISTRY)))) | Out-Null; Copy-Item -Force $(call ps_literal,$(abspath $(PHYSICS_REGISTRY_SOURCE))) $(call ps_literal,$(abspath $(PHYSICS_REGISTRY)))"
else
PHYSICS_REGISTRY_INSTALL = mkdir -p "$(dir $(abspath $(PHYSICS_REGISTRY)))" && cp "$(abspath $(PHYSICS_REGISTRY_SOURCE))" "$(abspath $(PHYSICS_REGISTRY))"
endif

.PHONY: help vanilla-assets assets particle-assets atmosphere-assets entity-assets equipment-assets ui-assets block-entity-assets font-assets font-assets-local hud-assets hud-assets-local lang-assets language-assets audio-assets audio-bank icon-assets physics-assets core local-server client play client-windows client-macos client-linux client-wayland client-x11 dist-local FORCE_CINNABAR_CLOUDS_OVERRIDE
.PHONY: registry-foundation-check jsonui-editor

FORCE_CINNABAR_CLOUDS_OVERRIDE:

help:
	@echo make registry-foundation-check - Validate the exact protocol-2193 registry foundation
	@echo make vanilla-assets  - Acquire the pinned official Mojang sample resource pack
	@echo make assets          - Download and compile the vanilla resource pack
	@echo make atmosphere-assets - Compile pinned sun, moon, and cloud runtime assets
	@echo make entity-assets   - Compile pinned entity catalog and geometry payloads
	@echo make equipment-assets - Compile pinned attachable equipment bindings carrier
	@echo make particle-assets - Compile particle effect json and particle textures carrier
	@echo make ui-assets        - Pack pinned JSON-UI textures, sidecars, and raw ui json carrier
	@echo make block-entity-assets - Pack block-entity model textures and the pinned block-entity inventory
	@echo make font-assets     - Fetch and compile the pinned open-licensed Monocraft UI font
	@echo make font-assets-local - Compile a reviewed local bitmap font source via FONT_PACK_DIR
	@echo make hud-assets      - Compile pinned HUD sprites from the official Mojang sample pack
	@echo make hud-assets-local - Compile from an explicitly selected matching pack via HUD_PACK_DIR
	@echo make audio-assets    - Compile the pinned sound-definition lookup catalog
	@echo make audio-bank      - Pack sound routing and FSB sound files for playback
	@echo make physics-assets  - Install and verify the pinned protocol-2193 physics registry
	@echo make core            - Compile and run the Go networking/auth core
	@echo make local-server    - Build the dragonfly local-world server and experience-runtime beside the core binary
	@echo make play            - Refresh stale assets, build the core, and run the full game from the menu
	@echo make client          - Refresh stale assets, then join the core at SOCKET_DIR directly
	@echo make client-windows  - Run the client on Windows
	@echo make client-macos    - Run the client on macOS
	@echo make client-linux    - Run with automatic Wayland/X11 selection
	@echo make client-wayland  - Run on Wayland
	@echo make client-x11      - Run on X11/XWayland
	@echo make dist-local      - Stage an unsigned local-development-only bundle under .local/dist
	@echo make jsonui-editor   - Build the static JSON-UI editor site into JSONUI_EDITOR_OUT
	@echo UPSTREAM=host:port is required for make core
	@echo Override optional settings with SOCKET_DIR=..., AUTH_CACHE=..., and NO_VSYNC=1
	@echo Set CINNABAR_CLOUDS_PNG to the exact local-only Bedrock 1.26.33.1 clouds.png

registry-foundation-check:
	$(REGISTRY_FOUNDATION_CHECK)

JSONUI_EDITOR_OUT ?= target/jsonui-editor-site

jsonui-editor: $(UI_FONT_SOURCE) $(UI_FONT_DIR)/LICENSE
	bash tools/jsonui-editor/build.sh "$(abspath $(JSONUI_EDITOR_OUT))" "$(abspath $(UI_FONT_SOURCE))"
	@echo Serve it locally with: python3 -m http.server --directory $(JSONUI_EDITOR_OUT) 8000
	@echo then open http://localhost:8000/

vanilla-assets: $(PACK_SENTINEL)

assets: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT) $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT) $(FONT_ASSET_BLOB) $(FONT_ASSET_REPORT) $(FONT_ASSET_NOTICES) $(HUD_ASSET_BLOB) $(HUD_ASSET_REPORT) $(LANG_ASSET_BLOB) $(LANG_ASSET_REPORT) $(ICON_ASSET_BLOB) $(ICON_ASSET_REPORT) $(AUDIO_ASSET_BLOB) $(AUDIO_ASSET_REPORT) $(AUDIO_BANK_BLOB) $(AUDIO_BANK_REPORT)
assets: $(ACTOR_ASSET_BLOB) $(ACTOR_ASSET_REPORT)
assets: $(EQUIPMENT_ASSET_BLOB) $(EQUIPMENT_ASSET_REPORT)
assets: $(UI_ASSET_BLOB) $(UI_ASSET_REPORT)
assets: $(WEATHER_ASSET_BLOB)
assets: $(LANGUAGE_ASSET_STAMP)
assets: $(HUD_EXTRAS_ASSET_BLOB)
assets: $(PARTICLE_ASSET_BLOB) $(PARTICLE_ASSET_REPORT)
assets: $(BLOCK_ENTITY_ASSET_BLOB) $(BLOCK_ENTITY_ASSET_REPORT)
.PHONY: weather-assets
weather-assets: $(WEATHER_ASSET_BLOB)
$(WEATHER_ASSET_BLOB): $(PACK_SENTINEL) $(ASSET_COMPILER_INPUTS)
	$(WEATHER_ASSET_COMPILE)
.PHONY: hud-extras-assets
hud-extras-assets: $(HUD_EXTRAS_ASSET_BLOB)
$(HUD_EXTRAS_ASSET_BLOB): $(PACK_SENTINEL) $(ASSET_COMPILER_INPUTS)
	$(HUD_EXTRAS_ASSET_COMPILE)
.PHONY: actor-assets
actor-assets: $(ACTOR_ASSET_BLOB) $(ACTOR_ASSET_REPORT)
$(ACTOR_ASSET_BLOB): $(ENTITY_ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST) crates/assets/data/neutral-actor-materials-v1.json
	$(ACTOR_ASSET_COMPILE)
$(ACTOR_ASSET_REPORT): $(ACTOR_ASSET_BLOB)
	$(RUN_IF_ASSET_REPORT_STALE) || $(ACTOR_ASSET_COMPILE)

.PHONY: equipment-assets
equipment-assets: $(EQUIPMENT_ASSET_BLOB) $(EQUIPMENT_ASSET_REPORT)
$(EQUIPMENT_ASSET_BLOB): $(ENTITY_ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST)
	$(EQUIPMENT_ASSET_COMPILE)
$(EQUIPMENT_ASSET_REPORT): $(EQUIPMENT_ASSET_BLOB)
	$(RUN_IF_ASSET_REPORT_STALE) || $(EQUIPMENT_ASSET_COMPILE)

.PHONY: particle-assets
particle-assets: $(PARTICLE_ASSET_BLOB) $(PARTICLE_ASSET_REPORT)
$(PARTICLE_ASSET_BLOB): $(PACK_SENTINEL) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST)
	$(PARTICLE_ASSET_COMPILE)
$(PARTICLE_ASSET_REPORT): $(PARTICLE_ASSET_BLOB)
	$(RUN_IF_ASSET_REPORT_STALE) || $(PARTICLE_ASSET_COMPILE)

.PHONY: block-entity-assets
block-entity-assets: $(BLOCK_ENTITY_ASSET_BLOB) $(BLOCK_ENTITY_ASSET_REPORT)
$(BLOCK_ENTITY_ASSET_BLOB): $(PACK_SENTINEL) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST)
	$(BLOCK_ENTITY_ASSET_COMPILE)
$(BLOCK_ENTITY_ASSET_REPORT): $(BLOCK_ENTITY_ASSET_BLOB)
	$(RUN_IF_ASSET_REPORT_STALE) || $(BLOCK_ENTITY_ASSET_COMPILE)

.PHONY: ui-assets
ui-assets: $(UI_ASSET_BLOB) $(UI_ASSET_REPORT)
$(UI_ASSET_BLOB): $(PACK_SENTINEL) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST)
	$(UI_ASSET_COMPILE)
$(UI_ASSET_REPORT): $(UI_ASSET_BLOB)
	$(RUN_IF_ASSET_REPORT_STALE) || $(UI_ASSET_COMPILE)

atmosphere-assets: $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT)

entity-assets: $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT)

font-assets: $(FONT_ASSET_BLOB) $(FONT_ASSET_REPORT) $(FONT_ASSET_NOTICES)

font-assets-local:
	$(LOCAL_FONT_ASSET_COMPILE)

hud-assets: $(HUD_ASSET_BLOB) $(HUD_ASSET_REPORT)

hud-assets-local:
	$(HUD_ASSET_COMPILE)

lang-assets: $(LANG_ASSET_BLOB) $(LANG_ASSET_REPORT)

# Optional: other languages; the client falls back to en_US without them.
language-assets: $(LANGUAGE_ASSET_STAMP)

audio-assets: $(AUDIO_ASSET_BLOB) $(AUDIO_ASSET_REPORT)

audio-bank: $(AUDIO_BANK_BLOB) $(AUDIO_BANK_REPORT)

# Explicit opt-in finite predecode; not part of startup or playback activation.
.PHONY: audio-pcm-assets
audio-pcm-assets: $(AUDIO_PCM_BLOB) $(AUDIO_PCM_REPORT)

icon-assets: $(ICON_ASSET_BLOB) $(ICON_ASSET_REPORT)

$(UI_FONT_SOURCE) $(UI_FONT_FALLBACK_SOURCE) $(UI_FONT_DIR)/LICENSE $(UI_FONT_FALLBACK_DIR)/LICENSE: $(UI_FONT_SOURCE_MANIFEST)
ifeq ($(OS),Windows_NT)
	$(POWERSHELL) -NoProfile -ExecutionPolicy Bypass -File scripts/fetch-ui-font.ps1
else
	bash scripts/fetch-ui-font.sh
endif

physics-assets: $(PHYSICS_REGISTRY)
	$(PHYSICS_REGISTRY_CHECK) || ( $(PHYSICS_REGISTRY_INSTALL) && $(PHYSICS_REGISTRY_CHECK) )

$(PHYSICS_REGISTRY): $(PHYSICS_REGISTRY_SOURCE) $(PHYSICS_REGISTRY_SHA256) $(BEDROCK_TARGET_MANIFEST)
	$(PHYSICS_REGISTRY_INSTALL)


$(PACK_SENTINEL): $(VANILLA_SOURCE_MANIFEST) | $(VANILLA_FETCH_INPUTS)
	$(VANILLA_ASSET_FETCH)

$(ASSET_BLOB): $(PACK_SENTINEL) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST) $(BLOCK_REGISTRY) $(LIGHT_REGISTRY) $(BIOME_REGISTRY)
	$(WORLD_ASSET_COMPILE)

$(ATMOSPHERE_BLOB): $(ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST) $(CLOUDS_OVERRIDE_PREREQUISITE)
	$(ATMOSPHERE_COMPILE)

$(ATMOSPHERE_REPORT): $(ATMOSPHERE_BLOB)
	$(RUN_IF_ASSET_REPORT_STALE) || $(ATMOSPHERE_COMPILE)

$(ENTITY_ASSET_BLOB): $(ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST)
	$(ENTITY_ASSET_COMPILE)

$(ENTITY_ASSET_REPORT): $(ENTITY_ASSET_BLOB)
	$(RUN_IF_ASSET_REPORT_STALE) || $(ENTITY_ASSET_COMPILE)

$(FONT_ASSET_BLOB): $(ASSET_COMPILER_INPUTS) $(UI_FONT_SOURCE_MANIFEST) $(UI_FONT_SOURCE) $(UI_FONT_FALLBACK_SOURCE) $(UI_FONT_DIR)/LICENSE $(UI_FONT_FALLBACK_DIR)/LICENSE
	$(FONT_ASSET_COMPILE)

$(FONT_ASSET_REPORT): $(FONT_ASSET_BLOB)
	$(RUN_IF_ASSET_REPORT_STALE) || $(FONT_ASSET_COMPILE)

# Notices are published before the carrier. Their earlier timestamp is valid;
# only their absence needs recovery after the carrier has been checked/rebuilt.
$(FONT_ASSET_NOTICES): | $(FONT_ASSET_BLOB)
	$(FONT_ASSET_COMPILE)

$(HUD_ASSET_BLOB): $(ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(HUD_SOURCE_MANIFEST)
	$(HUD_ASSET_COMPILE)

$(HUD_ASSET_REPORT): $(HUD_ASSET_BLOB)
	$(RUN_IF_ASSET_REPORT_STALE) || $(HUD_ASSET_COMPILE)

$(LANG_ASSET_BLOB): $(ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST)
	$(LANG_ASSET_COMPILE)

$(ICON_ASSET_BLOB): $(ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST)
	$(ICON_ASSET_COMPILE)

$(ICON_ASSET_REPORT): $(ICON_ASSET_BLOB)
	@if [ ! -f "$@" ] || [ "$@" -ot "$<" ]; then $(ICON_ASSET_COMPILE); fi

$(LANG_ASSET_REPORT): $(LANG_ASSET_BLOB)
	@if [ ! -f "$@" ] || [ "$@" -ot "$<" ]; then $(LANG_ASSET_COMPILE); fi

$(LANGUAGE_ASSET_STAMP): $(ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST)
	$(LANGUAGE_ASSET_COMPILE)

$(AUDIO_ASSET_BLOB): $(PACK_SENTINEL) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST)
	$(AUDIO_ASSET_COMPILE)

$(AUDIO_ASSET_REPORT): $(AUDIO_ASSET_BLOB)
	@if [ ! -f "$@" ] || [ "$@" -ot "$<" ]; then $(AUDIO_ASSET_COMPILE); fi

$(AUDIO_BANK_BLOB): $(PACK_SENTINEL) $(ASSET_COMPILER_INPUTS)
	$(AUDIO_BANK_COMPILE)

$(AUDIO_BANK_REPORT): $(AUDIO_BANK_BLOB)
	@if [ ! -f "$@" ] || [ "$@" -ot "$<" ]; then $(AUDIO_BANK_COMPILE); fi

$(AUDIO_PCM_BLOB): $(AUDIO_ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST) $(PACK_DIR)/sounds/ambient/underwater/loop/underwater_ambience.fsb
	$(AUDIO_PCM_COMPILE)

$(AUDIO_PCM_REPORT): $(AUDIO_PCM_BLOB)
	@if [ ! -f "$@" ] || [ "$@" -ot "$<" ]; then $(AUDIO_PCM_COMPILE); fi

core:
	$(if $(strip $(UPSTREAM)),,$(error UPSTREAM is required; run make core UPSTREAM=host:port))
	@echo bedrock-core: build starting package=./core/cmd/bedrock-core
	$(GO) run ./core/cmd/bedrock-core -socket-dir "$(SOCKET_DIR)" -upstream "$(UPSTREAM)" -auth-cache "$(AUTH_CACHE)"

# Separate module: dragonfly needs a newer gophertunnel than the core, so it cannot join go.work.
LOCAL_SERVER_OUT ?= target/release/bedrock-local-server$(if $(filter windows,$(DIST_PLATFORM)),.exe)

local-server:
	cd tools/localserver && GOWORK=off $(GO) build -o "$(abspath $(LOCAL_SERVER_OUT))" .
	$(CARGO) build -p experience-runtime --release --locked

client: assets physics-assets
	$(CLIENT_RUN)

# Full game from the launcher menu: refresh assets, build the core and local server beside the client, run it.
play: assets physics-assets audio-pcm-assets
ifeq ($(CINNABAR_DEV_SERVER_EXPERIENCES),1)
	$(CARGO) build --profile $(PROFILE) -p mod-host --bin mod-host --locked
endif
	$(GO) build -o "$(abspath target/$(PROFILE_DIR)/bedrock-core$(EXE))" ./core/cmd/bedrock-core
	-cd tools/localserver && GOWORK=off $(GO) build -o "$(abspath target/$(PROFILE_DIR)/bedrock-local-server$(EXE))" .
	RUST_MCBE_BUILD_COMMIT="$(RUST_MCBE_BUILD_COMMIT)" $(CARGO) run --profile $(PROFILE) -p bedrock-client --locked -- $(if $(filter 1,$(NO_VSYNC)),--no-vsync)

client-windows client-macos client-linux: client

client-wayland:
	env -u DISPLAY $(MAKE) client

client-x11:
	env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET $(MAKE) client

dist-local:
	$(CARGO) run --locked -p dist-local -- --platform "$(DIST_PLATFORM)" --client "$(DIST_CLIENT)" --core "$(DIST_CORE)" --assets "$(dir $(ASSET_BLOB))" --physics "$(PHYSICS_REGISTRY)" --notices "$(DIST_NOTICES)" --target "$(DIST_TARGET)" --git-commit "$(DIST_GIT_COMMIT)" --out "$(DIST_OUT)"

# Release packaging (see packaging/README.md). Signing credentials come from the environment.
PKG_VERSION ?= $(shell sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p;}' Cargo.toml | head -n 1)
PKG_CORE_LDFLAGS = -s -w -X main.releaseVersion=$(PKG_VERSION)
.PHONY: package-binaries package-macos package-windows package-linux
package-binaries:
	$(CARGO) build --release --locked -p bedrock-client -p asset-compiler --bin bedrock-client --bin assetc
	$(GO) build -trimpath -ldflags "$(PKG_CORE_LDFLAGS)" -o "$(DIST_CORE)" ./core/cmd/bedrock-core
	cd tools/localserver && GOWORK=off $(GO) build -trimpath -ldflags "-s -w" -o "$(abspath $(LOCAL_SERVER_OUT))" .

package-macos: package-binaries $(UI_FONT_SOURCE)
	bash packaging/macos/build-app.sh
	bash packaging/macos/sign-notarize.sh .local/dist/macos-release/Cinnabar.app
	bash packaging/macos/make-dmg.sh .local/dist/macos-release/Cinnabar.app .local/dist/macos-release/Cinnabar-$(PKG_VERSION).dmg
	bash packaging/macos/sign-notarize.sh .local/dist/macos-release/Cinnabar-$(PKG_VERSION).dmg

package-windows: package-binaries $(UI_FONT_SOURCE)
	$(POWERSHELL) -NoProfile -ExecutionPolicy Bypass -File packaging/windows/build-installer.ps1

package-linux: package-binaries $(UI_FONT_SOURCE)
	bash packaging/linux/build-appimage.sh
