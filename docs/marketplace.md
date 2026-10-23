# Community mod marketplace

The proof of concept pairs a website storefront with an OreUI browser under
**Settings → Mods → Browse marketplace**. Both read
`crates/launcher/data/marketplace-demo.json`. All entries are synthetic. Saving
an entry changes a local preview library; it does not download a component,
enable a mod, grant permissions, or sync between devices.
Client preview saves last only for the current client session. Website demo
state remains in that browser until it is reset or browser storage is cleared.
The interface presents a signed-in account scenario with fictional profiles and
normal library, submission and staff language. The website account switcher
selects presentation scenarios; it does not authenticate anyone. Offline client
captures use the same fictional profile. The running client reflects its real
launcher account state. No credentials or verified identity are supplied by the
catalog's account records.

The [website repository](https://github.com/cinnabar-mc/website) owns the web
storefront, creator submissions, review queue and staff screens. Its pinned
client inputs supply the sample catalog and review policy. The client owns
native browsing and the existing component import and permission flow.

## Production service

Use one marketplace service in the existing Cinnabar API for catalog revisions,
accounts, libraries, submissions, staff assignments and an append-only audit
trail. Keep browsing public. Require a verified account for library changes,
submissions and staff actions. The browser must never decide authorization.

| Role | Responsibility |
| --- | --- |
| Player | Browse and manage their own library |
| Creator | Submit and revise their own mods |
| Moderator | Review another creator's submissions and suspend listings |
| Administrator | Manage staff membership and moderation |

The POC derives role permissions and publication transitions from the shared
fixture. Production should permit multiple role assignments per account,
bootstrap the first administrator outside public signup, prevent removal of the
last administrator, and audit every assignment, revocation and review with the
acting account, target revision, reason and server timestamp. Staff permissions
belong in the database; Microsoft sign-in alone never makes someone staff.

Review immutable revisions. Approval must bind the exact artifact digest,
permission declaration and metadata. Editing an approved release creates a new
pending revision; it cannot replace the approved bytes. Enforce revision
preconditions on writes so two reviewers cannot overwrite each other's decision.
Reject self-review, unknown roles and invalid state transitions. Run artifact
validation before approval and allow removal from discovery independently of
installed-mod state.

## Microsoft and game identity

Use Microsoft authorization code sign-in with PKCE, state and nonce on the web,
with exact registered redirect URIs. Keep tokens and client secrets on the API;
issue secure, HttpOnly session cookies and protect state-changing requests from
CSRF. Verify token signature, issuer, audience and expiry against the provider's
published metadata. Use the verified issuer and subject as the external login
key, never an email address or display name.

Microsoft sign-in does not by itself establish an Xbox identity. Link a game
account only after verifying an Xbox identity through the approved Xbox service
flow or proof from the client's existing authenticated account service. Bind a
short-lived, single-use linking challenge to both signed-in sessions; show the
account being linked before confirmation. Do not accept an XUID supplied by a
form. Store the permitted partner or pairwise identity with a uniqueness
constraint after confirming the integration's identity-storage requirements;
do not assume global XUID storage is permitted for account linking. Require
fresh authentication for relinking and provide unlinking. Gamertags are display
values and can change.

The client reuses its existing Microsoft/Xbox account flow. Exchange verified
account proof for a narrowly scoped marketplace session at the API rather than
sharing the client's token cache with the website. A shared account then owns
one library across both surfaces. Pairing proves control of both sessions; it
does not prove that they use the same underlying Microsoft account.

Microsoft application registration, allowed personal accounts, redirect URIs,
Xbox service access and service credentials must be configured and validated
before enabling sign-in. None is provisioned by this POC. See Microsoft's
[authorization code flow](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-auth-code-flow)
and [Xbox website authentication](https://learn.microsoft.com/en-us/gaming/gdk/docs/services/fundamentals/s2s-auth-calls/service-authentication/live-website-authentication?view=gdk-2604).

## Installation and remaining work

An approved production release needs immutable object storage, an authenticated
library API, signed catalog metadata and a hash-verified download. Route the
download through the existing component validator and explicit per-mod grant
review. Saving a mod must never grant runtime authority. Updating a mod whose
permission requests change requires renewed consent.
Marketplace-managed components must update only to approved releases. Embedded
GitHub update declarations must not bypass marketplace revision review.

The POC has no production backend, durable accounts, Microsoft callback,
verified Xbox linking, downloads, cross-device library, payments or production
moderation. Native UI captures exercise offline states; live controller, touch
and authenticated multi-account acceptance remain incomplete. This feature
does not close a vanilla parity gate.
