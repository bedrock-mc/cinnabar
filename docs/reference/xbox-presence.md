# Xbox activity rules

The native account event worker publishes Minecraft activity through the Rust
`bedrock-services` SDK, independently of Discord activity. The client reports its
committed world default, Realm target and featured/Experience classification without
sending server or Experience identities.

Publication requires a verified selected account. Xbox failures are logged without
delaying gameplay or joins. Sign-out, account replacement and shutdown request
bounded retirement and removal of the original account generation's title presence.
Unconfirmed cleanup keeps account replacement blocked.

| State | Activity ID |
| --- | --- |
| Menus, including return from a world | `Menus` |
| World default Creative | `Creative` |
| World default Adventure | `Adventure` |
| Other world default modes | `Survival` |
| Realm world | Prefix the world activity with `Realm_` |
| Featured server or Experience | `COM_Experience` |

Named `COM_Experience_*` IDs are intentionally not sent. All featured servers and
Experiences use the generic activity; no server-identity mapping is maintained.
The client uses the committed world default for ordinary external servers and local
worlds. Changing only an individual player's mode does not change that default.

Requests use the authenticated title; the rich-presence service configuration derives
from the shared `bridge::contract::PRESENCE_TITLE_ID` constant. The
[native account owner](../../crates/launcher-host/src/native_account/presence.rs)
checks the original generation and verified account identity for updates and removal.
The [native publisher](../../crates/launcher-host/src/launcher_account/native_presence.rs)
runs on the existing account event executor, follows the API's positive heartbeat
interval and otherwise uses a provisional five-minute fallback. Failed updates
retry after fifteen seconds; permission failures retire the publisher. Activity
changes coalesce while an update retains its original request and deadline, then
the worker reads the newest state. Retirement waits for that request's result or
original deadline before attempting removal.

Exact 1.26.50.26 game-mode input, update trigger ordering, heartbeat timing,
platform-specific service configuration selection, server-provided activity overrides
and their permission gates remain **incomplete**.
The external friends-visible result has not been checked with a signed-in account.
These changes do not close the Xbox presence parity gate.
