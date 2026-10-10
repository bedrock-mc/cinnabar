# Profile screen reference evidence

The Profile target is the release client. The installed reference application's
`Info.plist` reports `CFBundleShortVersionString=1.26.50` and
`CFBundleVersion=1.26.50.04`. Reference assets remain outside git.

The reference root is
`~/Library/Containers/io.playcover.PlayCover/Applications/com.mojang.minecraftpe.app/data`.
In the notes below, **J** means
`gui/dist/hbui/index-168bae443ec79c00823c.js`, **C** means
`gui/dist/hbui/index-800b52fb984b5ed54515.css`, and **O** means
`resource_packs/oreui/texts/en_US.lang`, all relative to that root.
**P** means the worktree's
`.local/assets/bedrock-samples/v1.26.50.4/full/resource_pack/texts/en_US.lang`.

## Layout and controls

### Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Header | Profile header uses “Your Profile”; other players use their name |
| Breakpoint | Profile breakpoint is 70rem; wide columns are 4/8, narrow 0/8 |
| Scrolling | Left card and right tab content scroll independently |
| Card border | Card has a 0.2rem border and natural height |
| Banner | Banner aspect ratio is 16:9 |
| Default banner | Default banner is selected by the sum of JavaScript UTF-16 code units modulo the eight images |
| Gamerpic | Gamerpic is 5.2rem square; name and status sit alongside it |
| Character image | Successful character image is 14.8×19.6rem, bottom -7.6rem, left margin -4rem; gamerpic moves to bottom-right |
| Card margins | Card name margins are 1.2rem; action margins are 1.2rem |
| Narrow card | Narrow card is a 12.8rem horizontal card; banner occupies 40% |
| Narrow margins | Narrow content margins are 0.8rem; banner covers and crops at its center |
| Self action | Self action is secondary “Dressing room”, hanger icon, navigating to persona |
| Tabs | Tabs are 4.8rem, selected 4.4rem with 0.4rem top offset; underline is 4.8×0.2rem |
| List rows | List rows have minimum height 7.2rem, 0.2rem border, overlapping bottom border |
| Action padding | Action padding is 0.8rem; main cell horizontal padding is 0.8rem |
| Row icons | Row icons are 2.4rem; labels/values are stacked |

The card name uses Header5B (2rem font, 2.4rem line). Stats labels and values
use captionShort (1.4rem font, 2rem line), with a dimmest label. Overview
counts use body (1.6rem font, 2rem line). J `vZ`, `EJ`, `g2`; C
`.d6e62706875e51a9fa20`, `.fc77bf1310dc483c1eba`, `.bcd956e248e044dfd9a3`.
J `vZ` places both the name and status inside `yu`. C
`.e41d8159223d4eea19af` sets `white-space: nowrap`, `overflow: hidden`, and
`text-overflow: ellipsis`; long labels stay on one line in both card layouts.
The reference font is Minecraft Seven v2; the shipped Cinnangles Seven remains
the accepted repository deviation.

## Overview content

J `g2` places Friends and Followers side by side in a shared list row, then
Screenshot gallery (current/max count), then Achievements (unlocked/total)
with Minecraft gamerscore (current/max) in its right cell. Those rows are
actions, with zero friends/followers disabling their action. J `b2` places
the first three suggested and first three completed achievements below them,
subject to platform and build flags. J `Ik`, `Rk`, and `xk` establish suggested
order and descending unlock-date order. O:2833, 2837, 2840, 2843, 2852,
2876, 2883 provide the corresponding labels.

These values are distinct services/facets: friend list, followers list,
screenshot gallery, achievements, and player statistics. An Xbox account's
total gamerscore does not replace the Minecraft achievement summary. J `XZ`
loads the relevant facets when opening Profile, independent of which tab is
selected. Screenshot and avatar data cannot be invented from Xbox gamerpic.

## Stats values and formatting

J `h2` reads `vanilla.playerStatistics`, passing each entry's `label`, `icon`,
`valueRaw`, `valueDisplay`, and `valueNarration` into J `EJ`. It does not
convert distances to kilometers. The loaded empty list has no empty message;
J `pQ` receives no `emptyMessage` argument.

The four service names and label keys are MinutesPlayed, BlockBrokenTotal,
MobKilled.IsMonster.1 and DistanceTravelled. J's unused `ek.BlocksBroken` enum
says BlocksBrokenTotal, but the service constructor and request list use the
singular BlockBrokenTotal. Labels are P:11130–11133.

Truncate MinutesPlayed to an integer, multiply by 60, then format the duration
with three fields and format 2. Split days, hours, minutes and seconds, omit
leading zero fields, and retain trailing fields up to the three-field limit.
Format 2 uses localized one-character abbreviations. P:2755–2757 gives the day,
hour and minute templates. English examples are `59m`, `2h 0m`, `1d 0h 0m`.

For non-time values, truncate the double to an integer and group digits by three
with commas. This includes raw DistanceTravelled, with no identified kilometer
conversion. Negative, missing, non-finite or out-of-range values stay unavailable
in Cinnabar.

## Loading, error, and unavailable states

J `p2` computes the left card's loading state independently from the right
content: profile loading, requested avatar, pending permissions, or loading
featured image keep it loading. J `ap`, `np`, `lp` disable scrolling and
content focus while loading. J `ep` uses the pixelated animation asset at
2rem square; the loading panel defaults to rgba(0,0,0,0.4).
The installed `assets/animation-074ed0ba8c16bb30e36c.gif` is 28×28 with
ten frames at 100ms each. Optional originals mode decodes those frames at
runtime and selects them using the menu clock; no image bytes enter git.

