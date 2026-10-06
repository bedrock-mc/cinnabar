package store

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/marketplace"
)

type fakeTokens struct{}

func (fakeTokens) ServiceToken(context.Context) (*service.Token, error) {
	return &service.Token{AuthorizationHeader: "MCToken synthetic", ValidUntil: time.Now().Add(time.Hour)}, nil
}

// newTestClient serves distinct store and entitlements origins and returns the entitlements server.
func newTestClient(t *testing.T, handler http.HandlerFunc) (*Client, *httptest.Server) {
	t.Helper()
	// serve rejects requests routed to the wrong service before invoking the test's handler.
	serve := func(entitlements bool) *httptest.Server {
		server := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			var needsEntitlements bool
			switch r.URL.Path {
			case "/api/v1.0/player/inventory", "/api/v1.0/currencies/virtual/balances", "/api/v1.0/transaction/virtual":
				needsEntitlements = true
			}
			if entitlements != needsEntitlements {
				t.Errorf("%s routed to wrong service: entitlements=%t", r.URL.Path, entitlements)
				w.WriteHeader(http.StatusNotFound)
				return
			}
			handler.ServeHTTP(w, r)
		}))
		t.Cleanup(server.Close)
		return server
	}
	storeServer, entitlementsServer := serve(false), serve(true)
	env := new(marketplace.Environment)
	if err := json.Unmarshal([]byte(`{"serviceUri":"`+storeServer.URL+`"}`), env); err != nil {
		t.Fatal(err)
	}
	env.HTTPClient = storeServer.Client()
	entitlementsEnv := new(marketplace.EntitlementsEnvironment)
	if err := json.Unmarshal([]byte(`{"serviceUri":"`+entitlementsServer.URL+`"}`), entitlementsEnv); err != nil {
		t.Fatal(err)
	}
	entitlementsEnv.HTTPClient = entitlementsServer.Client()
	market, err := env.New(fakeTokens{}, entitlementsEnv)
	if err != nil {
		t.Fatal(err)
	}
	client, err := NewClient(Config{Market: market, Identity: Identity{XUID: "2535", TitleID: "20CA2"}})
	if err != nil {
		t.Fatal(err)
	}
	return client, entitlementsServer
}

// Authored to vanilla's store inventory format; not a captured payload.
const inventoryFixture = `{"result":{"inventory":{"entitlements":[{"id":"AAAAAAAA-0000-0000-0000-000000000001"},
{"id":"bbbbbbbb-0000-0000-0000-000000000002"},{"id":"aaaaaaaa-0000-0000-0000-000000000001"},{"id":"bad id"}]},"receipt":"e30="}}`

func TestBalancesMapTheServiceTypes(t *testing.T) {
	client, _ := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.WriteString(w, `{"result":{"virtualCurrencyBalances":[{"type":"Minecoin","amount":1500},{"type":"PlayStationToken","amount":7},{"amount":3}]}}`)
	})
	balances, err := client.Balances(context.Background())
	if err != nil || len(balances) != 2 || balances[0] != (Balance{"Minecoin", 1500}) || balances[1].Amount != 7 {
		t.Fatalf("balances = %+v err=%v", balances, err)
	}
}

func TestEntitlementsAreDedupedAndPaged(t *testing.T) {
	client, _ := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("InventoryETag", "etag-1")
		_, _ = io.WriteString(w, inventoryFixture)
	})
	got, err := client.Entitlements(context.Background(), 0, 1, false)
	if err != nil || got.Total != 2 || len(got.Owned) != 1 || got.Owned[0] != "aaaaaaaa-0000-0000-0000-000000000001" || got.InventoryVersion != "etag-1" {
		t.Fatalf("entitlements = %+v err=%v", got, err)
	}
	if _, err := client.Entitlements(context.Background(), -1, 0, false); !errors.Is(err, ErrInvalidRequest) {
		t.Fatalf("negative offset err = %v", err)
	}
}

