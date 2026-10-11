package control

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"sync"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/service/marketplace"
)

type stubMarket struct {
	page     StorePage
	purchase StorePurchaseRequest
	result   StorePurchaseResult
	err      error
	homeArg  string
	calls    int
}

func (m *stubMarket) Home(_ context.Context, page string) (StorePage, error) {
	m.homeArg = page
	return m.page, m.err
}
func (m *stubMarket) Search(context.Context, StoreSearchQuery) (StoreSearchResults, error) {
	return StoreSearchResults{}, m.err
}
func (m *stubMarket) Offer(context.Context, string) (StoreOfferDetail, error) {
	return StoreOfferDetail{}, m.err
}
func (m *stubMarket) Balances(context.Context) ([]StoreBalance, error) { return nil, m.err }
func (m *stubMarket) MoreOffers(context.Context, string) (StoreRowMore, error) {
	return StoreRowMore{}, m.err
}
func (m *stubMarket) Entitlements(context.Context, int, int, bool) (StoreEntitlements, error) {
	return StoreEntitlements{Owned: []string{"a"}, Total: 1}, m.err
}
func (m *stubMarket) Purchase(_ context.Context, r StorePurchaseRequest) (StorePurchaseResult, error) {
	m.calls++
	m.purchase = r
	return m.result, m.err
}

func startMarket(t *testing.T, m Marketplace) string {
	t.Helper()
	dir := t.TempDir()
	server, err := Start(dir, NewStore())
	if err != nil {
		t.Fatal(err)
	}
	if m != nil {
		server.SetMarketplace(m)
	}
	t.Cleanup(func() { _ = server.Close() })
	return dir
}

const validPurchase = `{"purchase_id":"0123456789abcdef","offer_id":"offer-1","currency":"mc","amount":"320","confirmed":true}`

func TestStoreMethodsWithoutMarketplaceAreRejected(t *testing.T) {
	dir := startMarket(t, nil)
	if reply := rpc(t, dir, methodStoreBalance, ""); reply.Error == nil || reply.Error.Code != codeServicesDisabled {
		t.Fatalf("error = %+v", reply.Error)
	}
}

func TestStoreHomeDefaultsPageAndEncodesEmptyRows(t *testing.T) {
	m := &stubMarket{page: StorePage{ID: "store"}}
	dir := startMarket(t, m)
	raw := string(call(t, dir, methodStoreHome, ""))
	if m.homeArg != marketplace.PageStoreRoot || !strings.Contains(raw, `"rows":[]`) {
		t.Fatalf("page=%q response=%s", m.homeArg, raw)
	}
	if reply := rpc(t, dir, methodStoreHome, `{"page":"marketplacepass"}`); reply.Error != nil || m.homeArg != "marketplacepass" {
		t.Fatalf("named page: %+v arg=%q", reply.Error, m.homeArg)
	}
}

func TestStorePurchaseRequiresExplicitConfirmation(t *testing.T) {
	m := &stubMarket{result: StorePurchaseResult{Status: "purchased", CorrelationID: "c"}}
	dir := startMarket(t, m)
	for _, params := range []string{
		``, `{}`,
		strings.Replace(validPurchase, `"confirmed":true`, `"confirmed":false`, 1),
		strings.Replace(validPurchase, `,"confirmed":true`, ``, 1),
		strings.Replace(validPurchase, `"amount":"320"`, `"amount":320`, 1),
		strings.TrimSuffix(validPurchase, "}") + `,"extra":1}`,
	} {
		if reply := rpc(t, dir, methodStorePurchase, params); reply.Error == nil || reply.Error.Code != -32602 {
			t.Fatalf("params %q error = %+v", params, reply.Error)
		}
	}
	if m.calls != 0 {
		t.Fatalf("unconfirmed purchase reached the service %d times", m.calls)
	}
	var result storePurchaseResultV1
	reply := rpc(t, dir, methodStorePurchase, validPurchase)
	if reply.Error != nil || json.Unmarshal(reply.Result, &result) != nil || result.Status != "purchased" ||
		result.SchemaVersion != 1 || m.purchase.Amount != "320" || !m.purchase.Confirmed {
		t.Fatalf("purchase = %+v / %+v", result, reply.Error)
	}
}

