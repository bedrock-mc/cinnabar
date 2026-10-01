package ingress

import (
	"encoding/json"
	"fmt"
	"net/http"
	"time"

	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/spectator"
)

func (h *Handler) events(w http.ResponseWriter, r *http.Request, id string, live *spectator.Live) {
	if r.Method == http.MethodHead {
		if _, ok := live.Snapshot(time.Now()); !ok {
			failure(w, http.StatusNotFound, "This duel is no longer available to watch.")
			return
		}
		w.Header().Set("Content-Type", "text/event-stream")
		w.WriteHeader(http.StatusOK)
		return
	}
	sub := live.Subscribe(time.Now())
	if sub == nil {
		failure(w, http.StatusNotFound, "This duel is no longer available to watch.")
		return
	}
	defer sub.Cancel()
	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-store")
	w.Header().Set("X-Accel-Buffering", "no")
	controller := http.NewResponseController(w)
	// SSE is long-lived; each write gets a separate, bounded deadline below.
	if err := controller.SetWriteDeadline(time.Time{}); err != nil {
		failure(w, http.StatusInternalServerError, "Streaming is unavailable.")
		return
	}
	if !sendFrame(controller, w, live) {
		return
	}
	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()
	for {
		select {
		case <-r.Context().Done():
			return
		case <-sub.Closed:
			sendClosed(controller, w, id)
			return
		case <-sub.Updates:
			if !sendFrame(controller, w, live) {
				sendClosed(controller, w, id)
				return
			}
		case <-ticker.C:
			if _, ok := live.Snapshot(time.Now()); !ok {
				sendClosed(controller, w, id)
				return
			}
		}
	}
}

func sendFrame(controller *http.ResponseController, w http.ResponseWriter, live *spectator.Live) bool {
	if err := controller.SetWriteDeadline(time.Now().Add(writeTimeout)); err != nil {
		return false
	}
	valid, err := live.WithCurrent(time.Now(), func(frame spectator.Frame, _ *spectator.Arena) error {
		encoded, err := json.Marshal(frame)
		if err != nil {
			return err
		}
		if _, err := fmt.Fprintf(w, "event: frame\ndata: %s\n\n", encoded); err != nil {
			return err
		}
		return controller.Flush()
	})
	return valid && err == nil
}

func sendClosed(controller *http.ResponseController, w http.ResponseWriter, id string) {
	if controller.SetWriteDeadline(time.Now().Add(writeTimeout)) != nil {
		return
	}
	encoded, _ := json.Marshal(map[string]string{"id": id})
	if _, err := fmt.Fprintf(w, "event: closed\ndata: %s\n\n", encoded); err == nil {
		_ = controller.Flush()
	}
}
