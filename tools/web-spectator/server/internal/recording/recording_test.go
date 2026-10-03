package recording

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"image"
	"image/color"
	"image/png"
	"io"
	"log/slog"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/replay"
	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/spectator"
)

func harness(t *testing.T) (*Service, time.Time) {
	t.Helper()
	store, err := replay.Open(replay.Config{Directory: t.TempDir()})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(store.Close)
	s := &Service{store: store, live: spectator.NewStore(), log: slog.New(slog.NewTextHandler(io.Discard, nil)), queue: make(chan event, 128), stop: make(chan struct{}), active: map[string]*active{}, failed: map[string]time.Time{}}
	now := time.Now().Add(-time.Second)
	part := spectator.ArenaPart{Version: spectator.Version, Arena: spectator.Arena{ID: "arena", Name: "Arena", Bounds: [6]int32{0, 0, 0, 10, 10, 10}, Palette: []spectator.PaletteEntry{{Name: "minecraft:air"}, {Name: "minecraft:stone"}}, Blocks: [][4]int32{{1, 0, 1, 1}}}, Parts: 1}
	data, _ := json.Marshal(part)
	if err = s.live.Accept(spectator.ArenaSubject, data, now); err != nil {
		t.Fatal(err)
	}
	return s, now
}
func fixtureFrame(now time.Time) spectator.Frame {
	return spectator.Frame{Version: spectator.Version, ID: "match", ArenaID: "arena", Mode: "NoDebuff", UpdatedAt: now, MatchStartedAt: &now, Players: []spectator.Player{{ID: "one", Name: "One", Bot: true, Health: 20, MaxHealth: 20}, {ID: "two", Name: "Two", Health: 20, MaxHealth: 20, Team: 1}}, TeamWins: []int{0, 0}}
}
func enqueue(t *testing.T, s *Service, subject string, value any, stamp time.Time) event {
	t.Helper()
	data, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.live.Accept(subject, data, stamp); err != nil {
		t.Fatal(err)
	}
	s.Accept(subject, data, stamp)
	select {
	case e := <-s.queue:
		s.pending.Add(-e.bytes)
		return e
	default:
		t.Fatal("validated event was not queued")
		return event{}
	}
}
func enqueueOpening(t *testing.T, s *Service, frames ...spectator.Frame) event {
	t.Helper()
	start := spectator.ReplayStart{Version: spectator.Version, ID: frames[0].ID, ArenaID: frames[0].ArenaID, Frames: frames}
	data, err := json.Marshal(start)
	if err != nil {
		t.Fatal(err)
	}
	opening, arena, err := s.live.ValidateReplayStart(data, frames[len(frames)-1].UpdatedAt)
	if err != nil {
		t.Fatal(err)
	}
	s.AcceptReplayStart(opening, arena, data, frames[len(frames)-1].UpdatedAt)
	select {
	case e := <-s.queue:
		s.pending.Add(-e.bytes)
		return e
	default:
		t.Fatal("replay opening was not queued")
		return event{}
	}
}
func pngSkin(t *testing.T, now time.Time, tint uint8) (spectator.SkinAsset, []byte) {
	t.Helper()
	img := image.NewNRGBA(image.Rect(0, 0, 64, 64))
	img.Set(0, 0, color.NRGBA{R: tint, A: 255})
	var buffer bytes.Buffer
	if err := png.Encode(&buffer, img); err != nil {
		t.Fatal(err)
	}
	data := buffer.Bytes()
	hash := sha256.Sum256(data)
	return spectator.SkinAsset{Version: spectator.Version, ID: "match", PlayerID: "one", SkinID: hex.EncodeToString(hash[:]), PNG: base64.StdEncoding.EncodeToString(data), Model: "classic", Width: 64, Height: 64, UpdatedAt: now}, data
}
func finish(t *testing.T, s *Service, now time.Time, reason string) {
	t.Helper()
	s.consume(enqueue(t, s, spectator.ClosedSubject, map[string]any{"version": spectator.Version, "id": "match", "updatedAt": now, "reason": reason}, now))
}
func request(s *Service, path string) *httptest.ResponseRecorder {
	response := httptest.NewRecorder()
	NewHTTP(s.store).ServeHTTP(response, httptest.NewRequest("GET", path, nil))
	return response
}

