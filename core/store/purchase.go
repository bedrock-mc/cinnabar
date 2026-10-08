package store

import (
	"context"
	"strconv"
	"sync"
	"time"

	"github.com/sandertv/gophertunnel/minecraft/service/marketplace"
)

const (
	guardTTL      = time.Hour
	guardMax      = 256
	uncertainHold = 2 * time.Minute
)

type purchaseEntry struct {
	fingerprint string
	offerID     string
	done        chan struct{}
	result      PurchaseResult
	err         error
	at          time.Time
}

// purchaseGuard makes a purchase idempotent per purchase_id and exclusive per offer.
type purchaseGuard struct {
	mu   sync.Mutex
	now  func() time.Time
	byID map[string]*purchaseEntry
	busy map[string]time.Time // offer id -> hold expiry; zero time while in flight
}

func newPurchaseGuard(now func() time.Time) *purchaseGuard {
	return &purchaseGuard{now: now, byID: map[string]*purchaseEntry{}, busy: map[string]time.Time{}}
}

func fingerprint(r PurchaseRequest) string {
	duration := ""
	if r.UnitDurationSeconds != nil {
		duration = strconv.FormatUint(*r.UnitDurationSeconds, 10)
	}
	return r.OfferID + "|" + r.StoreID + "|" + r.Currency + "|" + r.Amount + "|" + duration
}

// begin registers the attempt; it returns the existing entry for a replayed id, or a new one this caller owns.
func (g *purchaseGuard) begin(r PurchaseRequest) (entry *purchaseEntry, owner bool, err error) {
	g.mu.Lock()
	defer g.mu.Unlock()
	now := g.now()
	g.prune(now)
	if existing, ok := g.byID[r.PurchaseID]; ok {
		if existing.fingerprint != fingerprint(r) {
			return nil, false, ErrPurchaseReused
		}
		return existing, false, nil
	}
	if until, held := g.busy[r.OfferID]; held && (until.IsZero() || now.Before(until)) {
		return nil, false, ErrPurchaseBusy
	}
	entry = &purchaseEntry{fingerprint: fingerprint(r), offerID: r.OfferID, done: make(chan struct{}), at: now}
	g.byID[r.PurchaseID] = entry
	g.busy[r.OfferID] = time.Time{}
	return entry, true, nil
}

// finish records the outcome and releases the offer, or holds it while the outcome is unknown.
func (g *purchaseGuard) finish(e *purchaseEntry, res PurchaseResult, err error) {
	g.mu.Lock()
	e.result, e.err = res, err
	if res.Status == PurchaseUnknown {
		g.busy[e.offerID] = g.now().Add(uncertainHold)
	} else {
		delete(g.busy, e.offerID)
	}
	g.mu.Unlock()
	close(e.done)
}

func (g *purchaseGuard) prune(now time.Time) {
	for id, e := range g.byID {
		select {
		case <-e.done:
			if now.Sub(e.at) > guardTTL || len(g.byID) > guardMax {
				delete(g.byID, id)
			}
		default:
		}
	}
	for offer, until := range g.busy {
		if !until.IsZero() && !now.Before(until) {
			delete(g.busy, offer)
		}
	}
}

// Purchase buys an offer with virtual currency through the store service. It is sent at most once per
// purchase_id; a replay returns the recorded outcome, and the offer is locked while one is in flight.
func (c *Client) Purchase(ctx context.Context, r PurchaseRequest) (PurchaseResult, error) {
	if err := r.Validate(); err != nil {
		return PurchaseResult{}, err
	}
	entry, owner, err := c.guard.begin(r)
	if err != nil {
		return PurchaseResult{}, err
	}
	if !owner {
		select {
		case <-entry.done:
			res := entry.result
			res.Replayed = true
			return res, entry.err
		case <-ctx.Done():
			return PurchaseResult{}, ctx.Err()
		}
	}
	res, err := c.sendPurchase(ctx, r)
	c.guard.finish(entry, res, err)
	return res, err
}

func (c *Client) sendPurchase(ctx context.Context, r PurchaseRequest) (PurchaseResult, error) {
	amount, err := strconv.ParseUint(r.Amount, 10, 64)
	if err != nil {
		return PurchaseResult{}, ErrInvalidRequest
	}
	sent, err := c.cfg.Market.PurchaseVirtual(ctx, marketplace.Purchase{
		OfferID: r.OfferID, StoreID: r.StoreID, Amount: amount, UnitDurationSeconds: r.UnitDurationSeconds,
	})
	if err != nil {
		return PurchaseResult{}, err // not sent
	}
	res := PurchaseResult{CorrelationID: sent.CorrelationID, HTTPStatus: sent.StatusCode, InventoryVersion: sent.InventoryETag}
	switch sent.Outcome {
	case marketplace.PurchaseSucceeded:
		res.Status = PurchaseOK
	case marketplace.PurchasePriceMismatch:
		res.Status = PurchasePriceRefused
	case marketplace.PurchasePreconditionFailed:
		res.Status = PurchaseStaleState
	case marketplace.PurchaseFailed:
		res.Status = PurchaseFailed
	default:
		// The request may or may not have reached the service; never report failure as definitive.
		res.Status = PurchaseUnknown
		c.invalidateInventory()
		return res, nil
	}
	if res.Status == PurchaseOK || res.Status == PurchaseStaleState {
		c.invalidateInventory()
		if res.Status == PurchaseOK {
			c.RefreshInventory(ctx)
		}
	}
	return res, nil
}
