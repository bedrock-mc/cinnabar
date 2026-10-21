# Flatpak

The Flatpak package uses the same client and first-run resource preparation as the
AppImage. Flatpak owns application updates; Settings > Updates explains how to
update through the software store or `flatpak update`. Application channel switching,
direct installer downloads, and replacement of the installed app are unavailable.
Mod updates remain independent.

## Build and install

On Linux, install `flatpak`, `flatpak-builder`, Python 3.11 or newer, the repository's
Rust toolchain, and a Go compiler supporting its modules. Prepare the sources from
the repository root:

```sh
python3 packaging/flatpak/prepare.py
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak-builder --user --install-deps-from=flathub --force-clean \
  --repo=.local/dist/flatpak/repo .local/dist/flatpak/build \
  .local/dist/flatpak/io.github.cinnabar_mc.cinnabar.json
flatpak install --user .local/dist/flatpak/repo io.github.cinnabar_mc.cinnabar
flatpak run io.github.cinnabar_mc.cinnabar
```

Preparation copies tracked working files, vendors the locked Rust and Go dependencies,
and creates a source archive and concrete manifest under `.local/dist/flatpak`. Add new
source files to Git before preparing a local build. Untracked files, local carriers,
build outputs and credentials are excluded. Preparation needs network access; compilation
inside `flatpak-builder` does not. The generated manifest downloads the exact Rust
toolchain declared in `rust-toolchain.toml`, using the Rust distribution's component
checksums. Build tools are removed from the installed payload.

The **Flatpak** GitHub workflow builds x86-64 packages for packaging PRs and manual
runs. Its artifact contains an installable `.flatpak`, the manifest and the source
archive. Add the Flathub remote above before installing the bundle so Flatpak can
resolve its runtime:

```sh
flatpak install --user Cinnabar-x86_64.flatpak
```

## Permissions and acceptance

The package requests network, GPU, controller, audio, Wayland and fallback X11 access.
It does not request home-directory access or permission to run host commands. File
imports and browser links use the existing desktop portals. Settings, downloaded
resources, logs and worlds use Flatpak's per-application XDG directories, separately
from an existing AppImage install.

Before publishing, test a clean first run, sign-in and browser return, a multiplayer
join, native local-world startup, world/pack import, controllers, audio, Wayland and X11,
and an update that preserves settings and worlds. Docker-backed local worlds on other
architectures are outside this initial x86-64 package.

## Flathub publication

This adds package support; it does not create a Flathub listing. A submission still
needs a stable release, public screenshots and release metadata, and acceptance testing
of the built package. Flathub currently excludes nightly releases and requires new
submissions to enter its stable repository first.

For submission, publish the prepared source archive with its release and replace the
generated manifest's local archive path with that public URL, retaining its SHA-256.
The archive includes the vendored sources for offline compilation. Submit the manifest
through Flathub's review process, then verify the application listing. Keep metadata
in this repository.

For later releases, regenerate the source archive and manifest, open an update PR in
the Flathub packaging repository, install its test build, and publish after testing.
Keep the runtime in the manifest template supported and regenerate after toolchain or
dependency changes. Do not commit generated archives or vendored dependencies here.