func TestFinishedCaptureAndHTTPSeekFreezeFirstSkin(t *testing.T) {
	s, now := harness(t)
	frame := fixtureFrame(now)
	s.consume(enqueueOpening(t, s, frame))
	first, _ := pngSkin(t, now.Add(50*time.Millisecond), 10)
	s.consume(enqueue(t, s, spectator.SkinSubject, first, first.UpdatedAt))
	frame.UpdatedAt = now.Add(100 * time.Millisecond)
	frame.Players[0].SkinID = first.SkinID
	s.consume(enqueue(t, s, spectator.FrameSubject, frame, frame.UpdatedAt))
	second, secondPNG := pngSkin(t, now.Add(150*time.Millisecond), 20)
	s.consume(enqueue(t, s, spectator.SkinSubject, second, second.UpdatedAt))
	frame.UpdatedAt = now.Add(200 * time.Millisecond)
	frame.Players[0].SkinID = second.SkinID
	s.consume(enqueue(t, s, spectator.FrameSubject, frame, frame.UpdatedAt))
	finish(t, s, now.Add(250*time.Millisecond), "finished")
	metadata := request(s, "/api/replays/match")
	if metadata.Code != 200 {
		t.Fatal(metadata.Body.String())
	}
	var info View
	if err := json.Unmarshal(metadata.Body.Bytes(), &info); err != nil {
		t.Fatal(err)
	}
	if info.DurationMS != 200 || info.Players[0].SkinID != first.SkinID || len(info.SkinHashes) != 2 {
		t.Fatalf("metadata skin/time mismatch: %+v", info)
	}
	response := request(s, "/api/replays/match/frames?fromMs=0&toMs=250")
	var window struct {
		Frames []spectator.Frame `json:"frames"`
	}
	if response.Code != 200 || json.Unmarshal(response.Body.Bytes(), &window) != nil {
		t.Fatal(response.Body.String())
	}
	if len(window.Frames) != 3 || window.Frames[0].Players[0].SkinID != first.SkinID || window.Frames[2].Players[0].SkinID != second.SkinID {
		t.Fatal("frozen/mid-match skins were not preserved")
	}
	seek := request(s, "/api/replays/match/frames?fromMs=125&toMs=225")
	if err := json.Unmarshal(seek.Body.Bytes(), &window); err != nil {
		t.Fatal(err)
	}
	if len(window.Frames) != 2 || !window.Frames[0].UpdatedAt.Equal(now.Add(100*time.Millisecond)) {
		t.Fatal("seek did not retain preceding frame")
	}
	skin := request(s, "/api/replays/match/skins/"+second.SkinID)
	if skin.Code != 200 || !bytes.Equal(skin.Body.Bytes(), secondPNG) {
		t.Fatal("changed skin unavailable")
	}
	arenaAsSkin := request(s, "/api/replays/match/skins/"+info.ArenaHash)
	if arenaAsSkin.Code != 404 {
		t.Fatal("arena was exposed as a skin")
	}
	if request(s, "/api/replays/match/arena").Code != 200 {
		t.Fatal("arena unavailable")
	}
	if request(s, "/api/replays/match/frames?fromMs=0&toMs=10001").Code != 400 {
		t.Fatal("unbounded window admitted")
	}
	filtered := request(s, "/api/replays?player=one&limit=1")
	var listing struct {
		Total int `json:"total"`
	}
	_ = json.Unmarshal(filtered.Body.Bytes(), &listing)
	if listing.Total != 1 {
		t.Fatal("player filter failed")
	}
	absent := request(s, "/api/replays?player=Other")
	_ = json.Unmarshal(absent.Body.Bytes(), &listing)
	if listing.Total != 0 {
		t.Fatal("unrelated player matched")
	}
}