// Synthesized in the live page shape: a curated row lists its offers inline under a header; a row
// with queries and no inline offers is not drawn.
const pageFixture = `{"result":{"pageId":"page-1","layout":[{"sectionName":"rows","rows":[
{"controlId":"Layout","components":[{"type":"topBarSearchComp","isVisible":true}],"queries":null},
{"telemetryId":"r0","controlId":"StoreRow","components":[{"type":"itemListComp","totalItems":9,
 "items":[{"id":"BBBBBBBB-0000-0000-0000-000000000002","title":"Curated","creatorName":"Maker","rating":{"average":4.5,"totalCount":3},
  "thumbnail":{"type":"Thumbnail","url":"https://cdn.example.test/c.png"},"price":{"listPrice":990,"currencyId":"coin"}}]},
 {"type":"carouselComp"},{"type":"headerComp","headerText":"Featured"}],"queries":null},
{"telemetryId":"r1","controlId":"StoreRow","components":[{"type":"itemListComp"},{"type":"headerComp","headerText":"New"}],
 "queries":[{"queryContentTypes":["Durable"],"orTags":["new"],"itemLimit":10}]}]}]}}`

// Curated rows map their inline offers, marked owned, with the owned ids and versions sent along.
func TestHomeMapsCuratedRows(t *testing.T) {
	var body marketplace.PageRequest
	client, _ := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/v1.0/session/config":
			_, _ = io.WriteString(w, `{"result":{"knownPages":{"storeRoot":"page-1"},"userListsVersion":"lists-1"}}`)
		case "/api/v2.0/layout/pages/page-1":
			_ = json.NewDecoder(r.Body).Decode(&body)
			w.Header().Set("InventoryETag", "etag-2")
			_, _ = io.WriteString(w, pageFixture)
		default:
			w.Header().Set("InventoryETag", "etag-1")
			_, _ = io.WriteString(w, inventoryFixture)
		}
	})
	page, err := client.Home(context.Background(), marketplace.PageStoreRoot)
	if err != nil || len(page.Rows) != 1 || page.InventoryVersion != "etag-2" {
		t.Fatalf("page = %+v err=%v", page, err)
	}
	curated := page.Rows[0]
	if curated.Title != "Featured" || curated.Kind != "StoreRow" || len(curated.Offers) != 1 {
		t.Fatalf("curated row = %+v", curated)
	}
	offer := curated.Offers[0]
	if offer.ID != "bbbbbbbb-0000-0000-0000-000000000002" || offer.Title != "Curated" || offer.ThumbnailURL == "" ||
		offer.Rating == nil || offer.Rating.Count != 3 || offer.Prices[0].Amount != 990 || !offer.Owned {
		t.Fatalf("curated offer = %+v", offer)
	}
	if len(body.Entitlements) != 2 || body.InventoryVersion != "etag-1" || body.ListVersion != "lists-1" {
		t.Fatalf("page body = %+v", body)
	}
}

// A page name the session config does not map fails before any layout request.
func TestHomeRefusesAPageTheSessionConfigDoesNotKnow(t *testing.T) {
	var layoutRequests int
	client, _ := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.URL.Path == "/api/v1.0/session/config":
			_, _ = io.WriteString(w, `{"result":{"knownPages":{"inventory":"inv-1","coinScreen":"coin-1"}}}`)
		case strings.HasPrefix(r.URL.Path, "/api/v2.0/layout/pages/"):
			layoutRequests++
			w.WriteHeader(http.StatusBadRequest)
		default:
			_, _ = io.WriteString(w, inventoryFixture)
		}
	})
	if _, err := client.Home(context.Background(), marketplace.PageStoreRoot); !errors.Is(err, marketplace.ErrUnknownPage) {
		t.Fatalf("err = %v, want ErrUnknownPage", err)
	}
	if layoutRequests != 0 {
		t.Fatalf("sent %d layout requests for an unknown page", layoutRequests)
	}
}

