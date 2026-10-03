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
**R:owner:line** refers to the supplied 26.30 reconstruction under `src/by-owner`.
Lens citations name the reconstructed 1.26.50.26 client (artifact 6) and function RVA.

## Layout and controls

| Element | Reference |
| --- | --- |
| Profile header uses “Your Profile”; other players use their name | J `m2`; O:2848–2849 |
| Profile breakpoint is 70rem; wide columns are 4/8, narrow 0/8 | J `p2`, `f2` |
| Left card and right tab content scroll independently | J `p2`, `b2`, `h2` |
| Card has a 0.2rem border and natural height | J `bZ`; C `.b3ea77029532ae5c1380` |
| Banner aspect ratio is 16:9 | J `bZ`, `nm.AspectRatio` |
| Default banner is selected by the sum of JavaScript UTF-16 code units modulo the eight images | J `mZ` |
| Gamerpic is 5.2rem square; name and status sit alongside it | J `vx`, `bZ`; C `.cad011cd688da5294d75` |
| Successful character image is 14.8×19.6rem, bottom -7.6rem, left margin -4rem; gamerpic moves to bottom-right | J `hZ`, `bZ`; C `.e00b5062b199c807b468`, `.c72117dd90e5ec13dea3` |
| Card name margins are 1.2rem; action margins are 1.2rem | C `.e029b233669f292c691e`, `.c0d0e3a4658d188285cf` |
| Narrow card is a 12.8rem horizontal card; banner occupies 40% | C `.cfdfb462a95f1a51c994`, `.ce85aa1190044faf5956` |
| Narrow content margins are 0.8rem; banner covers and crops at its center | C `.a467b7ac28ebbbf52ae5`, `.c61f96cf5e1c013afbe9`, `.b86424395a97c332f9a1`; J `nm`, `lm` |
| Self action is secondary “Dressing room”, hanger icon, navigating to persona | J `FZ`; O:2199 |
| Tabs are 4.8rem, selected 4.4rem with 0.4rem top offset; underline is 4.8×0.2rem | J `cL`, `sL`; C `.e2f6aa3d9859af25ebc1`, `.caa7713b620a56fca5ba`, `.a2c11937fe7d1cff8659` |
| List rows have minimum height 7.2rem, 0.2rem border, overlapping bottom border | J `uK`; C `.d88f68ad0091a0ae663f` |
| Action padding is 0.8rem; main cell horizontal padding is 0.8rem | C `.c99e146579fc302b9d5f`, `.c0ba399cff2bbe877009` |
| Row icons are 2.4rem; labels/values are stacked | J `EJ`; C `.cc5bc5ac25b30e8245f4` |

The card name uses Header5B (2rem font, 2.4rem line). Stats labels and values
use captionShort (1.4rem font, 2rem line), with a dimmest label. Overview
counts use body (1.6rem font, 2rem line). J `vZ`, `EJ`, `g2`; C
`.d6e62706875e51a9fa20`, `.fc77bf1310dc483c1eba`, `.bcd956e248e044dfd9a3`.
The reference font is Minecraft Seven v2; the project's open font remains
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

Lens 1.26.50.26 `0x5eac00` establishes the four service names and label keys:
MinutesPlayed, BlockBrokenTotal, MobKilled.IsMonster.1, DistanceTravelled.
J's unused `ek.BlocksBroken` enum says BlocksBrokenTotal, but the reconstructed
service constructor and request list use the singular BlockBrokenTotal.
R:PlayerStatisticsFacet:332, :342, :350, :364 corroborate the icon mapping.
Labels are P:11130–11133.

Lens `0x63bfd0` (lines 1038–1042) truncates MinutesPlayed to an integer,
multiplies by 60, then formats its duration with three fields and format 2.
Lens `0x775540` delegates to `0x775620`; that function splits days, hours,
minutes and seconds, omits leading zero fields, and retains trailing fields
up to the requested three-field limit. R:DateHelper:963 identifies format 2
as localized one-character abbreviations. P:2755–2757 give the day, hour,
and minute templates. English examples are `59m`, `2h 0m`, `1d 0h 0m`.

The non-time branch calls Lens `0x63b1e0` through its formatting adapter.
That function truncates the double value to an integer and groups digits by
three with commas (lines 102–114). This includes raw DistanceTravelled;
there is no identified kilometer conversion. Negative, missing, non-finite,
or out-of-range service values stay unavailable in Cinnabar.

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

## Account service transport

All Profile requests use the Go core's existing authenticated Xbox HTTP client.
Rust receives display data through `profile.v1`, never account credentials.
Statistics request the names documented above, with the shared
`auth.ServiceConfigID`; the method, path and contract version follow
[Microsoft's Xbox SDK UserStatisticsService](https://github.com/microsoft/xbox-live-api/blob/main/Source/Services/Stats/user_statistics_service.cpp).
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

The character is the persona service's rendered avatar image. Lens artifact 6
`0xf8d0b0` requests `getMinecraftAvatarByXuid` in the Profile subscriber; the
gamerpic stays the fallback when that image is unavailable. Lens `0xf80670`
builds `api/v1.0/profile/xuid/{xuid}/image/{subtype}` from the discovered
persona base URL. R:ProfileImageRequester_Minecraft:325 resolves the `persona`
service. The pinned Go transport already provides `persona.ImageAvatar` and
`ProfileImage`, with service-token authentication and a GET image response.
The existing Home persona-head transport establishes the corresponding core
integration pattern. No fixed production host or substitute native skin is
needed.

Screenshot counts differ for self and other profiles. J `g2` selects
`vanilla.screenshotGallery` for self and `vanilla.playerShowcasedGallery` for
others. R:ScreenshotGalleryFacet:116 gives the self limit of 100;
R:ScreenshotGallery:2014–2040 reads the local screenshot collection when no
full gallery count has been loaded. R:PlayerShowcasedGalleryFacet:43 reads the
remote showcase limit with a fallback of five. Lens `0x53c3910`, `0x53c53f0`
and `0x53bf1b0` identify the persona-service gallery list, size and featured
image routes respectively. A remote showcased-image count cannot substitute
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
Lens 1.26.50.26 `0xf80670` corroborates
`api/v1.0/profile/xuid/{xuid}/image/{subtype}`; `0xf8d0b0` is the Profile
subscriber's `getMinecraftAvatarByXuid` request. R:ProfileImageRequester_Minecraft:325
resolves the `persona` service through ServicesManager. The Go fork already
implements this contract in `minecraft/service/persona/persona.go`.
The core caches image bytes under a content hash, publishes a local artwork path,
and retains current avatar and achievement icons during artwork pruning.
An avatar failure remains separate from otherwise available account data.

The featured banner may be read from the existing Gallery client using the
same discovered persona service and Minecraft service token. The showcased
collection route is corroborated by Lens6 `0x53c3910`. Cinnabar selects only
an entry whose `isFeatured` flag is true and whose URL passes the artwork
policy; no featured entry selects the XUID-based banner fallback. The returned
image is downloaded through the core artwork cache. A showcased collection's
length never stands in for the self Profile screen's local screenshot count.