func TestFirstFrameKeepsSkinAfterLiveCacheCloses(t *testing.T) {
	s, now := harness(t)
	frame := fixtureFrame(now)
	s.consume(enqueueOpening(t, s, frame))
	skin, data := pngSkin(t, now, 42)
	s.consume(enqueue(t, s, spectator.SkinSubject, skin, now))
	frame.UpdatedAt = now.Add(50 * time.Millisecond)
	frame.Players[0].SkinID = skin.SkinID
	e := enqueue(t, s, spectator.FrameSubject, frame, frame.UpdatedAt)
	// The worker has not processed the first frame when the live cache closes.
	closed := enqueue(t, s, spectator.ClosedSubject, map[string]any{"version": spectator.Version, "id": "match", "updatedAt": now, "reason": "finished"}, now)
	s.consume(e)
	s.consume(closed)
	response := request(s, "/api/replays/match/skins/"+skin.SkinID)
	if response.Code != 200 || !bytes.Equal(response.Body.Bytes(), data) {
		t.Fatal("queued first-frame skin was lost")
	}
}

func TestRevokedCaptureNeverPublishes(t *testing.T) {
	s, now := harness(t)
	s.consume(enqueueOpening(t, s, fixtureFrame(now)))
	finish(t, s, now, "revoked")
	if len(s.store.List()) != 0 {
		t.Fatal("revoked duel became a replay")
	}
}

func TestIncompleteLiveSamplingCannotPublishReplay(t *testing.T) {
	s, now := harness(t)
	s.consume(enqueueOpening(t, s, fixtureFrame(now)))
	s.consume(enqueue(t, s, spectator.ClosedSubject, spectator.Closed{Version: spectator.Version, ID: "match", Reason: "finished", ReplayIncomplete: true, UpdatedAt: now.Add(time.Second)}, now.Add(time.Second)))
	if len(s.store.List()) != 0 {
		t.Fatal("failed live sampling left a partial saved replay")
	}
}

func TestObservedLiveAndTerminalGapsDiscardPartialReplay(t *testing.T) {
	for _, terminalGap := range []bool{false, true} {
		t.Run(map[bool]string{false: "live", true: "terminal"}[terminalGap], func(t *testing.T) {
			s, now := harness(t)
			frame := fixtureFrame(now)
			s.consume(enqueueOpening(t, s, frame))
			frame.UpdatedAt = now.Add(spectator.MaxReplayStartGap + time.Millisecond)
			if terminalGap {
				s.consume(enqueue(t, s, spectator.ClosedSubject, spectator.Closed{Version: spectator.Version, ID: frame.ID, Reason: "finished", UpdatedAt: frame.UpdatedAt, FinalFrame: &frame}, frame.UpdatedAt))
			} else {
				s.consume(enqueue(t, s, spectator.FrameSubject, frame, frame.UpdatedAt))
				finish(t, s, frame.UpdatedAt, "finished")
			}
			if len(s.store.List()) != 0 {
				t.Fatal("replay with a missing observed interval was published")
			}
		})
	}
}

func TestLostStreamCannotRestartPartialDuel(t *testing.T) {
	s, now := harness(t)
	frame := fixtureFrame(now)
	s.consume(enqueueOpening(t, s, frame))
	frame.UpdatedAt = now.Add(50 * time.Millisecond)
	old := enqueue(t, s, spectator.FrameSubject, frame, frame.UpdatedAt)
	s.Disconnect()
	s.handleLoss()
	if old.generation == s.generation.Load() {
		t.Fatal("loss did not invalidate queued frames")
	}
	if err := s.frame(old); err != nil {
		t.Fatal(err)
	}
	if len(s.active) != 0 {
		t.Fatal("ongoing failed match restarted")
	}
	frame.UpdatedAt = now.Add(100 * time.Millisecond)
	s.consume(enqueue(t, s, spectator.FrameSubject, frame, frame.UpdatedAt))
	finish(t, s, frame.UpdatedAt, "finished")
	if len(s.store.List()) != 0 {
		t.Fatal("partial recording published after loss")
	}
}

