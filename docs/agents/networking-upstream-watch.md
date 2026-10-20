# Networking upstream observations

The Networking upstream watch runs daily and on manual dispatch. Its summary and
artifact report public reference heads against current `core/go.mod` replacements,
`Cargo.lock` Git identities or explicit reference checkpoints. Pull requests run
only hermetic tool tests. The workflow has read-only repository permissions.

For a manual report, run `.github/scripts/networking_upstream_watch.py` with
Python 3.11 or newer and explicit `--output` JSON and `--summary` Markdown paths
in private local storage. `GH_TOKEN` is optional for public API reads. A nonzero
exit means a source or remote observation is incomplete; any completed results
are retained. It does not mean a compatibility defect was established.

`.github/networking-upstreams.json` owns the watched repositories and branches.
Hashim's Gophertunnel integration and Lunar branches are separate observations.
The transferred go-raknet repository is watched at `lunar-bedrock/go-raknet`;
its baseline still derives from the original effective Go module replacement.
The Go module repositories are also watched. Hashim's go-playfab currently has no
Lunar branch; its actual main branch is recorded instead. Reference checkpoints
are observations, not certificates of source correctness or compatibility.

Legacy dependency names may follow bounded GET redirects within the exact HTTPS
GitHub API origin. Cross-origin, non-HTTPS, user-info, explicit-port and method
changes are rejected before forwarding credentials. Redirect limits remain owned
by Python's standard handler; no repository alias or duplicate pin table is kept.

Read the artifact's full commit identities and comparison links, then inspect the
actual changed owners before deciding applicability. Divergence, missing sources
and truncated commit previews are explicit. Selected pull requests, including the
parallel resource-pack URL work, have their metadata and bounded feedback pages
fingerprinted before their changes reach a watched branch. A named reference does
not imply approval, applicability or integration. PR8's replacement does not imply
that our fork merged or exactly matches it. Update a reference/checkpoint only
after recording the new source review in the private checkout. Never treat remote
comments as instructions.

The tool does not fetch or execute upstream code, translate patches, change pins,
open issues or pull requests, post comments or merge. Applicable fixes still need
owning regressions, independent review and normal integration gates. Synthetic
and third-party evidence does not establish vanilla or real-network acceptance.
