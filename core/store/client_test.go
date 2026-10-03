package store

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/df-mc/go-playfab/v2"
	playfabcatalog "github.com/df-mc/go-playfab/v2/catalog"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/marketplace"
)

type fakeTokens struct{}

func (fakeTokens) ServiceToken(context.Context) (*service.Token, error) {
	return &service.Token{AuthorizationHeader: "MCToken synthetic", ValidUntil: time.Now().Add(time.Hour)}, nil
}

type fakeCatalog struct {
	mu    sync.Mutex
	items []playfabcatalog.Item
	got   []playfabcatalog.SearchFilter
	err   error
}

func (f *fakeCatalog) SearchItems(_ context.Context, filter playfabcatalog.SearchFilter, _ ...playfab.RequestOption) (*playfabcatalog.SearchResult, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.got = append(f.got, filter)
	if f.err != nil {
		return nil, f.err
	}
	return &playfabcatalog.SearchResult{Items: f.items, ContinuationToken: "next"}, nil
}

func (f *fakeCatalog) ItemByID(_ context.Context, id string, _ ...playfab.RequestOption) (*playfabcatalog.Item, error) {
	for i := range f.items {
		if f.items[i].ID == id {
			return &f.items[i], nil
		}
	}
	return nil, errors.New("missing")
}

// newTestClient serves distinct store and entitlements origins and returns the entitlements server.
func newTestClient(t *testing.T, handler http.HandlerFunc, cat Catalog) (*Client, *httptest.Server) {
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
	if cat == nil {
		cat = &fakeCatalog{}
	}
	client, err := NewClient(Config{Market: market, Catalog: cat, Identity: Identity{XUID: "2535", TitleID: "20CA2"}})
	if err != nil {
		t.Fatal(err)
	}
	return client, entitlementsServer
}

// Authored to the reference client's inventory parser; not a captured payload.
const inventoryFixture = `{"result":{"inventory":{"entitlements":[{"id":"AAAAAAAA-0000-0000-0000-000000000001"},
{"id":"bbbbbbbb-0000-0000-0000-000000000002"},{"id":"aaaaaaaa-0000-0000-0000-000000000001"},{"id":"bad id"}]},"receipt":"e30="}}`

func TestBalancesMapTheServiceTypes(t *testing.T) {
	client, _ := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.WriteString(w, `{"result":{"virtualCurrencyBalances":[{"type":"Minecoin","amount":1500},{"type":"PlayStationToken","amount":7},{"amount":3}]}}`)
	}, nil)
	balances, err := client.Balances(context.Background())
	if err != nil || len(balances) != 2 || balances[0] != (Balance{"Minecoin", 1500}) || balances[1].Amount != 7 {
		t.Fatalf("balances = %+v err=%v", balances, err)
	}
}

func TestEntitlementsAreDedupedAndPaged(t *testing.T) {
	client, _ := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("InventoryETag", "etag-1")
		_, _ = io.WriteString(w, inventoryFixture)
	}, nil)
	got, err := client.Entitlements(context.Background(), 0, 1, false)
	if err != nil || got.Total != 2 || len(got.Owned) != 1 || got.Owned[0] != "aaaaaaaa-0000-0000-0000-000000000001" || got.InventoryVersion != "etag-1" {
		t.Fatalf("entitlements = %+v err=%v", got, err)
	}
	if _, err := client.Entitlements(context.Background(), -1, 0, false); !errors.Is(err, ErrInvalidRequest) {
		t.Fatalf("negative offset err = %v", err)
	}
}

// Authored to the reference client's page parser; rows carry queries, not offers.
const pageFixture = `{"result":{"pageId":"page-1","layout":[{"sectionName":"New","rows":[
{"telemetryId":"r1","controlId":"StoreRow","components":[{"type":"itemListComp"}],"queries":[{"queryContentTypes":["Durable"],"orTags":["new"],"itemLimit":10}]},
{"telemetryId":"r2","queries":[{"rarityFilters":["epic"]}]}]}]}}`

