package proxy

import "sync"

// An admission blocks speculative preparation until its exact resource owner closes.
func (selector *UpstreamSelector) beginAdmission() func() {
	if selector == nil {
		return func() {}
	}
	selector.mu.Lock()
	selector.admissions++
	if selector.preparation != nil {
		selector.preparation.keep(selector.target)
	}
	selector.mu.Unlock()
	var releaseOnce sync.Once
	return func() {
		releaseOnce.Do(func() {
			selector.mu.Lock()
			selector.admissions--
			selector.mu.Unlock()
		})
	}
}