func TestJoiningCaptureMidMatchDoesNotPublishPartialReplay(t *testing.T) {
	s, now := harness(t)
	frame := fixtureFrame(now)
	frame.RoundActive = true
	s.consume(enqueue(t, s, spectator.FrameSubject, frame, now))
	frame.RoundActive = false
	frame.UpdatedAt = now.Add(time.Second)
	s.consume(enqueue(t, s, spectator.FrameSubject, frame, frame.UpdatedAt))
	finish(t, s, frame.UpdatedAt, "finished")
	if len(s.store.List()) != 0 {
		t.Fatal("partial recording became visible")
	}
}

func TestColdArenaOpeningPreservesElapsedTimeWithoutEnteringLiveCache(t *testing.T) {
	s, now := harness(t)
	opening := fixtureFrame(now)
	frames := make([]spectator.Frame, 0, 4)
	for index := 0; index < 4; index++ {
		frame := opening
		frame.UpdatedAt = now.Add(time.Duration(index) * 450 * time.Millisecond)
		frame.RoundActive = index >= 2
		frames = append(frames, frame)
	}
	s.consume(enqueueOpening(t, s, frames...))
	if len(s.live.List(now.Add(1500*time.Millisecond))) != 0 {
		t.Fatal("historical replay frames appeared in the live spectator cache")
	}
	current := opening
	current.UpdatedAt = now.Add(1800 * time.Millisecond)
	current.RoundActive = true
	s.consume(enqueue(t, s, spectator.FrameSubject, current, current.UpdatedAt))
	final := current
	final.UpdatedAt = now.Add(2 * time.Second)
	s.consume(enqueue(t, s, spectator.ClosedSubject, spectator.Closed{Version: spectator.Version, ID: final.ID, Reason: "finished", UpdatedAt: final.UpdatedAt, FinalFrame: &final}, final.UpdatedAt))
	records := s.store.List()
	if len(records) != 1 || records[0].DurationMS != 2000 {
		t.Fatalf("cold arena replay lost its real opening time: %+v", records)
	}
	response := request(s, "/api/replays/match/frames?fromMs=0&toMs=2000")
	var window struct {
		Frames []spectator.Frame `json:"frames"`
	}
	if response.Code != 200 || json.Unmarshal(response.Body.Bytes(), &window) != nil || len(window.Frames) != 6 || !window.Frames[0].UpdatedAt.Equal(now) {
		t.Fatal("cold arena opening frames were not retained in order")
	}
}

func TestMissingOpeningCannotStartCapture(t *testing.T) {
	s, now := harness(t)
	frame := fixtureFrame(now)
	frame.RoundActive = true
	s.consume(enqueue(t, s, spectator.FrameSubject, frame, now))
	frame.UpdatedAt = now.Add(time.Second)
	s.consume(enqueue(t, s, spectator.FrameSubject, frame, frame.UpdatedAt))
	finish(t, s, frame.UpdatedAt, "finished")
	if len(s.store.List()) != 0 {
		t.Fatal("live frames without the opening batch became a partial replay")
	}
}

func TestTerminalFrameContainsFinalHealthAndResult(t *testing.T) {
	s, now := harness(t)
	frame := fixtureFrame(now)
	s.consume(enqueueOpening(t, s, frame))
	frame.UpdatedAt = now.Add(500 * time.Millisecond)
	frame.Players[1].Health = 0
	frame.Players[1].Dead = true
	frame.TeamWins[0] = 1
	s.consume(enqueue(t, s, spectator.ClosedSubject, spectator.Closed{Version: spectator.Version, ID: frame.ID, Reason: "finished", UpdatedAt: frame.UpdatedAt, FinalFrame: &frame}, frame.UpdatedAt))
	records := s.store.List()
	if len(records) != 1 || records[0].DurationMS != 500 {
		t.Fatalf("terminal recording: %+v", records)
	}
	frames, err := s.store.ReadChunk(frame.ID, 0)
	if err != nil {
		t.Fatal(err)
	}
	var final spectator.Frame
	if err = json.Unmarshal(frames[len(frames)-1].Payload, &final); err != nil {
		t.Fatal(err)
	}
	if final.Players[1].Health != 0 || !final.Players[1].Dead || final.TeamWins[0] != 1 {
		t.Fatal("final outcome missing")
	}
}
