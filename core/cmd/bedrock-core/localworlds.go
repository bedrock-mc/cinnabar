package main

import (
	"context"

	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
)

// worldSentinels are the local-world failures the control plane reports by code.
var worldSentinels = []sentinelPair{
	{control.ErrWorldInvalid, localworld.ErrInvalid},
	{control.ErrWorldNotOpen, localworld.ErrNotOpen},
}

func worldErr(err error) error { return classify(err, &worldSentinels) }

// controlWorlds routes the launcher's open local world reports and serves friend invites.
type controlWorlds struct {
	local  *localworld.Local
	onOpen func()                              // runs each time a world begins opening
	invite func(context.Context, string) error // nil while friends cannot be invited
}

func (w controlWorlds) SetLocal(report control.LocalWorld) error {
	opened, err := w.local.SetLocal(localStatus(report))
	if opened && w.onOpen != nil {
		w.onOpen()
	}
	return worldErr(err)
}

// localStatus is the route a local_world.v1 report describes.
func localStatus(report control.LocalWorld) localworld.Status {
	status := localworld.Status{State: localworld.State(report.State), Error: report.Error}
	if world := report.World; world != nil {
		status.World = &localworld.World{ID: world.ID, Name: world.Name, GameMode: world.GameMode, Backend: world.Backend, MultiplayerDisabled: world.MultiplayerDisabled}
	}
	if target := report.Target; target != nil {
		status.Target = &localworld.ConnectionTarget{
			Address: target.Address, Transport: localworld.Transport(target.Transport),
			LANAddress: target.LANAddress, LevelName: target.LevelName,
		}
	}
	return status
}

func (w controlWorlds) Invite(ctx context.Context, xuid string) error {
	if w.invite == nil {
		return control.ErrWorldNotOpen
	}
	return worldErr(w.invite(ctx, xuid))
}
