# Updates

The launcher checks its selected release channel at every launch, then every six hours, and
downloads verified updates in the background. A [Cinnabar API](cinnabar-api.md) release notice for
the selected channel starts a check at once; the check still verifies the signed manifest. An interrupted transfer resumes from its digest-named partial file, or
restarts safely if the server does not support ranges.

Only one running client per install handles updates; other windows stay passive until they take over on a due check tick.

When the download is ready, a compact card in the home screen's top-right corner shows the
release version and **Restart and update** button. The card stays outside the Play panel, and
installation only starts when you choose the button. An update required to connect uses stronger
wording. Periodic checks continue, and a newer release replaces the waiting one. In
**Settings → General → Updates**, you can select a channel, turn off automatic checks, or enable
**Install updates when I quit**. The section shows the installed version and build, the current check
or download status, and a **Check now** button. Check now always fetches fresh release metadata, even
when automatic checks are off. It is available from the home menu while no check is running.
Installation on quit is off by default. The in-game pause menu only shows a small ready badge;
update actions remain in the launcher.

Mods may independently opt into [GitHub release notifications](modding-spike.md#optional-github-update-checks).
These launch-time checks have a separate **Check for mod updates** setting. A mod
update uses the same corner card and links to its GitHub release for manual installation.

Beta follows the latest tagged `vX.Y.Z` release. Nightly follows the rolling `nightly`
prerelease. Stable is reserved for a future release track and stays hidden until that track
publishes a signed release. Clients from before these channels require one manual install.
Changing channels never silently downgrades: a slower channel waits for a newer app version
and build. **Switch now** downloads the selected channel's older release for manual installation. Numeric
prereleases use SemVer ordering, and successive builds use a signed monotonic build number.

The client checks the manifest's Ed25519 signature, channel, expiry, app version and build,
then the artifact's size and SHA-256. The existing trusted-key list supports key rotation.
A separate Rust helper waits for the exact client process to exit and verifies the stage again.
A new version preparing game assets relays each setup progress report, so only a start that makes
no progress for five minutes counts as failed; time on setup's consent prompt never counts. The
helper then stops the client, its setup process and the asset compiler before restoring files.
Startup is acknowledged only after three primary-window frames have been presented and their GPU
work has completed. Main-world updates alone cannot confirm startup or discard the previous install.

- **macOS:** the helper replaces the complete signed app bundle from the DMG and preserves its
  signature and notarization ticket. The previous bundle remains beside it until the new client
  completes startup. A failed new start restores the previous bundle.
- **Linux:** the same swap and startup rollback apply to AppImages. Unpacked installations use
  the manual download action.
- **Windows:** the helper runs the verified MSI passively with existing rights and relaunches
  the client. The previous downloaded MSI remains in the update directory for manual repair.

If the install location is not writable, the launcher offers **Download update**. It opens the
verified DMG or installer for you to finish; on Linux it opens the folder containing the AppImage.
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
