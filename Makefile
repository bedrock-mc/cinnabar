.DEFAULT_GOAL := help

CARGO ?= cargo
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

PACK_DIR ?= .local/assets/bedrock-samples/v1.26.30.32-preview/full/resource_pack
PACK_SENTINEL ?= $(PACK_DIR)/blocks.json
FONT_PACK_DIR ?= .local/assets/font-source
HUD_PACK_DIR ?= $(PACK_DIR)
UI_FONT_SOURCE_MANIFEST ?= assets/ui-font-source.json
UI_FONT_DIR ?= .local/assets/ui-font/e498bf70aeb25b4bdcff1e44d878fb2cb4f7c2a9
UI_FONT_SOURCE ?= $(UI_FONT_DIR)/Monocraft.ttf
BEDROCK_TARGET_MANIFEST ?= assets/bedrock-target.json
BLOCK_REGISTRY ?= crates/assets/data/block-registry-v2168.bin
LIGHT_REGISTRY ?= crates/assets/data/block-light-registry-v2168.bin
BIOME_REGISTRY ?= crates/assets/data/biome-registry-v2168.bin
REGISTRY_FOUNDATION_MANIFEST ?= assets/registry-foundation-v2168.json
PHYSICS_REGISTRY ?= .local/assets/block-physics-v2168.bin
PHYSICS_REGISTRY_SOURCE ?= crates/assets/data/block-physics-v2168.bin
PHYSICS_REGISTRY_SHA256 ?= crates/assets/data/block-physics-v2168.sha256
VANILLA_SOURCE_MANIFEST ?= assets/vanilla-source.json
ASSET_BLOB ?= .local/assets/compiled/vanilla-v2168.mcbea
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
HUD_SOURCE_MANIFEST ?= assets/hud-source-v1001.json
LANG_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbelang
LANG_ASSET_REPORT ?= .local/assets/compiled/lang-assets.json
AUDIO_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeaud
AUDIO_ASSET_REPORT ?= .local/assets/compiled/audio-assets.json
ICON_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeico
ICON_ASSET_REPORT ?= .local/assets/compiled/icon-assets.json
CINNABAR_CLOUDS_PNG ?=
CLOUDS_OVERRIDE_PREREQUISITE = FORCE_CINNABAR_CLOUDS_OVERRIDE
ASSET_COMPILER_INPUTS := Cargo.toml Cargo.lock $(BEDROCK_TARGET_MANIFEST) crates/assets/Cargo.toml crates/asset-compiler/Cargo.toml Makefile $(wildcard crates/assets/src/*.rs) $(wildcard crates/assets/src/*/*.rs) $(wildcard crates/asset-compiler/src/*.rs) $(wildcard crates/asset-compiler/src/*/*.rs) $(wildcard crates/asset-compiler/src/*/*/*.rs)
VANILLA_FETCH_INPUTS := scripts/fetch-vanilla-assets.ps1 scripts/fetch-vanilla-assets.sh
PHYSICS_REGISTRY_CHECK = $(GO) -C tools/registrygen run ./cmd/hashcheck -file "$(abspath $(PHYSICS_REGISTRY))" -sha256-file "$(abspath $(PHYSICS_REGISTRY_SHA256))"
REGISTRY_FOUNDATION_CHECK = $(GO) -C tools/registrygen run ./cmd/foundationcheck -manifest "$(abspath $(REGISTRY_FOUNDATION_MANIFEST))"
WORLD_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- compile --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --registry "$(BLOCK_REGISTRY)" --light-registry "$(LIGHT_REGISTRY)" --biome-registry "$(BIOME_REGISTRY)" --out "$(ASSET_BLOB)"
ATMOSPHERE_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- atmosphere --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" $(if $(strip $(CINNABAR_CLOUDS_PNG)),--clouds-override "$(CINNABAR_CLOUDS_PNG)") --out "$(ATMOSPHERE_BLOB)" --report "$(ATMOSPHERE_REPORT)"
ENTITY_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- entity-assets --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(ENTITY_ASSET_BLOB)" --report "$(ENTITY_ASSET_REPORT)"
FONT_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- outline-font-assets --font "$(UI_FONT_SOURCE)" --source-manifest "$(UI_FONT_SOURCE_MANIFEST)" --out "$(FONT_ASSET_BLOB)" --report "$(FONT_ASSET_REPORT)"
LOCAL_FONT_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- font-assets --pack "$(FONT_PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(LOCAL_FONT_ASSET_BLOB)" --report "$(LOCAL_FONT_ASSET_REPORT)"
HUD_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- hud-assets --pack "$(HUD_PACK_DIR)" --source-manifest "$(HUD_SOURCE_MANIFEST)" --out "$(HUD_ASSET_BLOB)" --report "$(HUD_ASSET_REPORT)"
LANG_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- lang-assets --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(LANG_ASSET_BLOB)" --report "$(LANG_ASSET_REPORT)"
AUDIO_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- audio-assets --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(AUDIO_ASSET_BLOB)" --report "$(AUDIO_ASSET_REPORT)"
ICON_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- icon-assets --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(ICON_ASSET_BLOB)" --report "$(ICON_ASSET_REPORT)"
CLIENT_RUN = RUST_MCBE_BUILD_COMMIT="$(RUST_MCBE_BUILD_COMMIT)" $(CARGO) run --release -p bedrock-client --locked -- --socket-dir "$(SOCKET_DIR)" $(if $(filter 1,$(NO_VSYNC)),--no-vsync)

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

