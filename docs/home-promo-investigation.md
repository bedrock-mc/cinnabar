# Start-screen promo investigation

The left-hand promo is the gathering panel. Its parity gate remains incomplete:
the offline fixture is authored, and the exact current public-config request and
complete click behavior still need verification.

## Owner evidence

The read-only core log at `.local/logs/core.log:4546` records a partial home refresh
at `2026-10-01T16:26:48.065+03:00`. Its sole error is the gathering public-config
request returning 404. Earlier partial refreshes show the same error at lines
2453, 2861, 3244, 3643, 3952 and 4364. Messaging failures would be listed separately
by `catalog.HomeFeed`, so these records indicate a successful messaging request.
They do not establish the response's message count or image-download success.

The non-token catalog cache's home value contains 22 messages: one
`MarketplaceButton` and 21 `InboxMessage` entries. It contains no separate promo
message. Only home message fields were inspected; authentication token files were
not read. No account identifiers or cached payloads are included here.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Messaging POST | Include Authorization, Session-Id, Content-Type and active Accept-Language. |
| Refresh | Use `/api/v1.0/session/refresh` and JSON `sessionId`, `continuationToken`. |
| Start | Use `/api/v1.0/session/start` and JSON `sessionId`, `previousSessionId`, `continuationToken`. |
| Lifecycle | Start first and refresh only after start succeeds. |
| Event | Send JSON `SessionId`, `continuationToken`, `events`. |
| Surfaces | Construct message surface controllers. |
| Images | Associate fetched images by message and image ID with a local file path. |

The refresh request uses the discovered messaging service URI and has no
placement, platform or locale query. Locale is an HTTP header. The service's
registered surfaces are `LoginAnnouncement`, `MarketplaceAnnouncement`,
`MarketplaceButton`, `PlayButton`, `InboxMessage` and `ToastNotification`.
Vanilla starts the session before refreshing. The pinned Go client instead
refreshes immediately. This is another confirmed contract mismatch, still
unfixed. The start-response handler parses the start response's `result.id` as the session
ID and `reportFrequency` (default 20), then handles the shared messages/inbox
response. The pinned Go client cannot update its session ID, so
rewriting only the first request URL would be insufficient. Previous-session
persistence and service-session lifecycle still need verification. The HTTP
Session-Id is separate from the messaging JSON session ID: base request
constructor copies the service's session string, and POST builder uses it. Supplying the messaging client's ID there would be wrong.

The supplied `v1.26.50.4/full/resource_pack/ui/start_screen.json` confirms the
existing main-button banners:

- Lines 1626–1682 define `main_button_banner` and its variable bindings.
- Lines 1782–1788 select `#play_button_banner_*`.
- Lines 2101–2108 select `#store_button_banner_*`.
- Lines 944–1013 define the separate left-hand gathering badge/button panel.
- The featured-world control at lines 1869 and 2316 is unrelated to messaging.

The gathering panel has the described caption/image/button shape. Its visibility
is `#gathering_enabled`; its image is `#gathering_badge` with
`#gathering_badge_file_system`, caption is `#gathering_countdown_text`, and button
is `#gathering_button_text` with action `button.gathering`. The current controller's
label callback uses configured text or
`gathering.button.liveEventFallback`. Cinnabar already binds this panel from
`LiveEventCard`, but the owner's gathering fetch fails with 404.

The Dungeons II tile's association with that panel remains unconfirmed. A
read-only inspection of the locally available iOS
`1.26.50.04` OreUI bundle also did not identify a home news carousel. These
findings do not rule out another platform, treatment or service-delivered layout.
The exact promo surface, image key, Learn More action and geometry need an
identified reference before a promo fixture and visibility fix can be written.

## Pipeline and implemented change

The pinned `playermessaging.Client` refreshes the feed. `catalog.flatten` retains
well-formed top-level messages without a surface whitelist. The launcher downloads
their images, caches home data and serves it through `home.v1`. The bridge and
protocol facade preserve the messages and image paths. Rust polls and refreshes
the menu view, but maps only Play/Marketplace art and inbox messages. It has no
general message-button click handler.

