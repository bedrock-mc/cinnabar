# Local development bundles

`make dist-local` stages a relocatable bundle from binaries and compiled runtime
carriers that already exist on the developer machine. Output goes to the ignored
`.local/dist/<platform>` directory and contains a deterministic, path-sorted
SHA-256 manifest whose `distribution_scope` is `local-development-only`.
The manifest also records the explicit Rust target triple and Git commit and
stages `THIRD_PARTY_NOTICES.md` beside the platform resources.

The tool accepts `DIST_PLATFORM=windows`, `linux`, or `macos`. Override
`DIST_CLIENT`, `DIST_CORE`, `ASSET_BLOB` (its parent directory supplies the
carrier set), `PHYSICS_REGISTRY`, or `DIST_OUT` when staging synthetic fixtures
or a different local build. `DIST_TARGET`, `DIST_GIT_COMMIT`, and `DIST_NOTICES`
are explicit inputs as well. The output directory must not already exist.

The staging boundary is deliberately narrow: every input must be a bounded
regular file reached without symlinks, destinations come from a fixed platform
table, required carrier names are explicit, collisions and parent traversal are
rejected, and paths that look like credential material are refused. The command
does not fetch assets, discover credentials, or include authentication state.
Inputs are opened through validated file handles rather than reopened by path
during copying. Like ordinary local build tooling, this is not a sandbox against
an attacker concurrently replacing otherwise valid regular files.

These bundles are unsigned developer conveniences. This target is not a public
installer or release pipeline and makes no signing, notarization, support, or
redistribution claim.
