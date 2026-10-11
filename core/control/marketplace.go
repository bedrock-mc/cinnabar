package control

import (
	"context"
	"encoding/json"
	"errors"
	"net"

	"github.com/sandertv/gophertunnel/minecraft/service/marketplace"
)

const (
	methodStoreHome         = "store_home.v1"
	methodStoreSearch       = "store_search.v1"
	methodStoreOffer        = "store_offer.v1"
	methodStoreBalance      = "store_balance.v1"
	methodStoreEntitlements = "store_entitlements.v1"
	methodStorePurchase     = "store_purchase.v1"
	methodStoreRowMore      = "store_row_more.v1"

	codePurchaseBusy   = -32031
	codePurchaseReused = -32032
	codeStoreNotFound  = -32033

	defaultStorePage = marketplace.PageStoreRoot
)

var (
	// ErrStoreInvalidRequest reports store parameters that fail validation before any network call.
	ErrStoreInvalidRequest = errors.New("control: invalid store request")
	// ErrPurchaseBusy reports a purchase of the same offer still in flight or unresolved.
	ErrPurchaseBusy = errors.New("control: purchase in progress")
	// ErrPurchaseReused reports a purchase_id replayed with different parameters.
	ErrPurchaseReused = errors.New("control: purchase id reused")
)

// Marketplace is the Minecraft Marketplace service behind the store_* methods. Offer and
// MoreOffers report ErrStoreInvalidRequest for a malformed id or token before any network call.
type Marketplace interface {
	Home(ctx context.Context, page string) (StorePage, error)
	Search(ctx context.Context, q StoreSearchQuery) (StoreSearchResults, error)
	Offer(ctx context.Context, id string) (StoreOfferDetail, error)
	Balances(ctx context.Context) ([]StoreBalance, error)
	Entitlements(ctx context.Context, offset, limit int, refresh bool) (StoreEntitlements, error)
	MoreOffers(ctx context.Context, token string) (StoreRowMore, error)
	Purchase(ctx context.Context, r StorePurchaseRequest) (StorePurchaseResult, error)
}

// StorePrice is one currency amount an offer costs; Currency is the service's own identifier.
type StorePrice struct {
	Currency string `json:"currency"`
	Amount   int64  `json:"amount"`
}

// StoreRating is an offer's aggregate star rating.
type StoreRating struct {
	Average float64 `json:"average"`
	Count   int     `json:"count"`
}

// StoreOffer is one store listing as shown in a row, grid or search result.
type StoreOffer struct {
	ID           string       `json:"id"`
	Title        string       `json:"title"`
	Creator      string       `json:"creator,omitempty"`
	ContentType  string       `json:"content_type,omitempty"`
	ThumbnailURL string       `json:"thumbnail_url,omitempty"`
	StoreID      string       `json:"store_id,omitempty"`
	Prices       []StorePrice `json:"prices,omitempty"`
	Rating       *StoreRating `json:"rating,omitempty"`
	Tags         []string     `json:"tags,omitempty"`
	Owned        bool         `json:"owned"`
}

// StoreOfferDetail is an offer plus the fields only the detail screen shows.
type StoreOfferDetail struct {
	StoreOffer
	Description    string   `json:"description,omitempty"`
	ScreenshotURLs []string `json:"screenshot_urls,omitempty"`
	DisplayVersion string   `json:"display_version,omitempty"`
	Platforms      []string `json:"platforms,omitempty"`
}

// StoreRow is one titled strip of offers on a store page.
type StoreRow struct {
	ID           string       `json:"id,omitempty"`
	Title        string       `json:"title,omitempty"`
	Kind         string       `json:"kind,omitempty"`
	Offers       []StoreOffer `json:"offers"`
	Continuation string       `json:"continuation,omitempty"` // token for row_more, when the row has more offers
}

// StoreRowMore is the next slice of a row's offers.
type StoreRowMore struct {
	Offers       []StoreOffer `json:"offers"`
	Continuation string       `json:"continuation,omitempty"`
}

// StorePage is a server-driven store page reduced to the rows the client draws.
type StorePage struct {
	ID               string     `json:"id"`
	Rows             []StoreRow `json:"rows"`
	InventoryVersion string     `json:"inventory_version,omitempty"`
	Truncated        bool       `json:"truncated,omitempty"`
}

// StoreSearchQuery is a store search, or the continuation of one.
type StoreSearchQuery struct {
	Term         string
	Continuation string
}

// StoreSearchResults is one page of search results.
type StoreSearchResults struct {
	Offers       []StoreOffer `json:"offers"`
	Continuation string       `json:"continuation,omitempty"`
	Truncated    bool         `json:"truncated,omitempty"`
}

