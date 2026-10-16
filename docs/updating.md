# Updates

The Cinnabar launcher (`cinnabar-launcher`, `crates/desktop-launcher`) owns updates; the game
never checks, downloads or installs them. The launcher checks its selected channel's signed
manifests when it opens and on request. It only downloads and installs after **Update** is
clicked, showing download, verification and installation as separate steps. Offline, a cached
offer whose signed manifest is still valid stays visible; the installed game always plays. Set
`CINNABAR_UPDATE_CHECK=0` to skip the launch-time check.

Installation needs the whole game tree closed. The game, its core-family processes, setup and
crash monitor hold the installation's runtime gate shared for as long as they run; the launcher
refuses to install while any of them holds it, and the helper waits for it before changing files.
A game started directly also holds the gate, and waits while an install is in progress.

The saved channel lives in the update directory's `preferences.json`, separately from the
installed payload's channel. The first launcher start migrates the game's old channel setting.
When a server refuses an outdated client, a game started by the launcher offers **Return to
launcher**, which exits with a typed status; the launcher then marks its offer as required and
checks again. An ordinary quit never installs anything.

Beta follows the latest tagged `vX.Y.Z` release. Nightly follows the rolling `nightly`
prerelease. Stable is reserved for a future release track and stays hidden until that track
publishes a signed release. Clients from before these channels require one manual install.
Changing channels never silently downgrades: a slower channel waits for a newer app version
and build. Choosing an older channel's release in the launcher downloads it for manual installation. Numeric
prereleases use SemVer ordering, and successive builds use a signed monotonic build number.

The client checks the manifest's Ed25519 signature, channel, expiry, app version and build,
then the artifact's size and SHA-256. The existing trusted-key list supports key rotation.
A separate Rust helper waits for the exact launcher process to exit and verifies the stage again.
A new version preparing game assets relays each setup progress report, so only a start that makes
no progress for five minutes counts as failed; time on setup's consent prompt never counts. The
helper then stops the client, its setup process and the asset compiler before restoring files.
Startup is acknowledged only after three primary-window frames have been presented and their GPU
work has completed. Main-world updates alone cannot confirm startup or discard the previous install.

- **macOS:** the helper replaces the complete signed app bundle from the DMG and preserves its
  signature and notarization ticket. It starts the new launcher, which continues into the game;
  the previous bundle remains beside it until the game completes startup. A failed new start
  restores the previous bundle.
- **Linux:** the same swap and startup rollback apply to AppImages. Unpacked installations use
  the manual download action.
- **Windows:** the per-machine install is not writable without elevation, so the launcher opens
  the verified MSI and quits; Windows Installer asks for approval and replaces the files.

If the install location is not writable, the launcher opens the verified DMG or installer for
you to finish and quits; on Linux it opens the folder containing the AppImage.
The updater does not request elevated privileges or reboot the machine.
Manual installers are copied out of the waiting stage before opening. A newer download or channel
change can replace that stage while the handed-off installer remains available. Manual copies use
the same lease and handoff retention window as copied update helpers.

The updater retains its verified offer while a later download replaces the cache. The helper
revalidates that offer and queries the installed executable's version and build under the apply
lease. If the installation is already at that build or newer, it skips the swap and relaunches.
Rollback stays inside the lease that performed the swap. Helper copies and stages remain retained
while either side of a handoff holds them.

`update_channels` in `packaging/release-assets.json` owns each published channel's track and
manifest filename for both the client and the package workflow. Beta reads
`releases/latest/download/update-beta.json`; the manifest and its payloads are attached to the
version tag before it becomes latest. Nightly reads the same files from its fixed prerelease.
Payloads use content-addressed names beside the manifest, separate from native installer downloads.
Publication uploads new payloads before the manifest and keeps the preceding publication's payloads.
Renewing an unchanged payload set preserves that grace period. Older version tags keep their assets.
There is no Stable publishing path or compatibility manifest. Publication
must sign the correct channel and a positive build number. The release signer accepts
`-build-number`; packaged builds use `CINNABAR_BUILD_NUMBER` or the package workflow's
`GITHUB_RUN_NUMBER`.

The Publish job declares the protected `release` environment. Its `CINNABAR_UPDATE_SIGNING_KEY`
is an environment secret limited to `dev` and `v*` tags; `UPDATE_TRUSTED_KEYS` is a repository
variable compiled into the clients. Both are configured; the first signed publication remains pending.
After the release is published, the Publish job posts its channel and version to the Cinnabar API's
`/v1/releases` with `CINNABAR_API_RELEASE_HOOK_SECRET`; a failed notice only warns.
