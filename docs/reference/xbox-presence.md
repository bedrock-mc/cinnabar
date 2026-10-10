# Xbox activity rules

The account core publishes Minecraft activity independently of Discord activity.
The client reports its committed world default, Realm target and featured/Experience
classification without sending server or Experience identities.
Xbox failures are logged without delaying gameplay or joins. Offline accounts do not
create an Xbox presence client; sign-out and core shutdown stop updates and remove
this title's presence.

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

The request uses the account's authenticated title ID and its corresponding service
configuration UUID. Updates use `go-xsapi` and retain one client until shutdown.
The core follows the API's returned heartbeat interval; a missing interval uses a
provisional five-minute fallback. Failed updates retry after fifteen seconds,
and a changed client state cancels an older request and queues the newest state.

Exact 1.26.50.26 game-mode input, update trigger ordering, heartbeat timing,
platform-specific service configuration selection, server-provided activity overrides
and their permission gates remain **incomplete**.
The external friends-visible result has not been checked with a signed-in account.
These changes do not close the Xbox presence parity gate.
