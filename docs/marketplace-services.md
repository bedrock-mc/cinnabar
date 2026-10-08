# Marketplace services

Behaviour reference for the 26.30 client's store. Control surface: `docs/control-channel.md` (`store_*.v1`); Go: `core/store`.
Status per row: **ref** = vanilla client behavior, **doc** = public PlayFab documentation, **guess** = inferred,
needs one live capture against a sandbox account before it is relied on.

## Auth chain

1. Microsoft account (device code) -> XSTS via `go-xsapi`.
2. PlayFab `Client/LoginWithXbox` on the title id from discovery `auth.prod.playFabTitleId` (`20CA2` on retail) -> session ticket + entity token.
3. Mojang authorization service `POST {auth.serviceUri}/api/v1.0/session/start` with the ticket -> **MCToken** (`Authorization: MCToken ...`). ref
4. Discovery: `GET client.discovery.minecraft-services.net/api/v1.0/discovery/MinecraftPE/builds/{version}` names every service base URI.

| Token | Used for |
| --- | --- |
| MCToken (`Authorization`) + `Session-Id` header | every store-service call below |

The store base URI is discovery `serviceEnvironments.store.prod.serviceUri` (ref); inventory, balances and purchases go to
`serviceEnvironments.entitlements.prod.serviceUri`. Opening the store needs both. Protocol code lives in gophertunnel
`minecraft/service/marketplace`, which keeps each request (and redirect) on its service's origin. The core logs every
failed `store_*` call except images as `launcher service failed`, redacted.

## Service calls

Paths are relative to the owning service's base URI. Every answer is wrapped as `{"result": ...}`; errors are
`{namespace, code, message, customData}`.

| Function | Call | Notes |
| --- | --- | --- |
| Session config | `GET /api/v1.0/session/config` with `Session-Id` | ref. `result`: `knownPages` (name -> page id), `latestTextureVersion`, `binaryUrls`, `globalNotTags`, `storeFilters`, `dressingRoomFilters`, `storeSearch`, `platformSkus`, `storeVersion`, `feedbackCharacterLimit`, `userListsVersion`, `badgePromoCountdownWindow`, `upsellQueries` |
| Layout pages | `POST /api/v2.0/layout/pages/{id}` (`productId/{id}`, `packId/{id}` by navigation action) body `{entitlements, inventoryVersion, listVersion}` | ref. `id` is `knownPages[name]`; a name the config lacks fails as `-32033` and the core logs the configured names. Headers `InventoryETag`, `X-UserLists-Version`; `result.layout[].{sectionName, rows[]}`; a row is `{telemetryId, controlId, components[], queries[]}`; a curated row lists its offers inline in an `itemListComp`, and only those rows are drawn |
| Row continuation | `POST /api/v2.0/layout/items` body `{continuationToken, inventoryVersion}` | ref. `{continuationToken, result: [catalog items]}`; search results continue the same way |
| Search | `POST /api/v2.0/layout/pages/{knownPages.searchResults}` body: page state plus `{search, sortBy: "Relevance", sortDirection: "Desc", filterPastRealmsPlus, filterCurrentRealmsPlus, filters: {}}` | ref. Results are the page's `pagedItemListComp` (`items`, `totalItems`, `continuationToken`) |
| Offer detail | `POST /api/v2.0/layout/pages/productId/{offerId}` | ref. `itemSummaryComp.item`, `purchaseInfoComp.price`, `itemDescriptionComp.description`, `imageGalleryComp.images`, `ratingComp.rating`; tags are `{name, linksToInfo}` |
| Minecoin balance | `POST /api/v1.0/currencies/virtual/balances` | ref. `result.virtualCurrencyBalances: [{type, amount}]` |
| Entitlements | `GET /api/v1.0/player/inventory?includeReceipt=true` | ref. `result.{inventory.entitlements[].id, receipt, thirdPartyReceipts}` |
| Inventory refresh | `POST /api/v1.0/inventory/refresh` body `{}` | ref. `result.version` |
| **Minecoin purchase** | `POST /api/v1.0/transaction/virtual` | ref, below |
| Real-money top-up redeem | `PUT /api/v1.0/transaction/redeem` | ref, not implemented |

### Purchase (ref)

Body:

```json
{"VirtualCurrency": {"Type": "Minecoin", "Amount": "<decimal string>"},
 "OfferId": "<id>", "StoreId": "<id>", "UnitDurationInSeconds": 0,
 "CustomTags": {"ClientId": "", "DeviceSessionId": "", "CorrelationId": "", "TitleId": "20CA2",
                "BuildPlat": 7, "editionType": "", "Seq": 1, "DnAPlat": "", "Xuid": ""}}
```

`UnitDurationInSeconds` is present only for subscription offers. HTTP 2xx = purchased, 422 price mismatch, 412 precondition
failed, anything else a generic failure; the body is not read (`marketplaceErrorCode` belongs to redeem). Sent at most once, never
retried; no answer is an unknown outcome that holds the offer.

### Real-money top-up (documented, not implemented)