The upstream messaging HTTP client omitted Accept-Language. The Rust launcher
now passes its already selected language, converted to BCP 47, to the Go core.
The messaging HTTP adapter supplies that language on refreshes and event reports
without mutating the caller's request. The existing English service default has
one Go constant shared with the store's fallback.

Offline tests use a synthetic transport and token source. They check locale,
authorization, request paths, absence of query filters, continuation, impression
metadata and request cloning. Removing the locale setter makes the regression
fail with an empty Accept-Language header; restoring it passes. A Rust launcher
argument test checks `pt_BR` becomes `pt-BR`.

No live Microsoft, PlayFab or Xbox calls were made. The earlier investigation did
not capture a tile-present snapshot or close visual acceptance.

## Desktop gathering correction

The Android/Google query literals were in `core/catalog/home.go`, not the fork.
`core/clientplatform` now owns the desktop identity; the gatherings query and the
common authenticated service token configuration use it. Messaging, persona,
marketplace and discovery do not have equivalent platform query parameters in
their current request builders. Their shared token source already defaults to
Windows, and now receives that platform explicitly from the same constant.
The XAL Android OAuth application configuration is separate from these service
platform fields and has not been changed.

The desktop build reports `Windows10` (nine characters) and `Win32` (five
characters). The gathering manager registers service name `gatherings`; a
discovered host containing `gatherings-secondary` does not change that key.
The controller localizes configured button text or falls back to
`gathering.button.liveEventFallback`, now also used by the Rust home-feed mapping.
The pack's `texts/en_US.lang:11964` supplies “Join Game”.

The older client constructs a GET request for `/api/v1.0/config/public` using the
network game version and build platform/subplatform, omitting subplatform only
when empty. Resolve its host through service discovery, send Authorization and
Session-Id, and set JSON Content-Type only for a nonempty body. The fork uses its
supplied Minecraft service token. The current client no longer makes this request
and the service answers it with 404, so the core stopped fetching live events;
the gathering panel stays hidden as in the current client.

The badge callback returns downloaded art or the built-in badge. Its filesystem
callback returns `RawPath` for downloaded art and `InUserPackage` for the fallback.
The current pack's
`ui/start_screen.json:816` binds both texture and texture_file_system;
`:802` binds `button.gathering`, `#gathering_button_text` and button enabled;
`:910` binds `#gathering_countdown_text`; `:1009` controls panel visibility.
Cinnabar was missing the downloaded badge's file-system binding; it now supplies
`RawPath`. Its existing route-to-Servers/direct-connect action is provisional:
the full vanilla controller action, default badge, GIF and countdown behavior
are not claimed as complete.

The offline Go transport test checks the desktop query, request path, discovery host and token,
then maps the synthetic public response into `Home.LiveEvents` and cached artwork.
It can export that actual Go home-feed JSON using `CINNABAR_HOME_PROMO_FIXTURE`.
The Rust mapping test checks configured and fallback labels. The real-carrier
snapshot test consumes the Go feed when supplied, or uses authored test data.
It renders a generated blue badge and requires more than 1,000 badge pixels on
the start screen, with none before adding the event. Normal test runs exercise
the render whenever the carrier is available; requesting PNG output without a
carrier fails. Generated JSON, art and PNG stay outside git. Synthetic coverage
does not establish that correcting the query resolves the owner's service 404.

## Offline recording replay

No recorded successful public-config response was supplied in the handoff or
the home-promo scratch directory. The existing `home-feed.json` was exported
from authored data, not captured from the service. No live requests or token
reads were made to fill this gap. Recorded-response acceptance remains open.