func TestStoreErrorsAreSanitized(t *testing.T) {
	for _, test := range []struct {
		err  error
		code int
	}{
		{ErrSignedOut, codeSignedOut},
		{ErrStoreInvalidRequest, -32602},
		{ErrPurchaseBusy, codePurchaseBusy},
		{ErrPurchaseReused, codePurchaseReused},
		{marketplace.ErrUnknownPage, codeStoreNotFound},
		{errors.New(`Post https://x/y?token=SECRET: dial tcp`), codeServiceFailed},
	} {
		dir := startMarket(t, &stubMarket{err: test.err})
		reply := rpc(t, dir, methodStorePurchase, validPurchase)
		if reply.Error == nil || reply.Error.Code != test.code || strings.Contains(reply.Error.Message, "SECRET") {
			t.Fatalf("error for %v = %+v", test.err, reply.Error)
		}
	}
}

func TestFitPageKeepsResponsesInsideOneFrame(t *testing.T) {
	page := StorePage{ID: "store"}
	for r := 0; r < 40; r++ {
		row := StoreRow{ID: "r"}
		for o := 0; o < 40; o++ {
			row.Offers = append(row.Offers, StoreOffer{ID: "id", Title: strings.Repeat("t", 250), Creator: strings.Repeat("c", 250)})
		}
		page.Rows = append(page.Rows, row)
	}
	fitted := fitPage(page)
	if !fitted.Truncated || encodedLen(fitted) > frameBudget || len(fitted.Rows) == 0 {
		t.Fatalf("truncated=%v len=%d rows=%d", fitted.Truncated, encodedLen(fitted), len(fitted.Rows))
	}
}

// Store failures were never logged, so a Marketplace that could not open left no trace in the core log.
func TestStoreFailuresAreLoggedRedacted(t *testing.T) {
	dir := t.TempDir()
	server, err := Start(dir, NewStore())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = server.Close() })
	var logged strings.Builder
	server.SetLogger(slog.New(slog.NewTextHandler(&lockedWriter{w: &logged}, nil)))
	m := &stubMarket{err: errors.New("store: resolve entitlements service: token=eyJhbGciOi.eyJzdWIi.c2ln")}
	server.SetMarketplace(m)
	if reply := rpc(t, dir, methodStoreHome, ""); reply.Error == nil || reply.Error.Code != codeServiceFailed {
		t.Fatalf("error = %+v", reply.Error)
	}
	out := logged.String()
	if !strings.Contains(out, "method=store_home.v1") || !strings.Contains(out, "resolve entitlements service") {
		t.Fatalf("store failure not logged: %q", out)
	}
	if strings.Contains(out, "eyJ") {
		t.Fatalf("token leaked into the log: %q", out)
	}
	logged.Reset()
	m.err = fmt.Errorf("%w \"home\" (known pages: storeRoot)", marketplace.ErrUnknownPage)
	if reply := rpc(t, dir, methodStoreHome, ""); reply.Error == nil || reply.Error.Code != codeStoreNotFound {
		t.Fatalf("error = %+v", reply.Error)
	}
	if !strings.Contains(logged.String(), "known pages: storeRoot") {
		t.Fatalf("unknown page not logged with its known keys: %q", logged.String())
	}
	logged.Reset()
	m.err = ErrSignedOut
	if reply := rpc(t, dir, methodStoreBalance, ""); reply.Error == nil || reply.Error.Code != codeSignedOut {
		t.Fatalf("error = %+v", reply.Error)
	}
	if logged.Len() != 0 {
		t.Fatalf("signed out is not a service failure: %q", logged.String())
	}
}

type lockedWriter struct {
	mu sync.Mutex
	w  *strings.Builder
}

func (l *lockedWriter) Write(p []byte) (int, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.w.Write(p)
}
