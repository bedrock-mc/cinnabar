.DEFAULT_GOAL := help

CARGO ?= cargo
# Cargo profile for `make play`; PROFILE=release keeps developer features with shipping optimizations.
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

NO_VSYNC ?= 0
TRACY ?= 0
CLIENT_FEATURES = $(if $(filter 1,$(TRACY)),--features tracy)
# Passed to the client at launch only; it is never a compile input.
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
PACK_SENTINEL ?= $(PACK_DIR)/blocks.json
FONT_PACK_DIR ?= .local/assets/font-source
HUD_PACK_DIR ?= $(PACK_DIR)
FONT_SOURCE_MANIFEST ?= assets/cinnangles-sans-source.json
FONT_SOURCE ?= assets/fonts/CinnanglesSans.ttf
BEDROCK_TARGET_MANIFEST ?= assets/bedrock-target.json
REGISTRY_FOUNDATION_MANIFEST ?= assets/registry-foundation-v2193.json
PHYSICS_REGISTRY ?= .local/assets/block-physics-v2193.bin
PHYSICS_REGISTRY_SOURCE ?= crates/assets/data/block-physics-v2193.bin
PHYSICS_REGISTRY_SHA256 ?= crates/assets/data/block-physics-v2193.sha256
HUD_SOURCE_MANIFEST ?= assets/hud-source-v2193.json
# Development-only outputs of explicitly selected local packs; the carrier table owns the rest.
LOCAL_ASSET_DIR ?= .local/assets/compiled
CINNABAR_CLOUDS_PNG ?=
PHYSICS_REGISTRY_CHECK = $(GO) -C tools/registrygen run ./cmd/hashcheck -file "$(abspath $(PHYSICS_REGISTRY))" -sha256-file "$(abspath $(PHYSICS_REGISTRY_SHA256))"
REGISTRY_FOUNDATION_CHECK = $(GO) -C tools/registrygen run ./cmd/foundationcheck -manifest "$(abspath $(REGISTRY_FOUNDATION_MANIFEST))"
ASSETC = $(CARGO) run --profile $(PROFILE) --locked -p asset-compiler --bin assetc --
VANILLA_ASSET_FETCH = $(ASSETC) vanilla-pack --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --accept-eula
# One in-process parallel pass over the carrier table; carriers whose fingerprint is unchanged are skipped.
ASSETS_PREPARE = $(ASSETC) prepare --accept-eula $(if $(strip $(CINNABAR_CLOUDS_PNG)),--clouds-override "$(CINNABAR_CLOUDS_PNG)")
LOCAL_FONT_ASSET_COMPILE = $(ASSETC) font-assets --pack "$(FONT_PACK_DIR)" --source-manifest "$(VANILLA_SOURCE_MANIFEST)" --out "$(LOCAL_ASSET_DIR)/vanilla-v1.mcbefont" --report "$(LOCAL_ASSET_DIR)/font-assets.json"
LOCAL_HUD_ASSET_COMPILE = $(ASSETC) hud-assets --pack "$(HUD_PACK_DIR)" --source-manifest "$(HUD_SOURCE_MANIFEST)" --out "$(LOCAL_ASSET_DIR)/vanilla-v1.mcbehud" --report "$(LOCAL_ASSET_DIR)/hud-assets.json"

ifeq ($(OS),Windows_NT)
# PowerShell single-quoted literals escape an embedded apostrophe only by
# doubling it, so every path is quoted through this helper.
ps_literal = '$(subst ','',$(1))'
# Copy-Item keeps the source's write time; restamp the copy so make sees it as current.
PHYSICS_REGISTRY_INSTALL = $(POWERSHELL) -NoProfile -Command "New-Item -ItemType Directory -Force -Path $(call ps_literal,$(dir $(abspath $(PHYSICS_REGISTRY)))) | Out-Null; Copy-Item -Force $(call ps_literal,$(abspath $(PHYSICS_REGISTRY_SOURCE))) $(call ps_literal,$(abspath $(PHYSICS_REGISTRY))); (Get-Item -LiteralPath $(call ps_literal,$(abspath $(PHYSICS_REGISTRY)))).LastWriteTime = Get-Date"
else
PHYSICS_REGISTRY_INSTALL = mkdir -p "$(dir $(abspath $(PHYSICS_REGISTRY)))" && cp "$(abspath $(PHYSICS_REGISTRY_SOURCE))" "$(abspath $(PHYSICS_REGISTRY))"
endif

.PHONY: help vanilla-assets assets font-assets-local hud-assets-local physics-assets local-server play dist-local
.PHONY: registry-foundation-check jsonui-editor

help:
	@echo make registry-foundation-check - Validate the exact protocol-2193 registry foundation
	@echo make vanilla-assets  - Acquire the pinned official Mojang sample resource pack
	@echo make assets          - Build every stale carrier in one parallel pass, fetching the pack if needed
	@echo make NAME-assets     - Build one carrier and what it reads, e.g. hud-assets or audio-bank-assets
	@echo make font-assets-local - Compile a reviewed local bitmap font source via FONT_PACK_DIR
	@echo make hud-assets-local - Compile from an explicitly selected matching pack via HUD_PACK_DIR
	@echo make physics-assets  - Install and verify the pinned protocol-2193 physics registry
	@echo make local-server    - Build the dragonfly local-world server and experience-runtime beside the core binary
	@echo make play            - Refresh stale assets, build the core, and run the full game from the menu
	@echo make play TRACY=1    - Run with opt-in Tracy frame attribution
	@echo make dist-local      - Stage an unsigned local-development-only bundle under .local/dist
	@echo make jsonui-editor   - Build the static JSON-UI editor site into JSONUI_EDITOR_OUT
	@echo Override optional settings with NO_VSYNC=1
	@echo Set CINNABAR_CLOUDS_PNG to the exact local-only Bedrock 1.26.33.1 clouds.png