// Synthesized in the live search and detail page shapes, not captured payloads.
const (
	searchPageFixture = `{"result":{"pageId":"Search_SearchResults","layout":[{"sectionName":"rows","rows":[
{"controlId":"GridList","components":[{"type":"pagedItemListComp","totalItems":785,"continuationToken":"more",
 "items":[{"id":"AAAAAAAA-0000-0000-0000-000000000001","title":"Alpha","creatorName":"Studio","rating":{"average":4.5,"totalCount":10},
  "thumbnail":{"type":"Thumbnail","url":"https://cdn.example.test/t.png"},"price":{"listPrice":320,"currencyId":"mc"}},
  {"id":"untitled"}]}]}]}]}}`
	detailPageFixture = `{"result":{"pageId":"ItemDetail_x","layout":[{"sectionName":"rows","rows":[
{"controlId":"ItemSummary","components":[{"type":"itemSummaryComp","item":{"id":"aaaaaaaa-0000-0000-0000-000000000001","title":"Alpha",
 "creatorName":"Studio","packIdentity":[{"type":"worldtemplate","uuid":"u","version":"1.0.2"}],"tags":[{"name":"Castle"}]}},
 {"type":"purchaseInfoComp","price":{"listPrice":660,"currencyId":"mc"}}]},
{"controlId":"ItemDescription","components":[{"type":"itemDescriptionComp","description":"A castle."}]},
{"controlId":"ImageGallery","components":[{"type":"imageGalleryComp","images":[{"type":"Unknown","url":"https://cdn.example.test/s0.jpg"},{"url":"http://insecure.test/s.jpg"}]}]},
{"controlId":"RatingRow","components":[{"type":"ratingComp","rating":{"average":4.0,"totalCount":102}}]}]}]}}`
)

// Search renders the store's search page and continues it through row continuation; offers are
// marked owned from the inventory. The detail screen reads the offer's detail page.
func TestSearchAndOfferUseTheStoreLayoutPages(t *testing.T) {
	var searched map[string]any
	client, _ := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/v1.0/session/config":
			_, _ = io.WriteString(w, `{"result":{"knownPages":{"searchResults":"results-1"}}}`)
		case "/api/v2.0/layout/pages/results-1":
			_ = json.NewDecoder(r.Body).Decode(&searched)
			_, _ = io.WriteString(w, searchPageFixture)
		case "/api/v2.0/layout/items":
			_, _ = io.WriteString(w, `{"continuationToken":"","result":[{"id":"bbbbbbbb-0000-0000-0000-000000000002","title":"Beta"}]}`)
		case "/api/v2.0/layout/pages/productId/aaaaaaaa-0000-0000-0000-000000000001":
			_, _ = io.WriteString(w, detailPageFixture)
		default:
			_, _ = io.WriteString(w, inventoryFixture)
		}
	})
	results, err := client.Search(context.Background(), SearchQuery{Term: "castle"})
	if err != nil || searched["search"] != "castle" || len(results.Offers) != 1 || results.Continuation != "more" {
		t.Fatalf("results = %+v body = %v err = %v", results, searched, err)
	}
	offer := results.Offers[0]
	if !offer.Owned || offer.Creator != "Studio" || offer.ThumbnailURL != "https://cdn.example.test/t.png" ||
		offer.Prices[0] != (Price{"mc", 320}) || offer.Rating == nil || offer.Rating.Count != 10 {
		t.Fatalf("offer = %+v", offer)
	}
	more, err := client.Search(context.Background(), SearchQuery{Continuation: "more"})
	if err != nil || len(more.Offers) != 1 || !more.Offers[0].Owned || more.Continuation != "" {
		t.Fatalf("continued = %+v err = %v", more, err)
	}
	detail, err := client.Offer(context.Background(), "aaaaaaaa-0000-0000-0000-000000000001")
	if err != nil || !detail.Owned || detail.Description != "A castle." || detail.Prices[0] != (Price{"mc", 660}) ||
		len(detail.ScreenshotURLs) != 1 || detail.DisplayVersion != "1.0.2" || detail.Tags[0] != "Castle" ||
		detail.Rating == nil || detail.Rating.Count != 102 {
		t.Fatalf("detail = %+v err = %v", detail, err)
	}
}

