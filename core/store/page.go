package store

import (
	"context"
	"regexp"

	"github.com/sandertv/gophertunnel/minecraft/service/marketplace"
)

const (
	maxPageRows     = 60
	maxRowOffers    = 40
	maxStringLen    = 256
	maxTagsPerOffer = 8
)

var pagePattern = regexp.MustCompile(`^[A-Za-z0-9_.-]{1,64}$`)

// Home loads a known store page by its session-config name; its rows list their offers inline.
func (c *Client) Home(ctx context.Context, name string) (Page, error) {
	if !pagePattern.MatchString(name) {
		return Page{}, ErrInvalidRequest
	}
	cfg, err := c.sessionConfig(ctx)
	if err != nil {
		return Page{}, err
	}
	id, err := cfg.PageID(name)
	if err != nil {
		return Page{}, err
	}
	state, err := c.layoutState(ctx)
	if err != nil {
		return Page{}, err
	}
	layout, err := c.cfg.Market.Page(ctx, marketplace.PageByID, id, state)
	if err != nil {
		return Page{}, err
	}
	page := Page{ID: name, Rows: []Row{}, InventoryVersion: c.cfg.Market.InventoryVersion()}
	for _, section := range layout.Layout {
		for i := range section.Rows {
			row := &section.Rows[i]
			list := row.ItemList()
			if list == nil || len(list.Items) == 0 {
				continue
			}
			if len(page.Rows) >= maxPageRows {
				page.Truncated = true
				return page, nil
			}
			// The controlId names the vanilla row factory (StoreRow, HeroRow, CoinBundleRow, ...).
			out := Row{ID: clip(row.TelemetryID), Title: clip(row.Title()), Kind: clip(row.ControlID)}
			out.Offers, _ = c.offers(list.Items, maxRowOffers)
			if len(out.Offers) > 0 {
				page.Rows = append(page.Rows, out)
			}
		}
	}
	return page, nil
}

// layoutState returns the state layout pages are rendered against, loading the inventory first.
func (c *Client) layoutState(ctx context.Context) (marketplace.PageRequest, error) {
	inv, err := c.loadInventory(ctx, false)
	if err != nil {
		return marketplace.PageRequest{}, err
	}
	return c.cfg.Market.PageState(inv.ids), nil
}

func clip(s string) string {
	if len(s) <= maxStringLen {
		return s
	}
	cut := maxStringLen
	for cut > 0 && s[cut]&0xC0 == 0x80 { // do not split a UTF-8 sequence
		cut--
	}
	return s[:cut]
}
