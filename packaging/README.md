# Packaging

Release installers ship the client, the Go core, the dragonfly local-world server and its
`cinnabar-cxb` verifier beside it, the Cinnangles Sans font and a prep kit, never Mojang-derived
carriers. A packaged launch whose carriers are
missing or stale runs `crates/first-run`: a setup window (a `--first-run-setup` child process, or
native dialogs if no window opens) asks consent, downloads the pinned `bedrock-samples` pack
(`assets/vanilla-source.json`, resumable, hash-verified), runs the bundled `assetc`, and publishes
carriers to the per-user data directory (`InstallLayout::prepared_assets_dir`). `prepared.json`
there records each carrier's input identity, so an update rebuilds only stale carriers; the current
pack archive is kept so carrier-only updates need no download. Status: `logs/first-run-status.json`;
details: `logs/first-run.log`. On macOS and Linux a launch without a terminal (e.g. from Finder)
sends the client's stderr to `logs/client.log`, rotated to `client.log.1` per launch and past 8 MiB.

| Target | Command | Signing (env only) |
| --- | --- | --- |
| macOS `.app` + DMG | `make package-macos` | `CODESIGN_IDENTITY`, `NOTARY_PROFILE` or `APPLE_ID`/`APPLE_TEAM_ID`/`APPLE_APP_PASSWORD` |
| Windows setup EXE + MSI (WiX v5) | `make package-windows` | `WINDOWS_CERT_PFX_BASE64`, `WINDOWS_CERT_PASSWORD` |
| Linux AppImage | `make package-linux` | none |

All native builds include the local mod host and Settings › Mods. Release targets use
`--no-default-features` to exclude the acceptance harness. Developer defaults stay enabled for `make play`, live
testing, CI and client MCP. A local `cargo build --release` still includes acceptance; use
`make package-binaries` for shipping.

`packaging/common/runtime-binaries.txt` owns the runtime executable list. Each row gives the
basename, Cargo package (`-` for Go integrations), and Unix path override variable. Release
builds, platform staging, signing, payload checks and `dist-local` use this table. Unix packagers
accept `BINARIES` for the build directory; otherwise helpers default beside the selected local
server, core or client. `CXB` overrides the verifier path. `dist-local --binaries <DIR>` selects
companion executables (by default beside `--client`), while `--client` and `--core` retain their
explicit paths. Missing runtime executables fail staging.

Installers also ship the bundled Cinnangles Sans font at `<resources>/fonts/`, so first-run setup can
draw before any download.

The prep kit at `<resources>/prep-kit/` holds `bin/assetc` and each file
`packaging/common/prep-kit-files.txt` lists, at its checkout path. `assetc kit-files` generates that
list; an asset-compiler test fails when it is stale, and `check-payload.sh` plans a first run with
the shipped compiler against the shipped kit.

CI: `.github/workflows/package.yml`. The daily schedule and manual `nightly` runs replace
its rolling prerelease from the default branch. To publish a tagged Beta release as the latest
GitHub release, run **Package** in GitHub Actions and choose
`current`, `patch`, `minor`, `major`, or `custom`. The **branch** input accepts any existing branch;
leave it blank to use the branch selected in Actions' **Use workflow from** menu. A version bump
is committed to that branch, leaving other branches unchanged. These are Cinnabar app versions,
independent of the Minecraft game/protocol version. `current` releases the
version in `[workspace.package]`; the other choices update that version and the inherited
workspace entries in `Cargo.lock`, commit the change, and create a matching `vX.Y.Z` tag. The
commit and tag are pushed together before building. Every platform builds the exact prepared
commit, and the final job publishes the installers and signed Beta manifest as the latest release.
For `custom`, fill **version** with a greater plain semantic version such as `1.4.2` (no `v`
prefix or prerelease suffix). Patch increments the last component; minor and major reset lower
components to zero. Repository-wide release tags must be unique across branches.
Release tooling and logos come from the exact revision of the selected workflow; the target
branch is checked out separately for its source, binaries, manifests and assets. An older target
branch therefore needs no copy of the new release or setup scripts. Select a workflow revision
with these controls in **Use workflow from**, then enter the source branch in **branch**.
Publication requires selecting `dev` or a `v*` tag as the workflow revision, so the Publish job
can use the protected `release` environment. The source **branch** input remains independent.

An unsuccessful build leaves its tag unpublished. Re-run the failed jobs, or select `current`
while that tag still points at the selected branch's HEAD, to finish it. Repeating the same custom
version also recovers an unpublished tag at HEAD. Manual preparation rejects
an existing release or a tag on a different commit for the version being released. Pushing a
matching `vX.Y.Z` tag by hand also builds and publishes it; re-running that tagged workflow
replaces its release assets. Tagged runs do not cancel a running release; GitHub may replace an
older pending run. Scheduled runs cancel older nightly runs.