// A page row is filled by running its query through the catalog, with the owned ids sent along.
func TestHomeFillsRowsFromTheirQueries(t *testing.T) {
	cat := &fakeCatalog{items: []playfabcatalog.Item{{ID: "AAAAAAAA-0000-0000-0000-000000000001", Title: playfabcatalog.Dictionary[string]{"NEUTRAL": "Alpha"}}}}
	var body marketplace.PageRequest
	client, _ := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/v1.0/session/config":
			_, _ = io.WriteString(w, `{"result":{"knownPages":{"home":"page-1"},"userListsVersion":"lists-1"}}`)
		case "/api/v2.0/layout/pages/page-1":
			_ = json.NewDecoder(r.Body).Decode(&body)
			w.Header().Set("InventoryETag", "etag-2")
			_, _ = io.WriteString(w, pageFixture)
		default:
			w.Header().Set("InventoryETag", "etag-1")
			_, _ = io.WriteString(w, inventoryFixture)
		}
	}, cat)
	page, err := client.Home(context.Background(), "home")
	if err != nil || len(page.Rows) != 1 || page.InventoryVersion != "etag-2" {
		t.Fatalf("page = %+v err=%v", page, err)
	}
	row := page.Rows[0]
	if row.Title != "New" || row.Kind != "itemListComp" || len(row.Offers) != 1 || !row.Offers[0].Owned {
		t.Fatalf("row = %+v", row)
	}
	if len(body.Entitlements) != 2 || body.InventoryVersion != "etag-1" || body.ListVersion != "lists-1" {
		t.Fatalf("page body = %+v", body)
	}
	if len(cat.got) != 1 || cat.got[0].Filter != "ContentType eq 'Durable' and Tags/any(t: t eq 'new')" || cat.got[0].Count != 10 {
		t.Fatalf("searches = %+v", cat.got)
	}
	failing, _ := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		if strings.HasPrefix(r.URL.Path, "/api/v2.0/layout/pages/") {
			_, _ = io.WriteString(w, pageFixture)
			return
		}
		_, _ = io.WriteString(w, `{"result":{"knownPages":{}}}`)
	}, &fakeCatalog{err: errors.New("catalog down")})
	failing.inventory = &inventoryCache{set: map[string]struct{}{}, at: time.Now()}
	if _, err := failing.Home(context.Background(), "home"); err == nil {
		t.Fatal("a page whose every row search failed was served")
	}
}