registry-foundation-check:
	$(REGISTRY_FOUNDATION_CHECK)

JSONUI_EDITOR_OUT ?= target/jsonui-editor-site

jsonui-editor: $(FONT_SOURCE)
	bash tools/jsonui-editor/build.sh "$(abspath $(JSONUI_EDITOR_OUT))" "$(abspath $(FONT_SOURCE))"
	@echo Serve it locally with: python3 -m http.server --directory $(JSONUI_EDITOR_OUT) 8000
	@echo then open http://localhost:8000/

vanilla-assets: $(PACK_SENTINEL)

assets:
	$(ASSETS_PREPARE)

# Any one carrier, plus the carriers it reads, by its carrier-table name.
%-assets: FORCE_PREPARE
	$(ASSETS_PREPARE) --only $*

FORCE_PREPARE:

font-assets-local:
	$(LOCAL_FONT_ASSET_COMPILE)

hud-assets-local:
	$(LOCAL_HUD_ASSET_COMPILE)

physics-assets: $(PHYSICS_REGISTRY)

# Installed and verified only when the pinned source, its hash or the target change.
$(PHYSICS_REGISTRY): $(PHYSICS_REGISTRY_SOURCE) $(PHYSICS_REGISTRY_SHA256) $(BEDROCK_TARGET_MANIFEST)
	$(PHYSICS_REGISTRY_INSTALL)
	$(PHYSICS_REGISTRY_CHECK)

$(PACK_SENTINEL): $(VANILLA_SOURCE_MANIFEST)
	$(VANILLA_ASSET_FETCH)

# A failed install or check leaves no target behind to look current.
.DELETE_ON_ERROR:

# Separate module: dragonfly needs a newer gophertunnel than the core, so it cannot join go.work.
LOCAL_SERVER_OUT ?= target/release/bedrock-local-server$(if $(filter windows,$(DIST_PLATFORM)),.exe)

local-server:
	cd tools/localserver && GOWORK=off $(GO) build -o "$(abspath $(LOCAL_SERVER_OUT))" .
	$(CARGO) build -p experience-runtime --release --locked

# Full game from the launcher menu: refresh assets, build the core and local server beside the client, run it.
play: assets physics-assets
ifeq ($(CINNABAR_DEV_SERVER_EXPERIENCES),1)
	$(CARGO) build --profile $(PROFILE) -p mod-host --bin mod-host --locked
	$(CARGO) build --profile $(PROFILE) -p mod-host --bin cinnabar-media-helper --features media --locked
endif
	$(GO) build -o "$(abspath target/$(PROFILE_DIR)/bedrock-core$(EXE))" ./core/cmd/bedrock-core
	-cd tools/localserver && GOWORK=off $(GO) build -o "$(abspath target/$(PROFILE_DIR)/bedrock-local-server$(EXE))" .
	RUST_MCBE_BUILD_COMMIT="$(RUST_MCBE_BUILD_COMMIT)" $(CARGO) run --profile $(PROFILE) -p bedrock-client --locked $(CLIENT_FEATURES) -- $(if $(filter 1,$(NO_VSYNC)),--no-vsync)

dist-local:
	$(CARGO) run --locked -p dist-local -- --platform "$(DIST_PLATFORM)" --client "$(DIST_CLIENT)" --core "$(DIST_CORE)" --physics "$(PHYSICS_REGISTRY)" --notices "$(DIST_NOTICES)" --target "$(DIST_TARGET)" --git-commit "$(DIST_GIT_COMMIT)" --out "$(DIST_OUT)"

# Release packaging (see packaging/README.md). Signing credentials come from the environment.
PKG_VERSION ?= $(shell sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p;}' Cargo.toml | head -n 1)
UPDATE_TRUSTED_KEYS ?=
PKG_CORE_LDFLAGS = -s -w -X main.releaseVersion=$(PKG_VERSION) -X main.trustedUpdateKeys=$(UPDATE_TRUSTED_KEYS)
.PHONY: package-binaries package-macos package-windows package-linux
package-binaries:
	$(CARGO) build --release --locked --no-default-features -p bedrock-client -p asset-compiler --features bedrock-client/local-mods --bin bedrock-client --bin assetc
	$(GO) build -trimpath -ldflags "$(PKG_CORE_LDFLAGS)" -o "$(DIST_CORE)" ./core/cmd/bedrock-core
	cd tools/localserver && GOWORK=off $(GO) build -trimpath -ldflags "-s -w" -o "$(abspath $(LOCAL_SERVER_OUT))" .

package-macos: package-binaries $(FONT_SOURCE)
	bash packaging/macos/build-app.sh
	bash packaging/macos/sign-notarize.sh .local/dist/macos-release/Cinnabar.app
	bash packaging/macos/make-dmg.sh .local/dist/macos-release/Cinnabar.app .local/dist/macos-release/Cinnabar-$(PKG_VERSION).dmg
	bash packaging/macos/sign-notarize.sh .local/dist/macos-release/Cinnabar-$(PKG_VERSION).dmg

package-windows: package-binaries $(FONT_SOURCE)
	$(POWERSHELL) -NoProfile -ExecutionPolicy Bypass -File packaging/windows/build-installer.ps1

package-linux: package-binaries $(FONT_SOURCE)
	bash packaging/linux/build-appimage.sh
