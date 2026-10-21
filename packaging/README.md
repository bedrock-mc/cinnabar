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
details: `logs/first-run.log`. Every launch logs to `logs/client.log`, including stderr and panics
whether or not a terminal is attached. Each launch archives the previous session as
`client-<UTC yyyymmdd-hhmmss>.log` (and `first-run.log` likewise); a session past 8 MiB continues
in a fresh file, and the newest 10 archives up to 64 MiB are kept.

| Target | Command | Signing (env only) |
| --- | --- | --- |
| macOS `.app` + DMG | `make package-macos` | `CODESIGN_IDENTITY`, `NOTARY_PROFILE` or `APPLE_ID`/`APPLE_TEAM_ID`/`APPLE_APP_PASSWORD` |
| Windows setup EXE + MSI (WiX v5) | `make package-windows` | `WINDOWS_CERT_PFX_BASE64`, `WINDOWS_CERT_PASSWORD` |
| Linux AppImage | `make package-linux` | none |
| Linux Flatpak (x86-64) | See [Flatpak](flatpak/README.md) | none |

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

Every desktop installer is extracted and checked by `packaging/check-payload.sh`, which fails on
any file outside the payload allowlist (shared with `stage-payload.sh`). Windows uses WiX extraction
without running installer actions. DMG creation retries the transient `Resource busy` error on hosted
macOS runners; other errors remain fatal.

Windows publishes `Cinnabar-x64-setup.exe` and `Cinnabar-x64.msi`. Setup is a self-contained
WiX Burn wizard embedding the same MSI; CI checks its embedded MSI hash against the separately
verified package. Both install to Program Files and retain the existing machine-wide MSI upgrade
identity, so Windows may request administrator approval. Setup, the installed executable, uninstall
entry, Start menu and desktop shortcuts use the original `packaging/icons/cinnabar.svg` logo.
The update manifest points to the MSI. No Mojang artwork is embedded. Local packaging requires
ImageMagick and `dotnet tool install --global wix --version 5.0.2`, followed by
`wix extension add -g WixToolset.BootstrapperApplications.wixext/5.0.2`.

The game, core, crash monitor and update helper do not request administrator rights or reject
an elevated launch. All packaged executables embed an `asInvoker` manifest before signing,
so they inherit the launching process's permissions. Installation may still require administrator
approval. Setup never launches the game, and Windows Installer's Restart Manager relaunch is
disabled; the update helper owns the client relaunch. Windows SDK `mt.exe` is required to embed
and verify these manifests during packaging.

Desktop signing and update-manifest signing are optional. Android release signing is required:
Android must recognize the same signing key to install later APKs over an existing installation.

| Name (repo secret unless noted) | Enables |
| --- | --- |
| `MACOS_CERT_P12_BASE64`, `MACOS_CERT_PASSWORD`, `CODESIGN_IDENTITY` | Developer ID signing (else ad-hoc) |
| `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` | Notarization; required once `CODESIGN_IDENTITY` is set |
| `WINDOWS_CERT_PFX_BASE64`, `WINDOWS_CERT_PASSWORD` | Authenticode signing of the exes, MSI, setup engine and bundle |
| `CINNABAR_UPDATE_SIGNING_KEY` (`release` environment secret, limited to `dev` and `v*` tags) | Each run's signed channel manifest, published where `update_channels` in `release-assets.json` names it (else skipped with a notice) |
| `CINNABAR_ANDROID_KEYSTORE_BASE64` (`release` environment secret) | Base64-encoded persistent Android release keystore; required for APK publication |
| `CINNABAR_ANDROID_KEY_ALIAS`, `CINNABAR_ANDROID_STORE_PASSWORD`, `CINNABAR_ANDROID_KEY_PASSWORD` (`release` environment secrets) | Alias and passwords for the Android release keystore; all required |
| `UPDATE_TRUSTED_KEYS` (variable) | Keys the client trusts for update manifests |
| `DISCORD_CHANGELOG_WEBHOOK` | Posts each new `v*` release's changelog to Discord, grouped by PR label (`packaging/changelog.py`) |