Minecoins are bought in the platform store (Microsoft Store here), not through Mojang. The client then redeems the store receipt:
`POST /api/v1.0/transaction/redeem` with the common `CustomTags` block plus `platformPurchaseId`, the platform receipt fields,
`passSubscription`, `sku` and a production/sandbox flag. The coin-bundle screens (`coin_purchase_screen.json`, `MinecoinCatalogModel`)
list the bundles and open the platform store. Cinnabar has no platform-store integration: the insufficient-funds dialog
(`store.popup.purchaseFailedInsufficientFunds.*`) is shown, and its "Get Minecoins" button is where a top-up would attach.

## Owned content

Ownership comes from the inventory call; layout requests send every owned id so the service marks rows. Offer `owned` in the control
results is a lookup in that list (refreshed after a purchase). Downloading or decrypting owned packs for worlds is out of scope.

## Risks

- Third-party clients spending real Minecoins with client-supplied telemetry tags may violate Mojang's terms; the account can be
  actioned. The core never sends a purchase without `confirmed`, never retries, and refuses a repeat of an unresolved one.
- The auth token claims a Windows 10 UWP device while the client is not one; receipts and purchases are attributed to that platform.
- Unverified request shapes (marked guess) hit production services from a real account; reads are harmless, only `transaction/virtual` moves money.

## Purchases setting

The app never sends `store_purchase.v1` unless `store.json` (beside `servers.json`) holds `{"store_purchases_enabled": true}`; a
missing or malformed file means off. With purchases off the whole vanilla flow still runs (offer page, balance check, confirm) and the
send is replaced by the popup "Purchases disabled until verified" (not a vanilla string). The check sits in one place
(`StoreState::dispatch`) and again in the flow, so no path sends while off.

## Render checklist

Every store screen is drawn by the JSON-UI engine from the vanilla `ui/*.json`; data comes from `app/src/store/screens.rs` (globals and
collections) fed by `store_*.v1`. Rows and grids are nested lists: `factory_collection` holds the header item (`TopBar`) then one
item per row; each row's offers, and each offer's info rows and columns, are scoped lists (`name[index].child`). Bindings not listed as
unbound below are populated. Visual pass: for each screen, compare against the expectation and note which listed feed is empty.

Entry: the start screen's "Marketplace" button (`button.menu_store`, always shown) opens the store; Escape or `button.menu_exit`
steps back inside the store, then leaves it for the start screen. Needs a signed-in launcher core; otherwise the store shows the
vanilla connection-failed text.

| Screen | Should appear | Fed by |
| --- | --- | --- |
| `store_layout.store_data_driven_screen`, home | Header bar with "Marketplace", Minecoin balance, search and library buttons; titled rows of offer cards; a trailing "See All" tile on rows that have more; a spinner while loading | `store_home.v1` rows (`factory_collection`, scoped `offer_collection`), `store_balance.v1` (`#coin_balance`), `#page_loading_visible` |
| offer card (all screens) | Thumbnail, title, creator, price line (coin icon, price, strike-through price hidden) or "Owned"/"Free", star rating with score when rated | offer values in `bindings.rs`; thumbnails from `store_image.v1` packed into the menu artwork atlas (`#thumbnail_texture_path`, `RawPath`); info rows/columns scoped under each card |
| row "See All" (`button.show_more_offers`) | Appends the next offers to that row | `store_row_more.v1` with the row's `continuation` |
| same screen, search | Search bar row, then a grid of results; empty-results state; next-page control while more exist | `store_search.v1` (`#search_*`, `#pagination_visible`, `#next_enabled`); results in a `GridList` row (`offer_grid_factory[0].offer_collection`) |
| same screen, offer page | Key art, title, creator, rating, price on the purchase button (or the deactivated button when the balance is short, or nothing when owned), screenshots gallery, description | `store_offer.v1`; `#purchase_*`, `#full_price`, `#main_mashup_key_art_*`, `ItemSummary`/`ImageGallery`/`ItemDescription` rows, `screenshot_collection` |
| `store_inventory.store_inventory_screen` | "My Library" grid of owned offers that resolved to a catalog entry, owned count | `store_entitlements.v1` ids, then `store_offer.v1` for the first 24 (`items_collection`, `#collection_count`) |
| `store_progress.store_progress_screen` | Progress overlay with "This shouldn't take long." while a purchase runs | purchase flow `InProgress` (`#tooltip_text`) |
| `popup_dialog.modal_dialog_popup` | Bundle confirm, insufficient funds ("Get Minecoins"), price mismatch, pending, generic failure with error code and correlation id, purchases disabled, sign-in required | `PurchaseFlow::modal` with vanilla `store.popup.*` texts; the success is a toast text, not drawn yet |

Deliberately unbound (each keeps the layout's own default or stays hidden): text colours, fonts, scales and offsets of the offer info;
badges and icon overlays; genre/language/player-count/tag buttons; wishlist, share, video and rating submission; filters and sorting;
sidebar navigation and nav-button rows; the search box's text entry (the box is inert, so searches run with an empty term);
download and play buttons on owned content; coin bundle purchase (`coin_purchase`) and "Get Minecoins"; the bundle warning
(`bundle_purchase_warning`) and Marketplace Pass error (`csb_purchase_error`) screens; hero and carousel rows; timers and banners.
Unbound visibility flags read false on these screens, as the vanilla controller answers them.

Assumptions to check on screen: a card's row and position are read from the hit key's bracket indices (row = first index minus the
header item); popups replace input on the base screen; a `GridList` needs the single-item `offer_grid_factory` wrapper.