// StoreBalance is one virtual currency balance.
type StoreBalance struct {
	Currency string `json:"currency"`
	Amount   int64  `json:"amount"`
}

// StoreEntitlements is a window of the account's owned content ids.
type StoreEntitlements struct {
	Owned            []string `json:"owned"`
	Total            int      `json:"total"`
	Offset           int      `json:"offset"`
	InventoryVersion string   `json:"inventory_version,omitempty"`
}

// StorePurchaseRequest is one Minecoin purchase the player explicitly confirmed.
type StorePurchaseRequest struct {
	PurchaseID          string // client-generated idempotency key
	OfferID             string
	StoreID             string
	Currency            string
	Amount              string // decimal digits
	UnitDurationSeconds *uint64
	Confirmed           bool
}

// StorePurchaseResult is the recorded outcome of one purchase attempt.
type StorePurchaseResult struct {
	Status               string `json:"status"`
	HTTPStatus           int    `json:"http_status,omitempty"`
	MarketplaceErrorCode int    `json:"marketplace_error_code,omitempty"`
	CorrelationID        string `json:"correlation_id"`
	InventoryVersion     string `json:"inventory_version,omitempty"`
	Replayed             bool   `json:"replayed,omitempty"`
}

var storeMethods = map[string]struct{}{
	methodStoreHome: {}, methodStoreSearch: {}, methodStoreOffer: {},
	methodStoreBalance: {}, methodStoreEntitlements: {}, methodStorePurchase: {}, methodStoreRowMore: {},
}

func isStoreMethod(method string) bool {
	_, ok := storeMethods[method]
	return ok
}

// SetMarketplace enables the store_* methods; safe to call while serving.
func (server *Server) SetMarketplace(m Marketplace) {
	server.mu.Lock()
	server.marketplace = m
	server.mu.Unlock()
}

func (server *Server) marketplaceService() Marketplace {
	server.mu.Lock()
	defer server.mu.Unlock()
	return server.marketplace
}

type storeHomeResultV1 struct {
	SchemaVersion uint32    `json:"schema_version"`
	Page          StorePage `json:"page"`
}

type storeSearchResultV1 struct {
	SchemaVersion uint32 `json:"schema_version"`
	StoreSearchResults
}

type storeOfferResultV1 struct {
	SchemaVersion uint32           `json:"schema_version"`
	Offer         StoreOfferDetail `json:"offer"`
}

type storeBalanceResultV1 struct {
	SchemaVersion uint32         `json:"schema_version"`
	Balances      []StoreBalance `json:"balances"`
}

type storeEntitlementsResultV1 struct {
	SchemaVersion uint32 `json:"schema_version"`
	StoreEntitlements
}

type storeRowMoreResultV1 struct {
	SchemaVersion uint32 `json:"schema_version"`
	StoreRowMore
}

type storePurchaseResultV1 struct {
	SchemaVersion uint32 `json:"schema_version"`
	StorePurchaseResult
}