Without `CODESIGN_IDENTITY` the macOS app is ad-hoc signed and not notarized. Gatekeeper then blocks
it on other Macs until the recipient runs `xattr -dr com.apple.quarantine /Applications/Cinnabar.app`
(Open Anyway in System Settings clears only the app, so first-run helpers may still be blocked).

Keep a secure backup of the Android release keystore and its passwords, and reuse them for Beta
and nightly builds. The Android package job imports the keystore only for signing and deletes its
temporary copy even if the build fails. Missing signing secrets fail APK publication; the release
workflow never uses the local builder's generated test key. See [Android](../docs/android.md) for
device requirements and installation.

## Release downloads and Linux installation

`packaging/release-assets.json` is the canonical list of downloadable release artifacts and
their repository. `assets` lists desktop updater platforms; `manual_assets` lists downloads such
as Android APKs that are installed separately and have no desktop update payload or symbol archive.
Publication requires every platform installer, every desktop platform's symbol archive
and the generated Linux install script before writing checksums for the native downloads. A new tagged Beta release
stays draft until its downloads, update payloads and signed manifest are uploaded. Reruns keep
existing payload downloads public; the signature authenticates the update manifest.

Linux users can install the latest Beta release without root:

```sh
curl -fsSL https://github.com/cinnabar-mc/cinnabar/releases/latest/download/install.sh | sh
# or, without curl:
wget -qO- https://github.com/cinnabar-mc/cinnabar/releases/latest/download/install.sh | sh
```

The installer resolves a concrete release tag, verifies the AppImage against its release checksum,
and adds a launcher, icon and application-menu entry. It uses the user's XDG data directory and
`~/.local/bin` by default; `CINNABAR_BIN_DIR` overrides the launcher directory. Failed downloads,
verification or extraction preserve the current installation. The AppImage is unpacked once, without
FUSE or `/tmp`, into `<data>/cinnabar/app/<tag>-<digest>`; `app/current` names the build the
launcher runs and `app/previous` the one it replaced; each install keeps only those two. Pass
`--channel nightly` or `--version TAG` when invoking the script directly to test a particular release; these are explicit opt-ins to builds outside the Beta channel.

