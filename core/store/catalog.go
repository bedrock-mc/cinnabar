package store

import (
	"context"
	"strings"

	"github.com/sandertv/gophertunnel/minecraft/service/marketplace"
)

const (
	maxDescription   = 8192
	maxScreenshots   = 12
	maxThumbnailSize = 1024
)

// Search renders the store's search page for q.Term, or continues an earlier search from
// q.Continuation; offers are marked owned from the inventory. The service sets the page size.
func (c *Client) Search(ctx context.Context, q SearchQuery) (SearchResults, error) {
	if err := q.Validate(); err != nil {
		return SearchResults{}, err
	}
	var items []marketplace.Item
	var next string
	if q.Continuation != "" {
		var err error
		if items, next, err = c.continueRow(ctx, q.Continuation); err != nil {
			return SearchResults{}, err
		}
	} else {
		cfg, err := c.sessionConfig(ctx)
		if err != nil {
			return SearchResults{}, err
		}
		state, err := c.layoutState(ctx)
		if err != nil {
			return SearchResults{}, err
		}
		page, err := c.cfg.Market.Search(ctx, cfg, marketplace.SearchRequest{Search: q.Term}, state)
		if err != nil {
			return SearchResults{}, err
		}
		c.notePage(page)
		if results := page.Component(marketplace.ComponentPagedItemList); results != nil {
			items, next = results.Items, results.ContinuationToken
		}
	}
	out := SearchResults{}
	// The service sizes the page and its continuation starts after it, so every offer is kept.
	out.Offers, _ = c.offers(items, len(items))
	if ValidContinuation(next) {
		out.Continuation = next
	}
	return out, nil
}

// Offer returns one offer's detail from its store detail page.
func (c *Client) Offer(ctx context.Context, id string) (OfferDetail, error) {
	if !ValidOfferID(id) {
		return OfferDetail{}, ErrInvalidRequest
	}
	state, err := c.layoutState(ctx)
	if err != nil {
		return OfferDetail{}, err
	}
	page, err := c.cfg.Market.Page(ctx, marketplace.PageByProductID, id, state)
	if err != nil {
		return OfferDetail{}, err
	}
	c.notePage(page)
	summary := page.Component(marketplace.ComponentItemSummary)
	if summary == nil || summary.Item == nil {
		return OfferDetail{}, errNoOffer
	}
	offer, ok := offerFromMarketItem(summary.Item)
	if !ok {
		return OfferDetail{}, errNoOffer
	}
	if purchase := page.Component(marketplace.ComponentPurchaseInfo); purchase != nil && purchase.Price != nil {
		offer.Prices = []Price{priceOf(purchase.Price)}
	}
	if rating := page.Component(marketplace.ComponentRating); rating != nil && rating.Rating != nil && rating.Rating.TotalCount > 0 {
		offer.Rating = &Rating{Average: rating.Rating.Average, Count: rating.Rating.TotalCount}
	}
	offer.Owned = c.owned(offer.ID)
	detail := OfferDetail{Offer: offer}
	if packs := summary.Item.PackIdentity; len(packs) > 0 {
		detail.DisplayVersion = clip(packs[0].Version)
	}
	if description := page.Component(marketplace.ComponentItemDescription); description != nil {
		detail.Description = description.Description
		if len(detail.Description) > maxDescription {
			detail.Description = detail.Description[:maxDescription]
		}
	}
	if gallery := page.Component(marketplace.ComponentImageGallery); gallery != nil {
		for _, image := range gallery.Images {
			if safeImageURL(image.URL) && len(detail.ScreenshotURLs) < maxScreenshots {
				detail.ScreenshotURLs = append(detail.ScreenshotURLs, image.URL)
			}
		}
	}
	return detail, nil
}

// offers maps up to limit items, marking them owned; truncated reports items left out.
func (c *Client) offers(items []marketplace.Item, limit int) (offers []Offer, truncated bool) {
	offers = []Offer{}
	for i := range items {
		offer, ok := offerFromMarketItem(&items[i])
		if !ok {
			continue
		}
		if len(offers) >= limit {
			return offers, true
		}
		offer.Owned = c.owned(offer.ID)
		offers = append(offers, offer)
	}
	return offers, false
}

// offerFromMarketItem maps a store item to the bridge offer; an item without an id or title is skipped.
func offerFromMarketItem(item *marketplace.Item) (Offer, bool) {
	id := strings.ToLower(item.ID)
	title := clip(item.Title.Neutral())
	if !ValidOfferID(id) || title == "" {
		return Offer{}, false
	}
	offer := Offer{
		ID: id, Title: title, Creator: clip(item.CreatorName),
		ContentType: clip(item.ContentType), StoreID: clip(item.StoreID),
	}
	if thumbnail := item.ThumbnailURL(); safeImageURL(thumbnail) {
		offer.ThumbnailURL = thumbnail
	}
	if item.Rating != nil && item.Rating.TotalCount > 0 {
		offer.Rating = &Rating{Average: item.Rating.Average, Count: item.Rating.TotalCount}
	}
	if item.Price != nil {
		offer.Prices = []Price{priceOf(item.Price)}
	}
	for _, tag := range item.Tags {
		if tag.Name != "" && len(offer.Tags) < maxTagsPerOffer {
			offer.Tags = append(offer.Tags, clip(tag.Name))
		}
	}
	return offer, true
}

func priceOf(price *marketplace.Price) Price {
	return Price{Currency: price.CurrencyID, Amount: price.Amount()}
}

// safeImageURL reports whether the image cache may fetch url.
func safeImageURL(url string) bool {
	return strings.HasPrefix(url, "https://") && len(url) <= maxThumbnailSize
}