type purchaseServer struct {
	calls     atomic.Int32
	refreshes atomic.Int32
	bodies    []map[string]any
	mu        sync.Mutex
	status    int
	header    map[string]string
	gate      chan struct{}
}

func (s *purchaseServer) handler(t *testing.T) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/v1.0/inventory/refresh" {
			s.refreshes.Add(1)
			_, _ = io.WriteString(w, `{"result":{"version":"v2"}}`)
			return
		}
		if r.URL.Path != "/api/v1.0/transaction/virtual" || r.Method != http.MethodPost {
			t.Errorf("unexpected %s %s", r.Method, r.URL.Path)
		}
		s.calls.Add(1)
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		s.mu.Lock()
		s.bodies = append(s.bodies, body)
		s.mu.Unlock()
		if s.gate != nil {
			<-s.gate
		}
		for k, v := range s.header {
			w.Header().Set(k, v)
		}
		w.WriteHeader(s.status)
	}
}

func request(id, offer string) PurchaseRequest {
	return PurchaseRequest{PurchaseID: id, OfferID: offer, StoreID: "store-1", Currency: "mc", Amount: "320", Confirmed: true}
}

func TestPurchaseSendsTheVanillaRequestShape(t *testing.T) {
	server := &purchaseServer{status: 200, header: map[string]string{"InventoryETag": "etag-2"}}
	client, _ := newTestClient(t, server.handler(t))
	res, err := client.Purchase(context.Background(), request("purchase-0000000001", "offer-1"))
	if err != nil || res.Status != PurchaseOK || res.InventoryVersion != "etag-2" || res.CorrelationID == "" {
		t.Fatalf("result = %+v err=%v", res, err)
	}
	raw, _ := json.Marshal(server.bodies[0])
	for _, want := range []string{
		`"VirtualCurrency":{"Amount":"320","Type":"Minecoin"}`, `"OfferId":"offer-1"`, `"StoreId":"store-1"`,
		`"TitleId":"20CA2"`, `"BuildPlat":7`, `"Xuid":"2535"`, `"Seq":1`, `"CorrelationId":"` + res.CorrelationID + `"`,
	} {
		if !strings.Contains(string(raw), want) {
			t.Errorf("body %s lacks %s", raw, want)
		}
	}
	if strings.Contains(string(raw), "UnitDurationInSeconds") {
		t.Errorf("body carries a duration it was not given: %s", raw)
	}
}

func TestPurchaseIsIdempotentPerID(t *testing.T) {
	server := &purchaseServer{status: 200}
	client, _ := newTestClient(t, server.handler(t))
	req := request("purchase-0000000002", "offer-2")
	first, err := client.Purchase(context.Background(), req)
	if err != nil || first.Replayed {
		t.Fatalf("first = %+v err=%v", first, err)
	}
	again, err := client.Purchase(context.Background(), req)
	if err != nil || !again.Replayed || again.CorrelationID != first.CorrelationID || server.calls.Load() != 1 {
		t.Fatalf("replay = %+v err=%v calls=%d", again, err, server.calls.Load())
	}
	changed := req
	changed.Amount = "1"
	if _, err := client.Purchase(context.Background(), changed); !errors.Is(err, ErrPurchaseReused) {
		t.Fatalf("changed replay err = %v", err)
	}
}

func TestPurchaseLocksTheOfferWhileInFlight(t *testing.T) {
	server := &purchaseServer{status: 200, gate: make(chan struct{})}
	client, _ := newTestClient(t, server.handler(t))
	done := make(chan error, 1)
	go func() {
		_, err := client.Purchase(context.Background(), request("purchase-0000000003", "offer-3"))
		done <- err
	}()
	deadline := time.Now().Add(5 * time.Second)
	for server.calls.Load() == 0 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	if _, err := client.Purchase(context.Background(), request("purchase-0000000004", "offer-3")); !errors.Is(err, ErrPurchaseBusy) {
		t.Fatalf("second purchase err = %v", err)
	}
	close(server.gate)
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	if server.calls.Load() != 1 {
		t.Fatalf("calls = %d", server.calls.Load())
	}
}

