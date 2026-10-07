package store

import (
	"errors"
	"regexp"
)

var (
	// ErrInvalidRequest is returned for parameters that fail validation before any network call.
	ErrInvalidRequest = errors.New("store: invalid request")
	// ErrPurchaseBusy is returned while a purchase of the same offer is in flight or unresolved.
	ErrPurchaseBusy = errors.New("store: purchase in progress")
	// ErrPurchaseReused is returned when a purchase_id is replayed with different parameters.
	ErrPurchaseReused = errors.New("store: purchase id reused")
)

// Price is one currency amount an offer costs; Currency is the service's own identifier.
type Price struct {
	Currency string `json:"currency"`
	Amount   int64  `json:"amount"`
}

// Rating is an offer's aggregate star rating.
type Rating struct {
	Average float64 `json:"average"`
	Count   int     `json:"count"`
}

// Offer is one store listing as shown in a row, grid or search result.
type Offer struct {
	ID           string   `json:"id"`
	Title        string   `json:"title"`
	Creator      string   `json:"creator,omitempty"`
	ContentType  string   `json:"content_type,omitempty"`
	ThumbnailURL string   `json:"thumbnail_url,omitempty"`
	StoreID      string   `json:"store_id,omitempty"`
	Prices       []Price  `json:"prices,omitempty"`
	Rating       *Rating  `json:"rating,omitempty"`
	Tags         []string `json:"tags,omitempty"`
	Owned        bool     `json:"owned"`
}

// OfferDetail is an offer plus the fields only the detail screen shows.
type OfferDetail struct {
	Offer
	Description    string   `json:"description,omitempty"`
	ScreenshotURLs []string `json:"screenshot_urls,omitempty"`
	DisplayVersion string   `json:"display_version,omitempty"`
	Platforms      []string `json:"platforms,omitempty"`
}

// Row is one titled strip of offers on a store page.
type Row struct {
	ID           string  `json:"id,omitempty"`
	Title        string  `json:"title,omitempty"`
	Kind         string  `json:"kind,omitempty"`
	Offers       []Offer `json:"offers"`
	Continuation string  `json:"continuation,omitempty"` // token for row_more, when the row has more offers
}

// RowMore is the next slice of a row's offers.
type RowMore struct {
	Offers       []Offer `json:"offers"`
	Continuation string  `json:"continuation,omitempty"`
}

// Page is a server-driven store page reduced to the rows the client draws.
type Page struct {
	ID               string `json:"id"`
	Rows             []Row  `json:"rows"`
	InventoryVersion string `json:"inventory_version,omitempty"`
	Truncated        bool   `json:"truncated,omitempty"`
}

// SearchQuery is a store search, or the continuation of one.
type SearchQuery struct {
	Term         string
	Continuation string
}

// SearchResults is one page of search results.
type SearchResults struct {
	Offers       []Offer `json:"offers"`
	Continuation string  `json:"continuation,omitempty"`
	Truncated    bool    `json:"truncated,omitempty"`
}

// Balance is one virtual currency balance.
type Balance struct {
	Currency string `json:"currency"`
	Amount   int64  `json:"amount"`
}

// Entitlements is a window of the account's owned content ids.
type Entitlements struct {
	Owned            []string `json:"owned"`
	Total            int      `json:"total"`
	Offset           int      `json:"offset"`
	InventoryVersion string   `json:"inventory_version,omitempty"`
}

// PurchaseRequest is one Minecoin purchase; Confirmed must carry the player's explicit confirmation
// of exactly the offer, currency and amount shown.
type PurchaseRequest struct {
	PurchaseID          string // client-generated idempotency key
	OfferID             string
	StoreID             string
	Currency            string
	Amount              string // decimal digits
	UnitDurationSeconds *uint64
	Confirmed           bool
}

// Purchase outcomes; the status follows the service's HTTP outcome.
const (
	PurchaseOK           = "purchased"
	PurchasePriceRefused = "price_mismatch"      // HTTP 422
	PurchaseStaleState   = "precondition_failed" // HTTP 412
	PurchaseFailed       = "failed"
	PurchaseUnknown      = "unknown" // no definitive answer; refresh balance and inventory before retrying
)

// PurchaseResult is the recorded outcome of one purchase attempt.
type PurchaseResult struct {
	Status               string `json:"status"`
	HTTPStatus           int    `json:"http_status,omitempty"`
	MarketplaceErrorCode int    `json:"marketplace_error_code,omitempty"`
	CorrelationID        string `json:"correlation_id"`
	InventoryVersion     string `json:"inventory_version,omitempty"`
	Replayed             bool   `json:"replayed,omitempty"`
}

var (
	idPattern       = regexp.MustCompile(`^[A-Za-z0-9._:-]{1,128}$`)
	purchaseIDRegex = regexp.MustCompile(`^[A-Za-z0-9-]{16,64}$`)
	amountPattern   = regexp.MustCompile(`^[1-9][0-9]{0,9}$`)
)

// Validate reports ErrInvalidRequest unless every field is well formed and Confirmed is set.
func (r PurchaseRequest) Validate() error {
	ok := r.Confirmed &&
		purchaseIDRegex.MatchString(r.PurchaseID) &&
		idPattern.MatchString(r.OfferID) &&
		(r.StoreID == "" || idPattern.MatchString(r.StoreID)) &&
		idPattern.MatchString(r.Currency) &&
		amountPattern.MatchString(r.Amount) &&
		(r.UnitDurationSeconds == nil || *r.UnitDurationSeconds > 0)
	if !ok {
		return ErrInvalidRequest
	}
	return nil
}

// Validate reports ErrInvalidRequest for an unusable search.
func (q SearchQuery) Validate() error {
	if len(q.Term) > 200 || len(q.Continuation) > 2048 {
		return ErrInvalidRequest
	}
	return nil
}

// ValidContinuation reports whether token is a usable continuation token.
func ValidContinuation(token string) bool { return token != "" && len(token) <= 2048 }

// ValidOfferID reports whether id is a well-formed offer identifier.
func ValidOfferID(id string) bool { return idPattern.MatchString(id) }
