package control

import (
	"context"
	"encoding/json"
	"errors"
	"net"

	"github.com/hashimthearab/rust-mcbe/core/store"
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

// Marketplace is the Minecraft Marketplace service behind the store_* methods; *store.Session implements it.
type Marketplace interface {
	Home(ctx context.Context, page string) (store.Page, error)
	Search(ctx context.Context, q store.SearchQuery) (store.SearchResults, error)
	Offer(ctx context.Context, id string) (store.OfferDetail, error)
	Balances(ctx context.Context) ([]store.Balance, error)
	Entitlements(ctx context.Context, offset, limit int, refresh bool) (store.Entitlements, error)
	MoreOffers(ctx context.Context, token string) (store.RowMore, error)
	Purchase(ctx context.Context, r store.PurchaseRequest) (store.PurchaseResult, error)
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
	SchemaVersion uint32     `json:"schema_version"`
	Page          store.Page `json:"page"`
}

type storeSearchResultV1 struct {
	SchemaVersion uint32 `json:"schema_version"`
	store.SearchResults
}

type storeOfferResultV1 struct {
	SchemaVersion uint32            `json:"schema_version"`
	Offer         store.OfferDetail `json:"offer"`
}

type storeBalanceResultV1 struct {
	SchemaVersion uint32          `json:"schema_version"`
	Balances      []store.Balance `json:"balances"`
}

type storeEntitlementsResultV1 struct {
	SchemaVersion uint32 `json:"schema_version"`
	store.Entitlements
}

type storeRowMoreResultV1 struct {
	SchemaVersion uint32 `json:"schema_version"`
	store.RowMore
}

type storePurchaseResultV1 struct {
	SchemaVersion uint32 `json:"schema_version"`
	store.PurchaseResult
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
		case errors.Is(err, store.ErrInvalidRequest):
			return reply.invalid()
		case errors.Is(err, store.ErrPurchaseBusy):
			return reply.fail(codePurchaseBusy, "Purchase in progress")
		case errors.Is(err, store.ErrPurchaseReused):
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
			result.Rows = []store.Row{}
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
		result, err := market.Search(ctx, store.SearchQuery{
			Term: params.Term, Continuation: params.Continuation,
		})
		if err != nil {
			return failStore(err)
		}
		if result.Offers == nil {
			result.Offers = []store.Offer{}
		}
		result = fitSearch(result)
		return reply.ok(storeSearchResultV1{SchemaVersion: 1, SearchResults: result})
	case methodStoreOffer:
		var params struct {
			OfferID *string `json:"offer_id"`
		}
		if !decodeParams(raw, &params) || params.OfferID == nil || !store.ValidOfferID(*params.OfferID) {
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
			balances = []store.Balance{}
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
		return reply.ok(storeEntitlementsResultV1{SchemaVersion: 1, Entitlements: result})
	case methodStoreRowMore:
		var params struct {
			Continuation *string `json:"continuation"`
		}
		if !decodeParams(raw, &params) || params.Continuation == nil || !store.ValidContinuation(*params.Continuation) {
			return reply.invalid()
		}
		result, err := market.MoreOffers(ctx, *params.Continuation)
		if err != nil {
			return failStore(err)
		}
		if result.Offers == nil {
			result.Offers = []store.Offer{}
		}
		return reply.ok(storeRowMoreResultV1{SchemaVersion: 1, RowMore: result})
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
		result, err := market.Purchase(ctx, store.PurchaseRequest{
			PurchaseID: *params.PurchaseID, OfferID: *params.OfferID, StoreID: params.StoreID,
			Currency: *params.Currency, Amount: *params.Amount,
			UnitDurationSeconds: params.UnitDurationSeconds, Confirmed: true,
		})
		if err != nil {
			return failStore(err)
		}
		return reply.ok(storePurchaseResultV1{SchemaVersion: 1, PurchaseResult: result})
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
func fitPage(page store.Page) store.Page {
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

func fitSearch(r store.SearchResults) store.SearchResults {
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