func TestPurchaseOutcomesFollowTheHTTPStatus(t *testing.T) {
	for status, want := range map[int]string{422: PurchasePriceRefused, 412: PurchaseStaleState, 500: PurchaseFailed, 400: PurchaseFailed} {
		server := &purchaseServer{status: status}
		client, _ := newTestClient(t, server.handler(t))
		res, err := client.Purchase(context.Background(), request("purchase-0000000005", "offer-5"))
		if err != nil || res.Status != want || res.HTTPStatus != status || server.calls.Load() != 1 {
			t.Fatalf("status %d: result = %+v err=%v calls=%d", status, res, err, server.calls.Load())
		}
		// A definitive refusal releases the offer for a fresh attempt.
		if _, err := client.Purchase(context.Background(), request("purchase-0000000006", "offer-5")); errors.Is(err, ErrPurchaseBusy) {
			t.Fatalf("status %d left the offer locked", status)
		}
	}
}

func TestPurchaseWithNoAnswerIsUnknownAndHoldsTheOffer(t *testing.T) {
	client, server := newTestClient(t, func(http.ResponseWriter, *http.Request) {})
	server.Close() // connection refused: the outcome cannot be known to have missed the service
	res, err := client.Purchase(context.Background(), request("purchase-0000000007", "offer-7"))
	if err != nil || res.Status != PurchaseUnknown {
		t.Fatalf("result = %+v err=%v", res, err)
	}
	if _, err := client.Purchase(context.Background(), request("purchase-0000000008", "offer-7")); !errors.Is(err, ErrPurchaseBusy) {
		t.Fatalf("retry of an unknown outcome err = %v", err)
	}
}

func TestPurchaseValidationRefusesUnconfirmedOrMalformedRequests(t *testing.T) {
	server := &purchaseServer{status: 200}
	client, _ := newTestClient(t, server.handler(t))
	base := request("purchase-0000000009", "offer-9")
	mutations := map[string]func(*PurchaseRequest){
		"unconfirmed": func(r *PurchaseRequest) { r.Confirmed = false },
		"zero amount": func(r *PurchaseRequest) { r.Amount = "0" },
		"negative":    func(r *PurchaseRequest) { r.Amount = "-5" },
		"decimal":     func(r *PurchaseRequest) { r.Amount = "3.5" },
		"short id":    func(r *PurchaseRequest) { r.PurchaseID = "abc" },
		"bad offer":   func(r *PurchaseRequest) { r.OfferID = "a b" },
		"no currency": func(r *PurchaseRequest) { r.Currency = "" },
	}
	for name, mutate := range mutations {
		r := base
		mutate(&r)
		if _, err := client.Purchase(context.Background(), r); !errors.Is(err, ErrInvalidRequest) {
			t.Errorf("%s: err = %v", name, err)
		}
	}
	if server.calls.Load() != 0 {
		t.Fatalf("invalid purchases reached the service %d times", server.calls.Load())
	}
}

func TestASuccessfulPurchaseRefreshesTheInventoryOnce(t *testing.T) {
	server := &purchaseServer{status: 200}
	client, _ := newTestClient(t, server.handler(t))
	if _, err := client.Purchase(context.Background(), request("purchase-0000000010", "offer-10")); err != nil {
		t.Fatal(err)
	}
	if server.refreshes.Load() != 1 {
		t.Fatalf("refreshes = %d", server.refreshes.Load())
	}
	refused := &purchaseServer{status: 422}
	other, _ := newTestClient(t, refused.handler(t))
	if _, err := other.Purchase(context.Background(), request("purchase-0000000011", "offer-11")); err != nil || refused.refreshes.Load() != 0 {
		t.Fatalf("refused purchase refreshed %d times, err=%v", refused.refreshes.Load(), err)
	}
}

