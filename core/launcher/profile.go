package launcher

import (
	"context"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
)

// Profile returns the signed-in profile with its gamerpic cached.
func (s *Service) Profile(ctx context.Context) (result catalog.Profile, resultErr error) {
	ctx = catalog.WithProfileObserver(ctx, s.logProfileRequest)
	finishProfile := catalog.ObserveProfileRequest(ctx, "profile")
	partialOutcome := false
	defer func() {
		if resultErr == nil && partialOutcome {
			finishProfile(catalog.ErrProfilePartial)
		} else {
			finishProfile(resultErr)
		}
	}()
	src, err := s.source()
	if err != nil {
		return catalog.Profile{}, err
	}
	finish := catalog.ObserveProfileRequest(ctx, "account")
	profile, err := s.cfg.Profile(ctx, src)
	if err == nil && profile.Partial() != nil {
		finish(catalog.ErrProfilePartial)
		partialOutcome = true
	} else {
		finish(err)
	}
	if err != nil {
		return catalog.Profile{}, err
	}
	if partial := profile.Partial(); partial != nil {
		s.logger.Warn("profile partly unavailable", "error", control.RedactError(partial))
	}
	if s.cfg.ArtworkDir != "" && profile.XUID != "" {
		finish = catalog.ObserveProfileRequest(ctx, "avatar")
		avatar, err := s.cfg.ProfileAvatar(ctx, src, profile.XUID, s.cfg.ArtworkDir)
		finish(err)
		if err != nil {
			profile.AvatarError = true
			partialOutcome = true
			s.logger.Warn("profile avatar unavailable", "error", control.RedactError(err))
		} else {
			profile.Avatar = avatar
		}
	} else {
		reason := "no_xuid"
		if s.cfg.ArtworkDir == "" {
			reason = "cache_disabled"
		}
		s.logProfileRequest(catalog.ProfileRequestEvent{Facet: "avatar", Outcome: "skipped", Reason: reason})
	}
	if profile.XUID != "" {
		finish = catalog.ObserveProfileRequest(ctx, "featured_screenshot")
		screenshot, err := s.cfg.ProfileFeaturedScreenshot(ctx, src, profile.XUID)
		finish(err)
		if err != nil {
			profile.FeaturedScreenshotError = true
			partialOutcome = true
			s.logger.Warn("profile featured screenshot unavailable", "error", control.RedactError(err))
		} else {
			profile.FeaturedScreenshot = screenshot
		}
	} else {
		s.logProfileRequest(catalog.ProfileRequestEvent{Facet: "featured_screenshot", Outcome: "skipped", Reason: "no_xuid"})
	}
	images := []*catalog.Image{&profile.Gamerpic, &profile.FeaturedScreenshot}
	if profile.Achievements != nil {
		for _, entry := range catalog.ProfileOverviewAchievements(profile.Achievements.Entries) {
			images = append(images, &entry.Image)
		}
	}
	started := time.Now()
	s.logProfileRequest(catalog.ProfileRequestEvent{Facet: "artwork", Outcome: "request"})
	s.cacheArt(ctx, images)
	art := profileArtworkOutcome(images, ctx.Err(), s.cfg.ArtworkDir != "")
	art.Elapsed = time.Since(started)
	s.logProfileRequest(art)
	partialOutcome = partialOutcome || (art.Outcome != "skipped" && art.Outcome != "ok")
	s.mu.Lock()
	s.gamerpic = profile.Gamerpic.Path
	s.profileArt = []string{profile.Avatar.Path}
	for _, image := range images {
		s.profileArt = append(s.profileArt, image.Path)
	}
	s.mu.Unlock()
	return profile, nil
}
