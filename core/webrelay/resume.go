package webrelay

import "time"

// resumeBearer keeps each issued token's expiry independent of later rotations.
type resumeBearer struct {
	session *webSession
	expires time.Time
}

// resumeLocked returns the account for an unexpired bearer while Relay.mu is held.
func (relay *Relay) resumeLocked(token string, now time.Time) *webSession {
	bearer, ok := relay.tokens[token]
	if !ok || !now.Before(bearer.expires) {
		return nil
	}
	return bearer.session
}

// rotateLocked issues a replacement while retaining only the supplied bearer at its original
// expiry. Resumes retain the presented token, so repeated lost welcomes cannot discard it.
func (relay *Relay) rotateLocked(session *webSession, retain string, now time.Time) {
	if session.token != retain {
		delete(relay.tokens, session.token)
	}
	if session.previousToken != retain {
		delete(relay.tokens, session.previousToken)
	}
	session.previousToken = retain
	session.token = newSessionID()
	relay.tokens[session.token] = resumeBearer{session: session, expires: now.Add(resumeTTL)}
	session.tokenPending = true
}

// acknowledge accepts only the current, unexpired bearer. Echoing it prevents a delayed pong
// from acknowledging a later replacement that the browser has not received.
func (relay *Relay) acknowledge(controller *sessionController, token string, now time.Time) bool {
	relay.mu.Lock()
	defer relay.mu.Unlock()
	session := controller.session
	if session.controller != controller || controller.ctx.Err() != nil || token != session.token || relay.resumeLocked(token, now) != session {
		return false
	}
	session.tokenPending = false
	return true
}