func TestMoreOffersMapsCatalogItemsAndMarksOwnership(t *testing.T) {
	var body map[string]string
	client, _ := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/v2.0/layout/items" {
			_ = json.NewDecoder(r.Body).Decode(&body)
			_, _ = io.WriteString(w, `{"continuationToken":"t2","result":[{"id":"AAAAAAAA-0000-0000-0000-000000000001","title":{"neutral":"Alpha"},"price":{"listPrice":990,"currencyId":"Minecoin","saleInfo":{"salePrice":490}}},{"id":"zzz","title":{"neutral":"Zed"}}]}`)
			return
		}
		w.Header().Set("InventoryETag", "etag-5")
		_, _ = io.WriteString(w, inventoryFixture)
	})
	more, err := client.MoreOffers(context.Background(), "t1")
	if err != nil || body["continuationToken"] != "t1" || body["inventoryVersion"] != "etag-5" {
		t.Fatalf("body = %+v err=%v", body, err)
	}
	if len(more.Offers) != 2 || !more.Offers[0].Owned || more.Offers[1].Owned || more.Continuation != "t2" || more.Offers[0].Prices[0].Amount != 490 {
		t.Fatalf("more = %+v", more)
	}
	if _, err := client.MoreOffers(context.Background(), ""); !errors.Is(err, ErrInvalidRequest) {
		t.Fatalf("empty token err = %v", err)
	}
}

func TestEntitlementsRefreshAsksTheServiceFirst(t *testing.T) {
	var refreshes atomic.Int32
	client, _ := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/v1.0/inventory/refresh" {
			refreshes.Add(1)
			_, _ = io.WriteString(w, `{"result":{"version":"v2"}}`)
			return
		}
		_, _ = io.WriteString(w, inventoryFixture)
	})
	if _, err := client.Entitlements(context.Background(), 0, 0, true); err != nil || refreshes.Load() != 1 {
		t.Fatalf("refreshes = %d err=%v", refreshes.Load(), err)
	}
	if _, err := client.Entitlements(context.Background(), 0, 0, false); err != nil || refreshes.Load() != 1 {
		t.Fatalf("a cached read refreshed: %d err=%v", refreshes.Load(), err)
	}
}

// An offer on a running free sale is quoted at zero, not its list price.
func TestOffersOnAFreeSaleCostNothing(t *testing.T) {
	var item marketplace.Item
	if err := json.Unmarshal([]byte(`{"id":"aaaaaaaa-0000-0000-0000-000000000001","title":"Alpha",
"price":{"listPrice":990,"currencyId":"mc","saleInfo":{"salePrice":0}}}`), &item); err != nil {
		t.Fatal(err)
	}
	offer, ok := offerFromMarketItem(&item)
	if !ok || len(offer.Prices) != 1 || offer.Prices[0] != (Price{"mc", 0}) {
		t.Fatalf("offer = %+v ok = %v", offer, ok)
	}
}

// A search returns every offer of the service's page, so its continuation skips none.
func TestSearchKeepsTheWholeServicePage(t *testing.T) {
	var items []string
	for i := range 51 {
		items = append(items, fmt.Sprintf(`{"id":"aaaaaaaa-0000-0000-0000-%012d","title":"Offer %d"}`, i, i))
	}
	page := `{"result":{"pageId":"Search_SearchResults","layout":[{"sectionName":"rows","rows":[{"controlId":"GridList",
"components":[{"type":"pagedItemListComp","continuationToken":"more","items":[` + strings.Join(items, ",") + `]}]}]}]}}`
	client, _ := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/v1.0/session/config":
			_, _ = io.WriteString(w, `{"result":{"knownPages":{"searchResults":"results-1"}}}`)
		case "/api/v2.0/layout/pages/results-1":
			_, _ = io.WriteString(w, page)
		default:
			_, _ = io.WriteString(w, inventoryFixture)
		}
	})
	results, err := client.Search(context.Background(), SearchQuery{Term: "x"})
	if err != nil || len(results.Offers) != 51 || results.Continuation != "more" || results.Truncated {
		t.Fatalf("offers = %d continuation = %q truncated = %v err = %v", len(results.Offers), results.Continuation, results.Truncated, err)
	}
}