func (server *Server) serveStore(conn net.Conn, id uint64, method string, raw json.RawMessage) error {
	reply := responseWriter{server: server, conn: conn, id: id}

	market := server.marketplaceService()
	if market == nil {
		return reply.fail(codeServicesDisabled, "Launcher services unavailable")
	}
	ctx, cancel := context.WithTimeout(context.Background(), serviceCallTimeout)
	defer cancel()
	failStore := func(err error) error {
		switch {
		case errors.Is(err, ErrSignedOut):
			return reply.fail(codeSignedOut, "Not signed in")
		case errors.Is(err, ErrStoreInvalidRequest):
			return reply.invalid()
		case errors.Is(err, ErrPurchaseBusy):
			return reply.fail(codePurchaseBusy, "Purchase in progress")
		case errors.Is(err, ErrPurchaseReused):
			return reply.fail(codePurchaseReused, "Purchase id reused")
		case errors.Is(err, marketplace.ErrUnknownPage):
			server.logServiceFailure(method, err) // names the page keys the session config offers
			return reply.fail(codeStoreNotFound, "Unknown page")
		}
		server.logServiceFailure(method, err)
		return reply.fail(codeServiceFailed, "Service unavailable")
	}
	switch method {
	case methodStoreHome:
		var params struct {
			Page *string `json:"page"`
		}
		if len(raw) != 0 && !decodeParams(raw, &params) {
			return reply.invalid()
		}
		page := defaultStorePage
		if params.Page != nil {
			page = *params.Page
		}
		result, err := market.Home(ctx, page)
		if err != nil {
			return failStore(err)
		}
		if result.Rows == nil {
			result.Rows = []StoreRow{}
		}
		result = fitPage(result)
		return reply.ok(storeHomeResultV1{SchemaVersion: 1, Page: result})
	case methodStoreSearch:
		var params struct {
			Term         string `json:"term"`
			Continuation string `json:"continuation"`
		}
		if !decodeParams(raw, &params) {
			return reply.invalid()
		}
		result, err := market.Search(ctx, StoreSearchQuery{
			Term: params.Term, Continuation: params.Continuation,
		})
		if err != nil {
			return failStore(err)
		}
		if result.Offers == nil {
			result.Offers = []StoreOffer{}
		}
		result = fitSearch(result)
		return reply.ok(storeSearchResultV1{SchemaVersion: 1, StoreSearchResults: result})
	case methodStoreOffer:
		var params struct {
			OfferID *string `json:"offer_id"`
		}
		if !decodeParams(raw, &params) || params.OfferID == nil {
			return reply.invalid()
		}
		result, err := market.Offer(ctx, *params.OfferID)
		if err != nil {
			return failStore(err)
		}
		return reply.ok(storeOfferResultV1{SchemaVersion: 1, Offer: result})
	case methodStoreBalance:
		if len(raw) != 0 {
			return reply.invalid()
		}
		balances, err := market.Balances(ctx)
		if err != nil {
			return failStore(err)
		}
		if balances == nil {
			balances = []StoreBalance{}
		}
		return reply.ok(storeBalanceResultV1{SchemaVersion: 1, Balances: balances})
	case methodStoreEntitlements:
		var params struct {
			Offset  int  `json:"offset"`
			Limit   int  `json:"limit"`
			Refresh bool `json:"refresh"`
		}
		if len(raw) != 0 && !decodeParams(raw, &params) {
			return reply.invalid()
		}
		result, err := market.Entitlements(ctx, params.Offset, params.Limit, params.Refresh)
		if err != nil {
			return failStore(err)
		}
		result.Owned = fitOwned(result.Owned)
		return reply.ok(storeEntitlementsResultV1{SchemaVersion: 1, StoreEntitlements: result})
	case methodStoreRowMore:
		var params struct {
			Continuation *string `json:"continuation"`
		}
		if !decodeParams(raw, &params) || params.Continuation == nil {
			return reply.invalid()
		}
		result, err := market.MoreOffers(ctx, *params.Continuation)
		if err != nil {
			return failStore(err)
		}
		if result.Offers == nil {
			result.Offers = []StoreOffer{}
		}
		return reply.ok(storeRowMoreResultV1{SchemaVersion: 1, StoreRowMore: result})
	case methodStorePurchase:
		var params struct {
			PurchaseID          *string `json:"purchase_id"`
			OfferID             *string `json:"offer_id"`
			StoreID             string  `json:"store_id"`
			Currency            *string `json:"currency"`
			Amount              *string `json:"amount"`
			UnitDurationSeconds *uint64 `json:"unit_duration_seconds"`
			Confirmed           *bool   `json:"confirmed"`
		}
		if !decodeParams(raw, &params) || params.PurchaseID == nil || params.OfferID == nil ||
			params.Currency == nil || params.Amount == nil || params.Confirmed == nil || !*params.Confirmed {
			return reply.invalid()
		}
		result, err := market.Purchase(ctx, StorePurchaseRequest{
			PurchaseID: *params.PurchaseID, OfferID: *params.OfferID, StoreID: params.StoreID,
			Currency: *params.Currency, Amount: *params.Amount,
			UnitDurationSeconds: params.UnitDurationSeconds, Confirmed: true,
		})
		if err != nil {
			return failStore(err)
		}
		return reply.ok(storePurchaseResultV1{SchemaVersion: 1, StorePurchaseResult: result})
	}
	return reply.fail(-32601, "Method not found")
}

// frameBudget leaves headroom under MaxFrameLen for the envelope.
const frameBudget = MaxFrameLen - 2048

func encodedLen(v any) int {
	b, err := json.Marshal(v)
	if err != nil {
		return frameBudget + 1
	}
	return len(b)
}

// fitPage drops trailing offers and rows until the page fits one control frame.
func fitPage(page StorePage) StorePage {
	for encodedLen(page) > frameBudget && len(page.Rows) > 0 {
		page.Truncated = true
		last := &page.Rows[len(page.Rows)-1]
		if len(last.Offers) > 1 {
			last.Offers = last.Offers[:len(last.Offers)-1]
		} else {
			page.Rows = page.Rows[:len(page.Rows)-1]
		}
	}
	return page
}

func fitSearch(r StoreSearchResults) StoreSearchResults {
	for encodedLen(r) > frameBudget && len(r.Offers) > 0 {
		r.Truncated = true
		r.Offers = r.Offers[:len(r.Offers)-1]
	}
	return r
}

func fitOwned(ids []string) []string {
	for encodedLen(ids) > frameBudget && len(ids) > 0 {
		ids = ids[:len(ids)/2]
	}
	return ids
}
