# Packaging

Release installers ship the client, the Go core, the dragonfly local-world server beside it, the
Monocraft font and a prep kit, never Mojang-derived carriers. A packaged launch whose carriers are
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

Installers also ship the pinned OFL Monocraft font at `<resources>/fonts/`, fetched by `package-*`
via `scripts/fetch-ui-font.sh`, so first-run setup can draw before any download.

CI: `.github/workflows/package.yml`. A daily schedule at 03:17 UTC packages the default branch
(`dev`) and replaces the `nightly` prerelease (tag moved, assets replaced). Branch pushes do not
start packaging. To publish a stable release, run **Package** in GitHub Actions and choose
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
older pending run. Scheduled nightly runs do not cancel a running package job.

Every installer is extracted and checked by `packaging/check-payload.sh`, which fails on any file
outside the payload allowlist (shared with `stage-payload.sh`). Windows uses WiX extraction without
running installer actions. DMG creation retries the transient `Resource busy` error on hosted
macOS runners; other errors remain fatal.

Windows publishes `Cinnabar-x64-setup.exe` and `Cinnabar-x64.msi`. Setup is a self-contained
WiX Burn wizard embedding the same MSI; CI checks its embedded MSI hash against the separately
verified package. Both install to Program Files and retain the existing machine-wide MSI upgrade
identity, so Windows may request administrator approval. Setup, the installed executable, uninstall
entry, Start menu and desktop shortcuts use the original `packaging/icons/cinnabar.svg` logo.
The update manifest points to the MSI. It schedules old-version removal inside the
installation transaction so a failed upgrade reinstalls the previous version
([WiX upgrade scheduling](https://docs.firegiant.com/wix/schema/wxs/majorupgrade/)). No Mojang artwork is embedded. Local packaging requires
ImageMagick and `dotnet tool install --global wix --version 5.0.2`, followed by
`wix extension add -g WixToolset.BootstrapperApplications.wixext/5.0.2`.

Signing is optional; each missing secret yields unsigned output instead of a failure:

| Name (repo secret unless noted) | Enables |
| --- | --- |
| `MACOS_CERT_P12_BASE64`, `MACOS_CERT_PASSWORD`, `CODESIGN_IDENTITY` | Developer ID signing (else ad-hoc) |
| `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` | Notarization; required once `CODESIGN_IDENTITY` is set |
| `WINDOWS_CERT_PFX_BASE64`, `WINDOWS_CERT_PASSWORD` | Authenticode signing of the exes, MSI, setup engine and bundle |
| `CINNABAR_UPDATE_SIGNING_KEY` | Signed `update-stable.json` on `v*` releases (else none is published) |

Without `CODESIGN_IDENTITY` the macOS app is ad-hoc signed and not notarized. Gatekeeper then blocks
it on other Macs until the recipient runs `xattr -dr com.apple.quarantine /Applications/Cinnabar.app`
(Open Anyway in System Settings clears only the app, so first-run helpers may still be blocked).

## Release downloads and Linux installation

`packaging/release-assets.json` is the canonical list of downloadable release artifacts and
their repository. The `updater_assets` entries override installer downloads for automatic updates.
macOS publishes a `.app.tar.gz` for each architecture, containing the signed and, when configured,
notarized `Cinnabar.app`. Windows uses its MSI; Linux reuses AppImage. Publication requires
every installer, updater archive and the generated Linux install script before writing checksums. The signed update manifest, when present, is included
in those checksums. Replacing an existing stable release keeps it draft until upload completes.

Linux users can install the latest stable release without root:

```sh
curl -fsSL https://github.com/bedrock-mc/cinnabar/releases/latest/download/install.sh | sh
```

The installer resolves a concrete release tag, verifies the AppImage against its release checksum,
and adds a launcher, icon and application-menu entry. It uses the user's XDG data directory and
`~/.local/bin` by default; `CINNABAR_BIN_DIR` overrides the launcher directory. Failed downloads,
verification or extraction preserve the current installation. The launcher defaults to running
without FUSE. Pass `--channel nightly` or `--version TAG` when invoking the script directly to test
a particular release; these are explicit opt-ins to builds outside the stable channel.

Windows users download the self-contained setup EXE. macOS users choose the native Apple silicon
or Intel DMG. The website is deployed separately and links directly to GitHub's latest release.

## Sign-in
The core owns Xbox device-code auth. The client's `AuthState::AwaitingCode { uri, code }` exposes the
code and URL; no packaging-specific UI exists.

## Crash reports
A panic hook writes `crashes/*.json` (message, backtrace, core log tail) for local debugging; the newest
reports are kept and nothing is uploaded.

## Automatic updates

The client checks for a new stable version at most daily and downloads it in the background.
The launcher menu shows download progress, failures, and a ready notice with **Restart to update**
and a release notes link. Automatic updates can be disabled in Settings or with
`CINNABAR_UPDATE_CHECK=0`. Applying waits until the client exits; it never replaces a running
session. Restarting from the menu exits first and then relaunches the installed version.

The core authenticates the Ed25519-signed manifest before trusting its artifact URL, size, SHA-256,
version or release notes. It rejects unknown keys, expiry, the wrong channel and non-HTTPS URLs.
Downloaded bytes must match both the size and SHA-256 before an artifact becomes ready. Failed
or interrupted downloads are retry-safe and cannot replace the installation.

| Platform | Apply mechanism |
| --- | --- |
| macOS | Extract the verified app archive beside the installed `.app`, keep the old bundle as a backup, then replace it. |
| Windows | Run the verified MSI with `msiexec /i /passive /norestart` after the client exits; Windows Installer manages the upgrade transaction and rollback. Administrator approval may be required. |
| Linux | Stage the verified AppImage beside the installed file, keep the old file as a backup, then replace it. |

The parent installation directory must be writable on macOS and Linux. Launch a macOS app from
its installed location, not from the read-only DMG. Failed replacement preserves or restores the
previous installation. User settings, worlds and prepared assets remain in the per-user data
directory and are outside the replacement payload.

On macOS and Linux the retained backup is `<installed target>.previous`. To recover from a
release that installs but cannot launch, close Cinnabar, move the new target aside, and rename
that backup to the original target path. The next update replaces the older backup. Installer
diagnostics stay in the per-user `update/apply-log.txt`; failures also appear in the next launcher.

The feed is `CINNABAR_UPDATE_URL` or the bundled `resources/update-url`, pointing to
`update-stable.json` on the latest stable release. CI signs the immutable tag's artifact URLs and
release notes link. Signing is separate from platform code signing: without the manifest signing
secret, installers are still published but no automatic update is offered. Signing fails if the
secret does not match the public key committed for its key ID. Manifests expire after the
signer's default lifetime; if a stable release remains current beyond that lifetime, rerun its
tagged Package workflow to publish a fresh signed manifest. Clients never bypass expiry.

Nightly packages remain an explicit manual download or `install.sh --channel nightly` opt-in.
They currently share the source app version between releases, so no `update-nightly.json` is
published: semantic version comparison cannot distinguish successive nightly builds. Stable
updates never silently move an installation to nightly.

### One-time owner signing setup

The public trust list is `core/update/trusted-keys.txt`, embedded in the core. It is the only
source of trusted release keys; do not configure a separate GitHub public-key variable. The
repository intentionally ships without a private key. Until the owner installs a public key and
configures its matching secret, automatic updates cannot authenticate a release.

Run these commands yourself from the repository root. First generate a key pair:

```sh
go run ./core/cmd/release-manifest keygen
```

Save the private seed in your password manager. Set the GitHub secret interactively and paste
only the private base64 seed at the prompt (not the output label):

```sh
gh secret set CINNABAR_UPDATE_SIGNING_KEY
```

Write only the public base64 key to the trust list, with the signing command's default key ID:

```sh
printf '%s\n' 'k1:REPLACE_WITH_PUBLIC_BASE64_KEY' > core/update/trusted-keys.txt
git add core/update/trusted-keys.txt
git commit -m "build: trust the owner release signing key"
```

Replace `REPLACE_WITH_PUBLIC_BASE64_KEY` before running the command. Never put the private seed in
a source file, command argument, issue, release note or commit. Publish a new stable release
containing this public key through the normal release process; existing releases without the
trusted key need one manual upgrade. Future signed releases can then update automatically.

For rotation, first ship a release that trusts both the old and new public key IDs, signed by the
old key. Only after users have that trust list should the signing secret and signing key ID switch.
Removing the old key immediately would strand clients that have not yet received the new one.
