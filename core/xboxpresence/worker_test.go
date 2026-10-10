package xboxpresence

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/df-mc/go-xsapi/v2/presence"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/auth"
)

func TestWorldAndExperienceIDs(t *testing.T) {
	for _, test := range []struct {
		state State
		want  string
	}{
		{State{}, "Menus"},
		{State{InWorld: true}, "Survival"},
		{State{InWorld: true, GameMode: 1}, "Creative"},
		{State{InWorld: true, GameMode: 2}, "Adventure"},
		{State{InWorld: true, GameMode: 6}, "Survival"},
		{State{InWorld: true, Realm: true}, "Realm_Survival"},
		{State{InWorld: true, Realm: true, GameMode: 1}, "Realm_Creative"},
		{State{InWorld: true, Realm: true, GameMode: 2}, "Realm_Adventure"},
		{State{InWorld: true, Experience: true}, "COM_Experience"},
		{State{InWorld: true, Experience: true, Realm: true, GameMode: 1}, "COM_Experience"},
		{State{Experience: true}, "Menus"},
	} {
		if got := test.state.ID(); got != test.want {
			t.Fatalf("%+v: %s, want %s", test.state, got, test.want)
		}
	}
}

func TestPresenceTriggersAndTitleCleanup(t *testing.T) {
	requests := make(chan presence.TitleRequest, 16)
	closed := make(chan struct{})
	var once sync.Once
	worker := New(context.Background(), func(context.Context) (*Client, error) {
		return &Client{
			Update: func(_ context.Context, request presence.TitleRequest) (*presence.UpdateResult, error) {
				requests <- request
				return &presence.UpdateResult{HeartbeatAfter: time.Hour}, nil
			},
			Close: func(ctx context.Context) error {
				if _, bounded := ctx.Deadline(); !bounded {
					t.Error("title cleanup has no deadline")
				}
				if ctx.Err() != nil {
					t.Error("shutdown canceled title cleanup before it started")
				}
				once.Do(func() { close(closed) })
				return nil
			},
		}, nil
	}, nil)
	defer worker.Close()
	expect := func(want string) {
		t.Helper()
		request := receive(t, requests)
		if request.Activity.RichPresence.ID != want {
			t.Fatalf("ID = %s, want %s", request.Activity.RichPresence.ID, want)
		}
		if request.ID != uint32(auth.AndroidConfig.TitleID) {
			t.Fatalf("wrong authenticated title")
		}
		if request.Activity.RichPresence.ServiceConfigID == uuid.Nil {
			t.Fatal("missing service configuration")
		}
	}
	expect("Menus")
	worker.Set(State{InWorld: true})
	expect("Survival")
	worker.Set(State{InWorld: true, GameMode: 1})
	expect("Creative")
	worker.Set(State{InWorld: true, GameMode: 2, Realm: true})
	expect("Realm_Adventure")
	worker.Set(State{InWorld: true, Experience: true})
	expect("COM_Experience")
	worker.Set(State{})
	expect("Menus")
	worker.Close()
	receive(t, closed)
}

func TestNewStateCancelsOldUpdate(t *testing.T) {
	started := make(chan string, 2)
	worker := New(context.Background(), func(context.Context) (*Client, error) {
		return &Client{
			Update: func(ctx context.Context, request presence.TitleRequest) (*presence.UpdateResult, error) {
				id := request.Activity.RichPresence.ID
				started <- id
				if id == "Menus" {
					<-ctx.Done()
					return nil, ctx.Err()
				}
				return &presence.UpdateResult{HeartbeatAfter: time.Hour}, nil
			},
			Close: func(context.Context) error { return nil },
		}, nil
	}, nil)
	defer worker.Close()
	if got := receive(t, started); got != "Menus" {
		t.Fatal(got)
	}
	worker.Set(State{InWorld: true, GameMode: 1})
	if got := receive(t, started); got != "Creative" {
		t.Fatal(got)
	}
}

func TestFailedUpdateDoesNotLoseLaterState(t *testing.T) {
	started := make(chan string, 2)
	worker := New(context.Background(), func(context.Context) (*Client, error) {
		return &Client{
			Update: func(_ context.Context, request presence.TitleRequest) (*presence.UpdateResult, error) {
				started <- request.Activity.RichPresence.ID
				return nil, errors.New("unavailable")
			},
			Close: func(context.Context) error { return nil },
		}, nil
	}, nil)
	defer worker.Close()
	receive(t, started)
	worker.Set(State{InWorld: true, GameMode: 2})
	if got := receive(t, started); got != "Adventure" {
		t.Fatal(got)
	}
}

func TestOfflineWorkerDoesNoServiceWork(t *testing.T) {
	worker := New(context.Background(), nil, nil)
	worker.Set(State{InWorld: true})
	worker.Close()
}

func TestHeartbeatUsesServiceDeadline(t *testing.T) {
	if got := heartbeatDelay(&presence.UpdateResult{HeartbeatAfter: 79 * time.Second}); got != 79*time.Second {
		t.Fatal(got)
	}
	if got := heartbeatDelay(nil); got != fallbackHeartbeat {
		t.Fatal(got)
	}
	if got := heartbeatDelay(&presence.UpdateResult{}); got != fallbackHeartbeat {
		t.Fatal(got)
	}
}

// receive bounds a failed asynchronous test without asserting scheduler timing.
func receive[T any](t *testing.T, values <-chan T) T {
	t.Helper()
	select {
	case value := <-values:
		return value
	case <-time.After(5 * time.Second):
		t.Fatal("presence worker did not finish its operation")
		var zero T
		return zero
	}
}
