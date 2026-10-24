# Marketplace installation

The website's Add to Cinnabar action queues a release for the signed-in Xbox
account. The account core receives the library in the authenticated API welcome
and subsequent `mod_library` messages. `cinnabar_mods.v1` exposes only that live
account's bounded selectors to the native client; it carries no access tokens or
permission authority. The API schema and its immutable revision are recorded in
`core/cinnabarapi/schema/source.json`.

The native marketplace worker uses the API origin from the generated bridge
contract. It fetches the exact signed release by project, version and SHA-256, verifies
the original statement bytes with a pinned Ed25519 public key, checks the
selection/size/hash, and retrieves the component through its digest URL. Public
requests carry no account credentials and follow no redirects. Responses,
component bytes and cached metadata are bounded; account changes cancel pending
work. Returning to an account retries selections that were not installed. Verified
receipts retain successful installations and local removals across account changes.
The account and exact selection are confirmed again after downloads.

The existing component host checks ABI compatibility without executing uploaded
code. The component's declared version and exact requested grants, including
command values, must match the signed release. Components and signed receipts
are published under immutable, account/hash-derived names; each file is complete
before it becomes visible. Imports never overwrite existing files, enable a mod,
copy prior grants or authorize new ones. New releases appear under Settings →
Mods → Available; enable them and choose permissions through the existing consent
controls. New versions remain separate files so an update cannot silently inherit
another file's authority. The GitHub mod updater cannot replace managed files.

## Withdrawal and offline behavior

Signed revocation snapshots include issue and expiry timestamps. The API issues
fifteen-minute snapshots; the client refreshes every minute, rejects expired,
future-dated or rolled-back evidence, and persists the last signed checkpoint.
Known withdrawn hashes stay held. Expired cached snapshots retain their issue
time across restarts but never grant a fresh execution window.

Within an established account session, a network failure does not extend the
remaining signed window. Managed mods are held when that window expires, the
account changes, a withdrawal arrives, or the receipt/source is invalid. A new
client account session must establish its identity through the authenticated API.
Source admission, prepared-instance publication and frame authority all check
the current guard. Local developer imports keep their existing explicit grants.
Managed filenames remain reserved even if a receipt is deleted.

Removing a managed mod through Mods leaves its signed receipt as a consumed
request. The worker queues `mod_library_remove` through the authenticated game
connection until the server drops that exact account/project/hash/send timestamp.
It cannot remove another account's selection or erase a later website send. A
new Add to Cinnabar action can intentionally reinstall the release.

## Production configuration and acceptance

Populate `crates/launcher-host/data/marketplace-keys.json` with reviewed base64
encodings of the raw 32-byte Ed25519 public keys corresponding to the API signer.
The initial public key is provisioned alongside the API's encrypted GitHub
`production` signing secret. The API deployment checks that its signer matches
the immutable client input before installing it. Deploy that API before shipping
the client key pin; unverified responses keep managed files held. Never put
the private signing key in the client, trust a key supplied by a marketplace
response, or reuse test keys. Rotation ships overlapping reviewed public keys
before the server changes its signer.

Deploy the API with exact-release retrieval, expiring signed revocations and
authenticated conditional removals before enabling client downloads. Test the
real website → account connection → disabled install → explicit consent flow,
offline expiry, restart, account switches, withdrawals and filesystem failures
on supported platforms. Synthetic tests cover signature tampering, exact grant
comparison, rollback/expiry, cancelled publication, collisions, consumed removals
and authenticated transport. The synthetic website/API handoff is replayed through
the native HTTP downloader, disabled installation and signed withdrawal path;
the same component exercises native ABI validation and Mods enable/consent actions.
These checks do not constitute production-domain
or interactive-client acceptance evidence.