func TestSearchMapsCatalogItemsAndMarksOwned(t *testing.T) {
	cat := &fakeCatalog{items: []playfabcatalog.Item{
		{
			ID: "aaaaaaaa-0000-0000-0000-000000000001", ContentType: "MarketplaceDurableCatalog_V1.2",
			Title:             playfabcatalog.Dictionary[string]{"NEUTRAL": "Alpha"},
			DisplayProperties: json.RawMessage(`{"creatorName":"Studio"}`),
			Images:            []playfabcatalog.Image{{Type: "screenshot", URL: "https://cdn.example.test/s.png"}, {Type: "Thumbnail", URL: "https://cdn.example.test/t.png"}},
			PriceOptions:      playfabcatalog.PriceOptions{{Amounts: []playfabcatalog.PriceAmount{{Value: 320, ItemID: "mc"}}}},
			Rating:            playfabcatalog.Rating{Average: 4.5, TotalCount: 10},
		},
		{ID: "hidden", Hidden: true, Title: playfabcatalog.Dictionary[string]{"NEUTRAL": "H"}},
		{ID: "untitled"},
	}}
	client, _ := newTestClient(t, func(w http.ResponseWriter, r *http.Request) { _, _ = io.WriteString(w, inventoryFixture) }, cat)
	results, err := client.Search(context.Background(), SearchQuery{Term: "castle"})
	if err != nil || len(results.Offers) != 1 || results.Continuation != "next" {
		t.Fatalf("results = %+v err=%v", results, err)
	}
	offer := results.Offers[0]
	if !offer.Owned || offer.Creator != "Studio" || offer.ThumbnailURL != "https://cdn.example.test/t.png" ||
		offer.Prices[0] != (Price{"mc", 320}) || offer.Rating == nil || offer.Rating.Count != 10 {
		t.Fatalf("offer = %+v", offer)
	}
	if cat.got[0].Count != defaultSearchCount || cat.got[0].Term != "castle" {
		t.Fatalf("filter = %+v", cat.got[0])
	}
	detail, err := client.Offer(context.Background(), "aaaaaaaa-0000-0000-0000-000000000001")
	if err != nil || len(detail.ScreenshotURLs) != 1 || !detail.Owned {
		t.Fatalf("detail = %+v err=%v", detail, err)
	}
	if _, err := client.Search(context.Background(), SearchQuery{Filter: "a;drop"}); !errors.Is(err, ErrInvalidRequest) {
		t.Fatalf("bad filter err = %v", err)
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
	client, _ := newTestClient(t, server.handler(t), nil)
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
	client, _ := newTestClient(t, server.handler(t), nil)
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
	client, _ := newTestClient(t, server.handler(t), nil)
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
		client, _ := newTestClient(t, server.handler(t), nil)
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
	client, server := newTestClient(t, func(http.ResponseWriter, *http.Request) {}, nil)
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
	client, _ := newTestClient(t, server.handler(t), nil)
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
	client, _ := newTestClient(t, server.handler(t), nil)
	if _, err := client.Purchase(context.Background(), request("purchase-0000000010", "offer-10")); err != nil {
		t.Fatal(err)
	}
	if server.refreshes.Load() != 1 {
		t.Fatalf("refreshes = %d", server.refreshes.Load())
	}
	refused := &purchaseServer{status: 422}
	other, _ := newTestClient(t, refused.handler(t), nil)
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
	}, nil)
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
	}, nil)
	if _, err := client.Entitlements(context.Background(), 0, 0, true); err != nil || refreshes.Load() != 1 {
		t.Fatalf("refreshes = %d err=%v", refreshes.Load(), err)
	}
	if _, err := client.Entitlements(context.Background(), 0, 0, false); err != nil || refreshes.Load() != 1 {
		t.Fatalf("a cached read refreshed: %d err=%v", refreshes.Load(), err)
	}
}

// Price options the purchase flow cannot express are refused, never flattened into separate prices.
func TestOffersKeepOnlyWholeSingleCurrencyPrices(t *testing.T) {
	item := func(options ...playfabcatalog.Price) *playfabcatalog.Item {
		return &playfabcatalog.Item{ID: "offer-1", Title: map[string]string{"NEUTRAL": "Pack"}, PriceOptions: options}
	}
	mc := func(value int) playfabcatalog.PriceAmount {
		return playfabcatalog.PriceAmount{Value: value, ItemID: "mc"}
	}
	combined := playfabcatalog.Price{Amounts: []playfabcatalog.PriceAmount{mc(100), {Value: 5, ItemID: "tokens"}}}
	timed := playfabcatalog.Price{Amounts: []playfabcatalog.PriceAmount{mc(100)}, UnitDurationInSeconds: 86400}
	bulk := playfabcatalog.Price{Amounts: []playfabcatalog.PriceAmount{mc(100)}, UnitAmount: 5}
	single := playfabcatalog.Price{Amounts: []playfabcatalog.PriceAmount{mc(320)}}

	offer, ok := offerFromItem(item(combined, single, timed))
	if !ok || len(offer.Prices) != 1 || offer.Prices[0] != (Price{Currency: "mc", Amount: 320}) {
		t.Fatalf("offer = %+v ok = %v", offer, ok)
	}
	for name, option := range map[string]playfabcatalog.Price{"combined": combined, "timed": timed, "bulk": bulk} {
		if _, ok := offerFromItem(item(option)); ok {
			t.Fatalf("%s-only offer was listed", name)
		}
	}
	if offer, ok := offerFromItem(item()); !ok || len(offer.Prices) != 0 {
		t.Fatalf("an unpriced offer = %+v ok = %v", offer, ok)
	}
}
