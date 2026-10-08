# Packaging

Release installers ship the client, the Go core, the dragonfly local-world server beside it, the
Cinnangles Sans font and a prep kit, never Mojang-derived carriers. A packaged launch whose carriers are
missing or stale runs `app/src/first_run`: a setup window (a `--first-run-setup` child process, or
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

All release targets build the client with `bedrock-client/local-mods`, enabling the local module
loader used by companion apps such as Cinnaroids.

Installers also ship the bundled Cinnangles Sans font at `<resources>/fonts/`, so first-run setup can
draw before any download.

CI: `.github/workflows/package.yml`. Pushes to `main` replace the `nightly` prerelease (tag moved,
assets replaced). To publish a stable release, run **Package** in GitHub Actions and choose
`current`, `patch`, `minor`, `major`, or `custom`. The **branch** input accepts any existing branch;
leave it blank to use the branch selected in Actions' **Use workflow from** menu. A version bump
is committed to that branch, leaving other branches unchanged. These are Cinnabar app versions,
independent of the Minecraft game/protocol version. `current` releases the
version in `[workspace.package]`; the other choices update that version and the inherited
workspace entries in `Cargo.lock`, commit the change, and create a matching `vX.Y.Z` tag. The
commit and tag are pushed together before building. Every platform builds the exact prepared
commit, and the final job publishes the installers as a stable release rather than a draft.
For `custom`, fill **version** with a greater plain semantic version such as `1.4.2` (no `v`
prefix or prerelease suffix). Patch increments the last component; minor and major reset lower
components to zero. Repository-wide release tags must be unique across branches.
Release tooling and logos come from the exact revision of the selected workflow; the target
branch is checked out separately for its source, binaries, manifests and assets. An older target
branch therefore needs no copy of the new release or setup scripts. Select a workflow revision
with these controls in **Use workflow from**, then enter the source branch in **branch**.

An unsuccessful build leaves its tag unpublished. Re-run the failed jobs, or select `current`
while that tag still points at the selected branch's HEAD, to finish it. Repeating the same custom
version also recovers an unpublished tag at HEAD. Manual preparation rejects
an existing release or a tag on a different commit for the version being released. Pushing a
matching `vX.Y.Z` tag by hand also builds and publishes it; re-running that tagged workflow
replaces its release assets. Stable runs do not cancel a running release; GitHub may replace an
older pending run. Main pushes cancel older nightly runs.

Every installer is extracted and checked by `packaging/check-payload.sh`, which fails on any file
outside the payload allowlist (shared with `stage-payload.sh`). Windows uses WiX extraction without
running installer actions. DMG creation retries the transient `Resource busy` error on hosted
macOS runners; other errors remain fatal.

Windows publishes `Cinnabar-x64-setup.exe` and `Cinnabar-x64.msi`. Setup is a self-contained
WiX Burn wizard embedding the same MSI; CI checks its embedded MSI hash against the separately
verified package. Both install to Program Files and retain the existing machine-wide MSI upgrade
identity, so Windows may request administrator approval. Setup, the installed executable, uninstall
entry, Start menu and desktop shortcuts use the original `packaging/icons/cinnabar.svg` logo.
The update manifest points to setup EXE. No Mojang artwork is embedded. Local packaging requires
ImageMagick and `dotnet tool install --global wix --version 5.0.2`, followed by
`wix extension add -g WixToolset.BootstrapperApplications.wixext/5.0.2`.

Signing is optional; each missing secret yields unsigned output instead of a failure:

| Name (repo secret unless noted) | Enables |
| --- | --- |
| `MACOS_CERT_P12_BASE64`, `MACOS_CERT_PASSWORD`, `CODESIGN_IDENTITY` | Developer ID signing (else ad-hoc) |
| `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` | Notarization; required once `CODESIGN_IDENTITY` is set |
| `WINDOWS_CERT_PFX_BASE64`, `WINDOWS_CERT_PASSWORD` | Authenticode signing of the exes, MSI, setup engine and bundle |
| `CINNABAR_UPDATE_SIGNING_KEY` | Signed `update-stable.json` on `v*` releases (else none is published) |
| `UPDATE_TRUSTED_KEYS` (variable) | Keys the core trusts for update manifests |
| `DISCORD_CHANGELOG_WEBHOOK` | Posts each new `v*` release's changelog to Discord, grouped by PR label (`packaging/changelog.py`) |

Without `CODESIGN_IDENTITY` the macOS app is ad-hoc signed and not notarized. Gatekeeper then blocks
it on other Macs until the recipient runs `xattr -dr com.apple.quarantine /Applications/Cinnabar.app`
(Open Anyway in System Settings clears only the app, so first-run helpers may still be blocked).

## Release downloads and Linux installation

`packaging/release-assets.json` is the canonical list of downloadable release artifacts and
their repository. Publication requires every platform installer and the generated
Linux install script before writing checksums. The signed update manifest, when present, is included
in those checksums. Replacing an existing stable release keeps it draft until upload completes.

Linux users can install the latest stable release without root:

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
`--channel nightly` or `--version TAG` when invoking the script directly to test a particular release; these are explicit opt-ins to builds outside the stable channel.

Windows users download the self-contained setup EXE. macOS users choose the native Apple silicon
or Intel DMG. The website is deployed separately and links directly to GitHub's latest release.

## Sign-in
The core owns Xbox device-code auth. The client's `AuthState::AwaitingCode { uri, code }` exposes the
code and URL; no packaging-specific UI exists.

## Crash reports
A panic hook writes `crashes/*.json` (message, backtrace, core log tail) for local debugging; the newest
reports are kept and nothing is uploaded.

## Update channel
`bedrock-core check-update` fetches an Ed25519-signed manifest (`core/update`), rejects unknown keys,
expiry, wrong channel, non-HTTPS, and bad digests, and the client records the verdict in
`update/available.json` at most daily. Manifest URL: `CINNABAR_UPDATE_URL` or `resources/update-url`.
Trusted keys are baked in with `-X main.trustedUpdateKeys=id:base64[,...]` (`UPDATE_TRUSTED_KEYS`),
so rotation ships as a new key ID. CI signs with `core/cmd/release-manifest` using
`CINNABAR_UPDATE_SIGNING_KEY`. Installing an update is manual (download the listed artifact).