J `m2` replaces the whole card/tab layout with a centered ten-column error
panel when offline, signed out, or user-not-found (eight columns when narrow).
J `r2` selects:

- Offline: connection error image, load-error title O:2854, offline body O:2864.
- Signed out: nothing-to-see image, title O:2869, body O:2865, primary sign-in O:2881.
- User not found: generic error image, title O:2854, body O:2853, retry O:2838.
- Privacy denied: separate adult/child images, title O:2862, body O:2861.

This document records the target behavior. It does not close a visual parity
gate: implemented behavior and remaining gaps are tracked in `plan.md`.

Opening Profile subscribes to or refetches the profile instead of waiting for
the Home feed. Avatar state is subscribed to separately. Cinnabar now wakes an independent Profile worker on opening,
retry and account changes. Its terminal control error finishes all loading
inputs and selects the existing unavailable/Retry panel. Optional avatar,
featured-image, statistics and achievement failures finish their own inputs.
Permissions are still not a separate transported facet; privacy/offline error
classification remains incomplete as recorded in `plan.md`.

Every startup mode owns an account control core, including direct-address
and external-socket runs. Direct remote sessions retain their separate game
core; local worlds use the core that opened them, even after a direct-startup
session returns Home. Account sign-in cannot restart a core serving a local
world. Direct-mode account cores omit the shared resource-pack cache lease,
leaving it to remote game cores; launcher-owned game cores retain their lease.
An absent feed worker selects unavailable immediately, and a control reply
withheld for 60 seconds ends in unavailable. This deadline is Cinnabar's local
transport safeguard, not a
claimed vanilla timeout. Core diagnostics identify each Profile dependency
before it runs and report fixed outcomes afterward, without credentials,
account identifiers, URLs or response bodies. Repeated core facet/outcome
lines are limited to one per 30 seconds; client RPC pairs and missing-worker
diagnostics are limited to one per 15 seconds.

## Account service transport

All Profile requests use the Go core's existing authenticated Xbox HTTP client.
Rust receives display data through `profile.v1`, never account credentials.
Statistics use gophertunnel's typed Profile API. Its Xbox batch reader owns the
statistic names, service configurations, request body, headers and response decoding;
it sums each statistic across the seven retail platform configurations. Cinnabar
only maps the resulting values into its existing display fields.
Numeric service strings retain fractional precision. A failed request is distinct
from a successful response with no recorded statistics, and zero stays zero.

The Overview achievement feed uses
[the Xbox v2 achievement collection contract](https://learn.microsoft.com/en-us/gaming/gdk/docs/reference/live/rest/uri/achievements/uri-achievementsusersxuidachievementsgetv2)
with `titleId` from `auth.AndroidConfig.TitleID`, the same shared configuration
used by `core/authcache/session.go`. It reads every continuation page, deduplicates
achievement IDs, and computes unlocked/total counts and earned/possible gamerscore
from the returned achievement rewards. Invalid or overflowing reward totals stay
unavailable. Malformed framing and unbounded pagination fail the feed rather than
reporting a partial total. Icon URLs pass the existing HTTPS artwork policy and
are cached by the core.

The character is the persona service's rendered avatar image, requested with
`getMinecraftAvatarByXuid`; the gamerpic remains its fallback. Build
`api/v1.0/profile/xuid/{xuid}/image/{subtype}` from the discovered `persona` URL.
The pinned Go transport already provides `persona.ImageAvatar` and
`ProfileImage`, with service-token authentication and a GET image response.
The existing Home persona-head transport establishes the corresponding core
integration pattern. No fixed production host or substitute native skin is
needed.

Screenshot counts differ for self and other profiles. J `g2` selects
`vanilla.screenshotGallery` for self and `vanilla.playerShowcasedGallery` for
others. The self limit is 100; read the local screenshot collection when no full
gallery count has loaded. The remote showcase limit has a fallback of five.
The persona service provides gallery list, size and featured-image routes. A remote showcased-image count cannot substitute
for the self gallery count. Its local persistence and screenshot management
remain separate parity work.

The Xbox collection supplies localized names, locked/unlocked descriptions,
icon art, score rewards and unlock dates. It does not establish Minecraft's
`suggestedOrder` or persona reward metadata; those fields remain incomplete.
The collection's service order is preserved and is not claimed to be vanilla's
suggested-achievement order. Fixture tests use authored responses and synthetic
transports, with no live account requests or `.local` dependency.

The character image uses the existing Minecraft persona client, requesting the
`avatar` subtype with `Accept: image/*` and the account's Minecraft service token.
The host comes from persona service discovery, with no fixed fallback host.
The Profile subscriber requests `getMinecraftAvatarByXuid` at
`api/v1.0/profile/xuid/{xuid}/image/{subtype}`, resolving `persona` through
ServicesManager. The Go fork already
implements this contract in `minecraft/service/persona/persona.go`.
The core caches image bytes under a content hash, publishes a local artwork path,
and retains current avatar and achievement icons during artwork pruning.
An avatar failure remains separate from otherwise available account data.

The featured banner may be read from the existing Gallery client using the
same discovered persona service and Minecraft service token. Cinnabar selects only
an entry whose `isFeatured` flag is true and whose URL passes the artwork
policy; no featured entry selects the XUID-based banner fallback. The returned
image is downloaded through the core artwork cache. A showcased collection's
length never stands in for the self Profile screen's local screenshot count.