Windows users download the self-contained setup EXE. macOS users choose the native Apple silicon
or Intel DMG. The [website](https://github.com/cinnabar-mc/website) owns its deployment
and links directly to this repository's latest release. Its build reads the release
manifest, API contract and branding from a pinned client revision; shared inputs
remain in this repository.

## Release symbols

Native packaging enables line tables with Cargo profile environment overrides, including when
packaging an older source checkout. It keeps them in split debug files: a PDB per Windows
executable, a `.dSYM` per macOS binary, and on Linux a `.debug` file that
`packaging/release-symbols.py split` moves out of each binary after linking (the Linux job keeps
debug info at link time, since the linker's `--strip-debug` would also drop what a `.dwp` needs).
Installers ship only the stripped binaries: packagers stage binaries by name and `check-payload.sh`
rejects any other file. Each package job zips its Rust binaries with their debug files and a
`manifest.json` (version, commit, platform, and each binary's PE GUID+age, Mach-O UUID or ELF build
id) into `cinnabar-symbols-<version>-<platform>-<arch>.zip`, named by `release-assets.json`, and
the release carries it beside the installers. Go binaries (`bedrock-core`, `bedrock-local-server`)
print symbolized tracebacks themselves and have no symbol files. Android release APKs use the
`release` profile with split debug info disabled and have no symbol archive; the separate Android
CI workflow uses `play`. Releasing an older source branch without debug info publishes no archives
when run without those overrides, with a workflow warning; `release-symbols.py has-symbols` checks
the effective setting for both the package
and publish jobs.

To symbolicate a user's crash, ask for the crash folder (`crashes/<UTC time>-<id>/` under the
user data directory, or the `devtool diag` bundle that includes it), download the symbol
archive for the report's release, platform and architecture, and run offline:

```sh
cargo run -p devtool --locked -- symbolicate path/to/crash-folder --symbols cinnabar-symbols-1.2.3-windows-x86_64.zip
```

It prints the `report.json` summary, warns when the archive's version or commit differs from the
report, and walks every thread of `crash.dmp`, giving function, file and line for frames in a binary
whose build id matches the archive and module offsets elsewhere. A folder without a dump prints only
the summary; a bare `.dmp` path works too, and `--symbols` also accepts an extracted directory.
Nightly releases replace their symbol archives, so keep the archive for any nightly you need to debug.

## Sign-in
The core owns Xbox device-code auth. The client's `AuthState::AwaitingCode { uri, code }` exposes the
code and URL; no packaging-specific UI exists.

## Crash reports
The client starts a crash monitor first: the same executable re-run as `cinnabar --crash-monitor
<socket> <parent pid> <crash dir>`, which logs to `logs/crash-monitor.log` and exits with the
client. Its socket sits in a directory only the player can enter (`$XDG_RUNTIME_DIR/cinnabar` or
`cinnabar-<uid>` under the temporary directory; the user's temporary directory on Windows), and it
keeps only messages that carry the random token the client handed it on its stdin, along with
the client's arguments for Restart, which reruns the monitor's own executable (or the AppImage).
A panic or native fault only asks it for a minidump. A panic's folder is written as soon as its
dump arrives; if the client lives on, nothing is shown and a later crash gets its own folder.
A native fault's folder is written once the client is gone. Each folder,
`crashes/<UTC yyyymmdd-hhmmss>-<session id>/` under the user data directory, holds `crash.dmp`
(thread stacks, thread and module lists), `report.json` (schema 1: kind, message, thread,
backtrace, release, commit, build profile, OS, architecture, GPU and driver, uptime, session ID,
renderer attempt and game state — session phase, server kind without its address, dimension,
server pack count; native faults add a `native` object), plus `client.log` and `core.log` with
this session's part of each log, up to 1 MiB apiece, `packets.txt` (the last 4,096 packet
headers by name), `decode-samples/` (bodies of packets the session rejected, when any) and
`flight.log` (the last 2,000 lines of the log flight recorder, `debug` for network, session,
core and packs). The last three come from the session's `crashes/.live-<session id>/` staging
directory, whose rings are mapped files that outlive any crash; the crash window lists the last
packets in its details. When the client dies, the monitor opens the
crash window (Restart, Copy report, Open folder, Close, Join our Discord) for the fault, or for
the panic that ended it. A client that dies without a dump (a fail-fast abort such as
`0xC0000409`, an out-of-memory or external kill) gets a `kind: abort` report from its exit status;
a clean exit (code 0, or 130 after Ctrl+C) gets none. When `main` fails in a Windows GUI build,
the window shows the error: as `Cinnabar couldn't start` (`kind: startup`) before the game showed
its first frames, such as on a missing carrier, and as `Cinnabar stopped` (`kind: runtime`) after.
When the main loop finishes no frame for 10 s outside a world join, the monitor writes one
`kind: hang` folder per stall, with a minidump of every thread on Windows and Linux (macOS reports
say why they have none), and leaves the client running without a window; killing the hung client
then shows that folder instead of writing an abort report.
Without a GPU for the window the monitor uses a native dialog with the same actions. Until the
monitor is connected, or where it cannot run (Android), the client writes the folder itself, and
a monitor that connects later reports nothing more; folders from fatal Unix signals then end in
`-signal` and, written with async-signal-safe calls only, lack the three staging files. The
newest 8 folders are kept. `CINNABAR_CRASH_UI=0` suppresses the window and
`CINNABAR_FULL_DUMP=1` writes full-memory dumps (Windows). `devtool diag` bundles the latest
session's folders after redaction. Copy report replaces the home directory with `~`. Nothing is
uploaded.

A `bedrock-core` process that crashes while the client lives (a Go panic or fatal error, or an
exit other than 0 or its own logged failure status 1 that the client did not ask for) gets a
`kind: core` folder written by the client, with `core-crash.txt` holding the Go runtime's
panic and every goroutine's stack; no crash window opens, since the client shows its own
disconnect. Every core's stderr goes to `logs/core.log`, one file per client session, each line
labelled with the process role and pid.

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