.PHONY: help vanilla-assets assets atmosphere-assets entity-assets font-assets font-assets-local hud-assets hud-assets-local lang-assets audio-assets icon-assets physics-assets core client client-windows client-macos client-linux client-wayland client-x11 dist-local FORCE_CINNABAR_CLOUDS_OVERRIDE
.PHONY: registry-foundation-check

FORCE_CINNABAR_CLOUDS_OVERRIDE:

help:
	@echo make registry-foundation-check - Validate the exact protocol-2168 registry foundation
	@echo make vanilla-assets  - Acquire the pinned official Mojang sample resource pack
	@echo make assets          - Download and compile the vanilla resource pack
	@echo make atmosphere-assets - Compile pinned sun, moon, and cloud runtime assets
	@echo make entity-assets   - Compile pinned entity catalog and geometry payloads
	@echo make font-assets     - Fetch and compile the pinned open-licensed Monocraft UI font
	@echo make font-assets-local - Compile a reviewed local bitmap font source via FONT_PACK_DIR
	@echo make hud-assets      - Compile pinned HUD sprites from the official Mojang sample pack
	@echo make hud-assets-local - Compile from an explicitly selected matching pack via HUD_PACK_DIR
	@echo make audio-assets    - Compile the pinned sound-definition lookup catalog
	@echo make physics-assets  - Install and verify the pinned protocol-2168 physics registry
	@echo make core            - Compile and run the Go networking/auth core
	@echo make client          - Refresh stale assets, then run the release Rust client
	@echo make client-windows  - Run the client on Windows
	@echo make client-macos    - Run the client on macOS
	@echo make client-linux    - Run with automatic Wayland/X11 selection
	@echo make client-wayland  - Run on Wayland
	@echo make client-x11      - Run on X11/XWayland
	@echo make dist-local      - Stage an unsigned local-development-only bundle under .local/dist
	@echo UPSTREAM=host:port is required for make core
	@echo Override optional settings with SOCKET_DIR=..., AUTH_CACHE=..., and NO_VSYNC=1
	@echo Set CINNABAR_CLOUDS_PNG to the exact local-only Bedrock 1.26.33.1 clouds.png

registry-foundation-check:
	$(REGISTRY_FOUNDATION_CHECK)

vanilla-assets: $(PACK_SENTINEL)

assets: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT) $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT) $(FONT_ASSET_BLOB) $(FONT_ASSET_REPORT) $(HUD_ASSET_BLOB) $(HUD_ASSET_REPORT) $(LANG_ASSET_BLOB) $(LANG_ASSET_REPORT) $(ICON_ASSET_BLOB) $(ICON_ASSET_REPORT) $(AUDIO_ASSET_BLOB) $(AUDIO_ASSET_REPORT)

atmosphere-assets: $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT)

entity-assets: $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT)

font-assets: $(FONT_ASSET_BLOB) $(FONT_ASSET_REPORT)

font-assets-local:
	$(LOCAL_FONT_ASSET_COMPILE)

hud-assets: $(HUD_ASSET_BLOB) $(HUD_ASSET_REPORT)

hud-assets-local:
	$(HUD_ASSET_COMPILE)

lang-assets: $(LANG_ASSET_BLOB) $(LANG_ASSET_REPORT)

audio-assets: $(AUDIO_ASSET_BLOB) $(AUDIO_ASSET_REPORT)

icon-assets: $(ICON_ASSET_BLOB) $(ICON_ASSET_REPORT)

$(UI_FONT_SOURCE): $(UI_FONT_SOURCE_MANIFEST)
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

$(FONT_ASSET_BLOB): $(ASSET_COMPILER_INPUTS) $(UI_FONT_SOURCE_MANIFEST) $(UI_FONT_SOURCE)
	$(FONT_ASSET_COMPILE)

$(FONT_ASSET_REPORT): $(FONT_ASSET_BLOB)
	$(RUN_IF_ASSET_REPORT_STALE) || $(FONT_ASSET_COMPILE)

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

$(AUDIO_ASSET_BLOB): $(PACK_SENTINEL) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST)
	$(AUDIO_ASSET_COMPILE)

$(AUDIO_ASSET_REPORT): $(AUDIO_ASSET_BLOB)
	@if [ ! -f "$@" ] || [ "$@" -ot "$<" ]; then $(AUDIO_ASSET_COMPILE); fi

core:
	$(if $(strip $(UPSTREAM)),,$(error UPSTREAM is required; run make core UPSTREAM=host:port))
	@echo bedrock-core: build starting package=./core/cmd/bedrock-core
	$(GO) run ./core/cmd/bedrock-core -socket-dir "$(SOCKET_DIR)" -upstream "$(UPSTREAM)" -auth-cache "$(AUTH_CACHE)"

client: assets physics-assets
	$(CLIENT_RUN)

client-windows client-macos client-linux: client

client-wayland:
	env -u DISPLAY $(MAKE) client

client-x11:
	env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET $(MAKE) client

dist-local:
	$(CARGO) run --locked -p dist-local -- --platform "$(DIST_PLATFORM)" --client "$(DIST_CLIENT)" --core "$(DIST_CORE)" --assets "$(dir $(ASSET_BLOB))" --physics "$(PHYSICS_REGISTRY)" --notices "$(DIST_NOTICES)" --target "$(DIST_TARGET)" --git-commit "$(DIST_GIT_COMMIT)" --out "$(DIST_OUT)"