`TestRecordedPublicConfigReachesHomeArtwork` now accepts an external recording
through `CINNABAR_GATHERING_RESPONSE_FIXTURE`. Set
`CINNABAR_GATHERING_FIXTURE_TIME` to its RFC3339 recording time, so expired
events can still be replayed deterministically. The test checks the desktop
request and requires an active event with badge artwork. It explicitly skips
when no recording is supplied, rather than relabeling synthetic data.

To replay without service access, set those variables and
`CINNABAR_HOME_PROMO_FIXTURE` to the desired home-feed output path, then run
`go test ./catalog -run '^TestRecordedPublicConfigReachesHomeArtwork$' -count=1`
from `core`. Pass the exported home-feed path to the Rust test below. Do not
commit recordings containing service-delivered art or account information.

For local PNGs, set `CINNABAR_HOME_PROMO_FIXTURE` to the exported Go feed and
`CINNABAR_FORM_SNAPSHOT_DIR` to the scratch output directory, then run through
the supplied `cslot` limiter:

```sh
cargo test -p bedrock-client --lib --locked snapshot_core_home_promo -- --nocapture
```

This writes `home-promo-before.png` and `home-promo.png` at 2560x1440, DPI scale
2, using the real JSON-UI carrier and a synthetic blue badge. It checks rendering
and feed handoff, not target-platform visual parity or complete click behavior.
The latest local gate run must use `lcheck`; the remote interruption below is
historical and does not authorize remote verification in this continuation.

## Verification interruption

Full `rcheck` was started for code commit
`0ddbf259ce59576d1399e86611c139208ac72fb9` using `RCHECK_SOURCE_MODE=objects`,
which avoids stash and remote ref updates. Formatting passed; clippy began
compiling but the remote pod entered `Failed`. Subsequent SSH polls returned
`cannot exec into a container in a completed pod; current phase is Failed`.
The local poll was interrupted with exit 130. There is no green rcheck result;
architecture, Rust tests and the remaining remote gates are unverified. Focused
offline Go tests passed locally. No new PNGs could be captured.

## Dev integration, October 2

Merged `origin/dev` at `8201d6e0`, preserving its JSON-UI stack and both sets of
incomplete parity notes. Launcher service localization now reads dev's existing
current-language state, rather than a second state that retained only the first
selected language. Desktop service identity, downloaded badge filesystem,
fallback labels and offline recording replay remain in place.

The integration retains `Windows10` and `Win32`, configured text or
`gathering.button.liveEventFallback`, gathering query fields and controller
bindings. The installed
vanilla pack's `ui/start_screen.json:816` binds the badge texture and filesystem,
and `texts/en_US.lang:11964` supplies the Join Game fallback. Current public
request and click parity remain incomplete as described above.

Five alternating focused latency runs on clean dev and the merged promo tree all
passed with the same installed carriers and limiter. Prepared Settings took
5.9–8.1 ms on dev and 5.9–13.1 ms on the merged tree, below the unchanged 16 ms
limit. Median warm Home frames were 0.5 and 0.6 ms respectively. Samples included
the existing live-event fixture; these are local test timings, not release
performance acceptance. The fresh offline before/after PNG regression also
passed, with the caption, badge and button visible without clipping.

Historical cross-check: five runs each on the old common base `11c02e49` and
original promo HEAD `3e325007` also passed. Prepared Settings measured 0.6–2.6 ms
on the base and 0.7–3.3 ms with the promo changes. Their settings cache, menu
rendering and original latency test are identical. No promo-specific regression
was reproduced. The earlier seconds-long failure is consistent with load-sensitive
shared preparation: the old engine could wait for background work and then do
layout synchronously if its three-second warmup was insufficient. This is an
inference from the source and repeated runs, not a reproduction of the earlier
machine load. Dev's current engine prepares Settings without that blocking wait.

The full local `lcheck` completed with exit 0 using four Rust test threads:
formatting, strict all-target Clippy, architecture, workspace tests, and both Go
vet/test suites passed. The app library had 2,119 passed, zero failed and 18
ignored. Recorded public-config replay still skips without a genuine recording;
the synthetic render does not close native visual or complete click acceptance.
