.DEFAULT_GOAL := help

CARGO ?= cargo
GO ?= go
POWERSHELL ?= powershell

SOCKET_DIR ?= .local/run-zeqa
AUTH_CACHE ?= .local/auth/microsoft-token.json
NO_VSYNC ?= 0

PACK_DIR ?= .local/assets/bedrock-samples/v1.26.30.32-preview/full/resource_pack
FONT_PACK_DIR ?= .local/assets/font-source
BLOCK_REGISTRY ?= crates/assets/data/block-registry-v1001.bin
LIGHT_REGISTRY ?= crates/assets/data/block-light-registry-v1001.bin
BIOME_REGISTRY ?= crates/assets/data/biome-registry-v1001.bin
VANILLA_SOURCE_MANIFEST ?= assets/vanilla-source.json
ASSET_BLOB ?= .local/assets/compiled/vanilla-v1001.mcbea
ATMOSPHERE_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeatm
ATMOSPHERE_REPORT ?= .local/assets/compiled/atmosphere-assets.json
ENTITY_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbeent
ENTITY_ASSET_REPORT ?= .local/assets/compiled/entity-assets.json
FONT_ASSET_BLOB ?= .local/assets/compiled/vanilla-v1.mcbefont
FONT_ASSET_REPORT ?= .local/assets/compiled/font-assets.json
CINNABAR_CLOUDS_PNG ?=
CLOUDS_OVERRIDE_PREREQUISITE = FORCE_CINNABAR_CLOUDS_OVERRIDE
ASSET_COMPILER_INPUTS := Cargo.toml Cargo.lock crates/assets/Cargo.toml crates/asset-compiler/Cargo.toml Makefile $(wildcard crates/assets/src/*.rs) $(wildcard crates/assets/src/*/*.rs) $(wildcard crates/asset-compiler/src/*.rs) $(wildcard crates/asset-compiler/src/*/*.rs) $(wildcard crates/asset-compiler/src/*/*/*.rs)
ATMOSPHERE_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- atmosphere --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" $(if $(strip $(CINNABAR_CLOUDS_PNG)),--clouds-override "$(CINNABAR_CLOUDS_PNG)") --out "$(ATMOSPHERE_BLOB)" --report "$(ATMOSPHERE_REPORT)"
ENTITY_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- entity-assets --pack "$(PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(ENTITY_ASSET_BLOB)" --report "$(ENTITY_ASSET_REPORT)"
FONT_ASSET_COMPILE = $(CARGO) run --locked -p asset-compiler --bin assetc -- font-assets --pack "$(FONT_PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(FONT_ASSET_BLOB)" --report "$(FONT_ASSET_REPORT)"

.PHONY: help assets atmosphere-assets entity-assets font-assets core client client-windows client-macos client-linux client-wayland client-x11 FORCE_CINNABAR_CLOUDS_OVERRIDE

FORCE_CINNABAR_CLOUDS_OVERRIDE:

help:
	@echo make assets          - Download and compile the vanilla resource pack
	@echo make atmosphere-assets - Compile pinned sun, moon, and cloud runtime assets
	@echo make entity-assets   - Compile pinned entity catalog and geometry payloads
	@echo make font-assets     - Compile a reviewed local bitmap font source via FONT_PACK_DIR
	@echo make core            - Compile and run the Go networking/auth core
	@echo make client          - Refresh stale assets, then run the release Rust client
	@echo make client-windows  - Run the client on Windows
	@echo make client-macos    - Run the client on macOS
	@echo make client-linux    - Run with automatic Wayland/X11 selection
	@echo make client-wayland  - Run on Wayland
	@echo make client-x11      - Run on X11/XWayland
	@echo UPSTREAM=host:port is required for make core
	@echo Override optional settings with SOCKET_DIR=..., AUTH_CACHE=..., and NO_VSYNC=1
	@echo Set CINNABAR_CLOUDS_PNG to the exact local-only Bedrock 1.26.33.1 clouds.png

assets: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT) $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT)

atmosphere-assets: $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT)

entity-assets: $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT)

font-assets: $(FONT_ASSET_BLOB) $(FONT_ASSET_REPORT)

$(ASSET_BLOB): $(ASSET_COMPILER_INPUTS) $(BLOCK_REGISTRY) $(LIGHT_REGISTRY) $(BIOME_REGISTRY)
ifeq ($(OS),Windows_NT)
	$(POWERSHELL) -NoProfile -ExecutionPolicy Bypass -File scripts/fetch-vanilla-assets.ps1 -AcceptEula
else
	bash scripts/fetch-vanilla-assets.sh --accept-eula
endif
	$(CARGO) run --locked -p asset-compiler --bin assetc -- compile --pack "$(PACK_DIR)" --registry "$(BLOCK_REGISTRY)" --light-registry "$(LIGHT_REGISTRY)" --biome-registry "$(BIOME_REGISTRY)" --out "$(ASSET_BLOB)"

$(ATMOSPHERE_BLOB): $(ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST) $(CLOUDS_OVERRIDE_PREREQUISITE)
	$(ATMOSPHERE_COMPILE)

$(ATMOSPHERE_REPORT): $(ATMOSPHERE_BLOB)
	@if [ ! -f "$@" ] || [ "$@" -ot "$<" ]; then $(ATMOSPHERE_COMPILE); fi

$(ENTITY_ASSET_BLOB): $(ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST)
	$(ENTITY_ASSET_COMPILE)

$(ENTITY_ASSET_REPORT): $(ENTITY_ASSET_BLOB)
	@if [ ! -f "$@" ] || [ "$@" -ot "$<" ]; then $(ENTITY_ASSET_COMPILE); fi

$(FONT_ASSET_BLOB): $(ASSET_BLOB) $(ASSET_COMPILER_INPUTS) $(VANILLA_SOURCE_MANIFEST)
	$(FONT_ASSET_COMPILE)

$(FONT_ASSET_REPORT): $(FONT_ASSET_BLOB)
	@if [ ! -f "$@" ] || [ "$@" -ot "$<" ]; then $(FONT_ASSET_COMPILE); fi

core:
	$(if $(strip $(UPSTREAM)),,$(error UPSTREAM is required; run make core UPSTREAM=host:port))
	@echo bedrock-core: build starting package=./core/cmd/bedrock-core
	$(GO) run ./core/cmd/bedrock-core -socket-dir "$(SOCKET_DIR)" -upstream "$(UPSTREAM)" -auth-cache "$(AUTH_CACHE)"

client: $(ASSET_BLOB) $(ATMOSPHERE_BLOB) $(ATMOSPHERE_REPORT) $(ENTITY_ASSET_BLOB) $(ENTITY_ASSET_REPORT)
	$(CARGO) run --release -p bedrock-client --locked -- --socket-dir "$(SOCKET_DIR)" $(if $(filter 1,$(NO_VSYNC)),--no-vsync)

client-windows client-macos client-linux: client

client-wayland:
	env -u DISPLAY $(MAKE) client

client-x11:
	env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET $(MAKE) client