Every installer is extracted and checked by `packaging/check-payload.sh`, which fails on any file
outside the payload allowlist (shared with `stage-payload.sh`). Windows uses WiX extraction without
running installer actions. DMG creation retries the transient `Resource busy` error on hosted
macOS runners; other errors remain fatal.

Windows publishes `Cinnabar-x64-setup.exe` and `Cinnabar-x64.msi`. Setup is a self-contained
WiX Burn wizard embedding the same MSI; CI checks its embedded MSI hash against the separately
verified package. Both install to Program Files and retain the existing machine-wide MSI upgrade
identity, so Windows may request administrator approval. Setup, the installed executable, uninstall
entry, Start menu and desktop shortcuts use the original `packaging/icons/cinnabar.svg` logo.
The update manifest points to the MSI. No Mojang artwork is embedded. Local packaging requires
ImageMagick and `dotnet tool install --global wix --version 5.0.2`, followed by
`wix extension add -g WixToolset.BootstrapperApplications.wixext/5.0.2`.

Signing is optional; each missing secret yields unsigned output instead of a failure:

| Name (repo secret unless noted) | Enables |
| --- | --- |
| `MACOS_CERT_P12_BASE64`, `MACOS_CERT_PASSWORD`, `CODESIGN_IDENTITY` | Developer ID signing (else ad-hoc) |
| `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` | Notarization; required once `CODESIGN_IDENTITY` is set |
| `WINDOWS_CERT_PFX_BASE64`, `WINDOWS_CERT_PASSWORD` | Authenticode signing of the exes, MSI, setup engine and bundle |
| `CINNABAR_UPDATE_SIGNING_KEY` (`release` environment secret, limited to `dev` and `v*` tags) | Each run's signed channel manifest, published where `update_channels` in `release-assets.json` names it (else skipped with a notice) |
| `UPDATE_TRUSTED_KEYS` (variable) | Keys the client trusts for update manifests |
| `DISCORD_CHANGELOG_WEBHOOK` | Posts each new `v*` release's changelog to Discord, grouped by PR label (`packaging/changelog.py`) |

Without `CODESIGN_IDENTITY` the macOS app is ad-hoc signed and not notarized. Gatekeeper then blocks
it on other Macs until the recipient runs `xattr -dr com.apple.quarantine /Applications/Cinnabar.app`
(Open Anyway in System Settings clears only the app, so first-run helpers may still be blocked).

## Release downloads and Linux installation

`packaging/release-assets.json` is the canonical list of downloadable release artifacts and
their repository. Publication requires every platform installer and the generated
Linux install script before writing checksums for the native downloads. A new tagged Beta release
stays draft until its downloads, update payloads and signed manifest are uploaded. Reruns keep
existing payload downloads public; the signature authenticates the update manifest.

Linux users can install the latest Beta release without root:

```sh
curl -fsSL https://github.com/bedrock-mc/cinnabar/releases/latest/download/install.sh | sh
# or, without curl:
wget -qO- https://github.com/bedrock-mc/cinnabar/releases/latest/download/install.sh | sh
```

The installer resolves a concrete release tag, verifies the AppImage against its release checksum,
and adds a launcher, icon and application-menu entry. It uses the user's XDG data directory and
`~/.local/bin` by default; `CINNABAR_BIN_DIR` overrides the launcher directory. Failed downloads,
verification or extraction preserve the current installation. The AppImage is unpacked once, without
FUSE or `/tmp`, into `<data>/cinnabar/app/<tag>-<digest>`; `app/current` names the build the
launcher runs and `app/previous` the one it replaced; each install keeps only those two. Pass
`--channel nightly` or `--version TAG` when invoking the script directly to test a particular release; these are explicit opt-ins to builds outside the Beta channel.

Windows users download the self-contained setup EXE. macOS users choose the native Apple silicon
or Intel DMG. The website is deployed separately and links directly to GitHub's latest release.

## Sign-in
The core owns Xbox device-code auth. The client's `AuthState::AwaitingCode { uri, code }` exposes the
code and URL; no packaging-specific UI exists.

## Crash reports
A panic hook writes `crashes/*.json` (message, backtrace, core log tail) for local debugging; the newest
reports are kept and nothing is uploaded.

## Update channel
The client checks its selected channel's fixed GitHub Releases manifest at most daily and
streams verified downloads to the user's update directory. It rejects unknown keys, expired
or wrong-channel manifests, non-HTTPS artifacts, corrupt payloads, and unintended downgrades.
Trusted keys (`id:base64[,...]`) are compiled into the client from `UPDATE_TRUSTED_KEYS`,
so rotation ships as a new key ID. The Publish job uses the `release` environment's
`CINNABAR_UPDATE_SIGNING_KEY`, available only to `dev` and `v*` tags, and
a positive `-build-number` matching the packaged client's publication number;
`release-manifest keygen` makes a key pair. Native payloads include the offline Rust helper.
See [Updates](../docs/updating.md) for consent, channel switching, and platform behavior.
